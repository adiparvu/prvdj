import Foundation
import PRVCore

/// Writes a WAVE file.
///
/// # Why this is here and not in the core
///
/// A WAVE header is forty-four bytes of arithmetic. No audio decision depends on
/// it: the samples arrive already quantised and already dithered, because how a
/// floating-point value becomes sixteen bits is audible when it is wrong and the
/// core owns it. What is left is a container format, which is exactly the kind
/// of thing ADR-0001 puts outside.
///
/// # Why it is written by hand rather than through AVFoundation
///
/// Because this way it compiles and is tested on every commit. `AVAudioFile`
/// would do the same job and would join the four types nothing has ever
/// compiled — and export is the feature where a silent mistake produces a file
/// somebody hands to a club.
///
/// The format is also genuinely small. RIFF is a length, a tag, and chunks; the
/// only subtleties are that everything is little-endian and that a chunk with an
/// odd length is padded, and both are handled below.
///
/// # Streaming, not assembling
///
/// The header is written first with the sizes left blank, blocks are appended as
/// they are rendered, and the two length fields are patched at the end. A
/// forty-minute set at 24 bits is about seven hundred megabytes; holding it in
/// memory to count it first would be a way to fail on a laptop.
public final class WaveWriter {
    private let handle: FileHandle
    private let url: URL
    private let channels: UInt32
    private let sampleRate: UInt32
    private let bitsPerSample: UInt32
    private let isFloat: Bool
    private var audioBytes: UInt32 = 0
    private var isOpen = true

    /// Where the two sizes live, so they can be patched when the length is known.
    private static let riffSizeOffset: UInt64 = 4
    private static let dataSizeOffset: UInt64 = 40

    /// Begins a file.
    ///
    /// - Parameters:
    ///   - sampleWidth: bytes per sample, from `prv_export_status`.
    ///   - isFloat: whether the samples are floating point. The format tag
    ///     differs, and a file that claims integers and holds floats is noise at
    ///     full scale.
    public init(
        url: URL,
        channels: UInt32,
        sampleRate: UInt32,
        sampleWidth: UInt32,
        isFloat: Bool
    ) throws {
        guard channels > 0, sampleRate > 0, sampleWidth > 0 else {
            throw PlatformError.unsupportedFormat("a file needs channels, a rate and a width")
        }
        self.url = url
        self.channels = channels
        self.sampleRate = sampleRate
        self.bitsPerSample = sampleWidth * 8
        self.isFloat = isFloat

        FileManager.default.createFile(atPath: url.path, contents: nil)
        guard let handle = try? FileHandle(forWritingTo: url) else {
            throw PlatformError.cannotRead(url.lastPathComponent)
        }
        self.handle = handle
        try write(Self.header(
            channels: channels,
            sampleRate: sampleRate,
            bitsPerSample: bitsPerSample,
            isFloat: isFloat
        ))
    }

    /// Appends one block of already-quantised bytes.
    public func append(_ bytes: [UInt8]) throws {
        guard isOpen else {
            throw PlatformError.unsupportedFormat("the file is already finished")
        }
        try write(Data(bytes))
        audioBytes = audioBytes.addingReportingOverflow(UInt32(bytes.count)).partialValue
    }

    /// Patches the sizes and closes the file.
    ///
    /// Must be called. A file left unfinished has a header claiming zero bytes
    /// of audio, which every player will honour — the samples are all there and
    /// none of them play.
    @discardableResult
    public func finish() throws -> UInt64 {
        guard isOpen else { return UInt64(audioBytes) }
        isOpen = false

        // 36 is everything in the header after the size field itself.
        try patch(UInt32(36).addingReportingOverflow(audioBytes).partialValue, at: Self.riffSizeOffset)
        try patch(audioBytes, at: Self.dataSizeOffset)

        // A chunk with an odd length is padded to an even one. Uncompressed
        // audio is almost always even, and "almost" is not a property to build
        // a file format on.
        if audioBytes % 2 == 1 {
            try handle.seekToEnd()
            try write(Data([0]))
        }

        try handle.close()
        return UInt64(audioBytes)
    }

    deinit {
        // A writer dropped without finishing leaves a file that looks valid and
        // plays silence. Closing the handle at least stops the descriptor
        // leaking; the sizes cannot be patched from here because it can throw.
        if isOpen { try? handle.close() }
    }

    private func write(_ data: Data) throws {
        do {
            try handle.write(contentsOf: data)
        } catch {
            throw PlatformError.cannotRead(url.lastPathComponent)
        }
    }

    private func patch(_ value: UInt32, at offset: UInt64) throws {
        do {
            try handle.seek(toOffset: offset)
            try handle.write(contentsOf: Data(Self.littleEndian(value)))
        } catch {
            throw PlatformError.cannotRead(url.lastPathComponent)
        }
    }

    // MARK: - The format

    static func littleEndian(_ value: UInt32) -> [UInt8] {
        [
            UInt8(value & 0xFF),
            UInt8((value >> 8) & 0xFF),
            UInt8((value >> 16) & 0xFF),
            UInt8((value >> 24) & 0xFF),
        ]
    }

    static func littleEndian(_ value: UInt16) -> [UInt8] {
        [UInt8(value & 0xFF), UInt8((value >> 8) & 0xFF)]
    }

    /// The forty-four byte header, with both sizes left at zero.
    ///
    /// `internal` rather than private so a test can read it back without a file,
    /// which is the difference between testing the format and testing the
    /// filesystem.
    static func header(
        channels: UInt32,
        sampleRate: UInt32,
        bitsPerSample: UInt32,
        isFloat: Bool
    ) -> Data {
        var bytes: [UInt8] = []
        bytes += Array("RIFF".utf8)
        bytes += littleEndian(UInt32(0))  // patched by `finish`
        bytes += Array("WAVE".utf8)

        bytes += Array("fmt ".utf8)
        bytes += littleEndian(UInt32(16))
        // 1 is integer PCM, 3 is IEEE float. A player reads this to decide how
        // to interpret every sample in the file.
        bytes += littleEndian(UInt16(isFloat ? 3 : 1))
        bytes += littleEndian(UInt16(truncatingIfNeeded: channels))

        bytes += littleEndian(sampleRate)
        let blockAlign = channels * (bitsPerSample / 8)
        bytes += littleEndian(sampleRate * blockAlign)  // bytes per second
        bytes += littleEndian(UInt16(truncatingIfNeeded: blockAlign))
        bytes += littleEndian(UInt16(truncatingIfNeeded: bitsPerSample))

        bytes += Array("data".utf8)
        bytes += littleEndian(UInt32(0))  // patched by `finish`
        return Data(bytes)
    }
}
