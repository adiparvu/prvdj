import CPRVBridge
import Foundation

// `BitDepth` is not declared here. It already exists in `Delivery.swift`,
// because deciding what depth to deliver at and writing at that depth are two
// halves of the same question — and a second enum would be a second place for
// the two halves to disagree.

/// What an export produced, and what it had to do to get there.
public struct ExportOutcome: Sendable, Equatable {
    /// How many bytes of audio were written.
    public let bytes: UInt64
    /// How many samples were limited to full scale.
    ///
    /// Non-zero means the mix clipped on the way out. Worth telling the user:
    /// nothing in this system is applied silently, and clamping is something
    /// applied.
    public let clipped: UInt64
    /// Whether noise was actually added.
    ///
    /// What happened, not what was asked for. Dither into a floating-point file
    /// is refused, and an export screen should say what it did.
    public let dithered: Bool
    /// Bytes per sample.
    public let sampleWidth: UInt32
    /// Whether the samples are floating point.
    public let isFloat: Bool

    /// Whether the file is fine to hand to somebody.
    public var clippedAnything: Bool { clipped > 0 }
}

/// One export in progress.
///
/// # Why this is a handle rather than a function
///
/// Dither is noise added before rounding, and it works only because the noise
/// differs from sample to sample. A per-block function would restart its noise
/// every block — and a pattern repeating every 512 samples at 48 kHz is a tone
/// at ninety-four hertz sitting under the whole file.
public final class Export {
    private let handle: OpaquePointer

    /// Begins an export.
    ///
    /// - Parameters:
    ///   - dither: should come from an export report rather than a preference.
    ///     Whether to dither is decided by what the depth does to the mix and
    ///     where the file is going, and both are already answered.
    ///   - seed: makes the noise reproducible. The same project and the same
    ///     seed produce the same file, byte for byte.
    public init(depth: BitDepth, dither: Bool, seed: UInt64, channels: UInt32) throws {
        var created: OpaquePointer?
        try EngineError.check(
            prv_export_begin(depth.code, dither ? 1 : 0, seed, channels, &created)
        )
        guard let created else { throw EngineError.nullPointer }
        handle = created
    }

    deinit { prv_export_destroy(handle) }

    /// How many bytes a block of this many frames will produce.
    ///
    /// Ask once and allocate once: the answer does not change.
    public func blockBytes(frames: UInt32) throws -> UInt64 {
        var value: UInt64 = 0
        try EngineError.check(prv_export_block_bytes(handle, frames, &value))
        return value
    }

    /// Converts one rendered block into the bytes a file holds.
    ///
    /// `planar` is channel-major, as ``Engine/render(at:into:channels:frames:)``
    /// produced it. The result is interleaved and little-endian.
    public func block(_ planar: [Float], frames: UInt32) throws -> [UInt8] {
        let capacity = try blockBytes(frames: frames)
        var into = [UInt8](repeating: 0, count: Int(capacity))
        var written: UInt64 = 0

        try planar.withUnsafeBufferPointer { source in
            try into.withUnsafeMutableBufferPointer { destination in
                try EngineError.check(
                    prv_export_block(
                        handle,
                        source.baseAddress,
                        frames,
                        destination.baseAddress,
                        UInt64(destination.count),
                        &written
                    )
                )
            }
        }
        return Array(into.prefix(Int(written)))
    }

    /// What has been produced so far.
    public func outcome() throws -> ExportOutcome {
        var bytes: UInt64 = 0
        var clipped: UInt64 = 0
        var dithered: Int32 = 0
        var width: UInt32 = 0
        var isFloat: Int32 = 0
        try EngineError.check(
            prv_export_status(handle, &bytes, &clipped, &dithered, &width, &isFloat)
        )
        return ExportOutcome(
            bytes: bytes,
            clipped: clipped,
            dithered: dithered != 0,
            sampleWidth: width,
            isFloat: isFloat != 0
        )
    }
}
