import CPRVBridge
import Foundation

/// Where a master is going.
public enum DeliveryTarget: Sendable, CaseIterable {
    /// A streaming service, which will normalise it whatever you do.
    case streaming
    /// A club system, where the ceiling matters more than the loudness.
    case club
    /// Broadcast, where a standard says what is acceptable.
    case broadcast

    var code: Int32 {
        switch self {
        case .streaming: PRV_TARGET_STREAMING.rawValue
        case .club: PRV_TARGET_CLUB.rawValue
        case .broadcast: PRV_TARGET_BROADCAST.rawValue
        }
    }
}

/// What container it goes in.
public enum AudioFormat: Sendable, CaseIterable {
    case wave, aiff, flac
    /// Lossy at 320 kbit/s.
    case lossy320

    var code: Int32 {
        switch self {
        case .wave: PRV_FORMAT_WAVE.rawValue
        case .aiff: PRV_FORMAT_AIFF.rawValue
        case .flac: PRV_FORMAT_FLAC.rawValue
        case .lossy320: PRV_FORMAT_LOSSY_320.rawValue
        }
    }
}

/// At what resolution.
///
/// Used both to ask whether a master is fit to deliver and to write the file
/// afterwards. One enum for both, because they are two halves of the same
/// question and a second one would be a second place for the halves to disagree.
public enum BitDepth: Sendable, Equatable, CaseIterable {
    case sixteen, twentyFour, float32

    var code: Int32 {
        switch self {
        case .sixteen: PRV_DEPTH_SIXTEEN.rawValue
        case .twentyFour: PRV_DEPTH_TWENTY_FOUR.rawValue
        case .float32: PRV_DEPTH_FLOAT32.rawValue
        }
    }

    /// A stable identifier, for localisation.
    public var key: String {
        switch self {
        case .sixteen: "depth.16"
        case .twentyFour: "depth.24"
        case .float32: "depth.float32"
        }
    }

    /// Whether writing at this depth discards information from the mix.
    ///
    /// What decides whether dither is needed. A property of the depth rather
    /// than a setting: the engine works in floating point, so anything narrower
    /// is a reduction.
    public var reducesResolution: Bool { self != .float32 }
}

/// Whether a master can go as it is.
public enum Compliance: Sendable, Equatable {
    /// Nothing needs doing.
    case ready
    /// It needs a gain, and applying it is safe.
    case needsGain
    /// The gain it needs would push it past the ceiling.
    ///
    /// The case a host must not quietly apply anyway. Something has to give —
    /// the limiter, the target, or the expectation — and that is a person's
    /// decision.
    case wouldClip
    case unrecognised(code: Int32)

    init(code: Int32) {
        self =
            switch code {
            case PRV_COMPLIANCE_READY.rawValue: .ready
            case PRV_COMPLIANCE_NEEDS_GAIN.rawValue: .needsGain
            case PRV_COMPLIANCE_WOULD_CLIP.rawValue: .wouldClip
            default: .unrecognised(code: code)
            }
    }
}

/// What a master measures, and what a target would need of it.
///
/// # The core writes no files
///
/// It answers what has to happen before this can go where it is going. The host
/// applies the gain, encodes and writes — which is what lets one rule serve a
/// WAV on a laptop, a stream upload and a broadcast delivery.
public struct DeliveryVerdict: Sendable, Equatable {
    public let compliance: Compliance
    /// Measured integrated loudness, LUFS.
    public let measuredLUFS: Double
    /// Measured true peak, dBTP.
    public let measuredTruePeak: Double
    /// The gain that would put it on target, in decibels.
    ///
    /// Zero on a master far below target. That is usually a mistake upstream —
    /// a muted lane, the wrong project — and turning it up produces a loud
    /// version of the wrong thing.
    public let gainDB: Double
    /// Where the true peak lands once the gain is applied, dBTP.
    ///
    /// The number that decides whether the gain is safe. Shown beside the gain,
    /// never instead of it.
    public let resultingTruePeak: Double
    /// Room left under the ceiling afterwards, in decibels.
    public let headroomDB: Double
    /// Whether the encoder should dither. Follows the depth, not the format.
    public let needsDither: Bool

    /// Whether a person should look before exporting.
    public var needsAttention: Bool { compliance != .ready }
}

extension Analysis {
    /// Judges this master against a delivery target.
    ///
    /// - Throws: ``EngineError/refused`` when the loudness could not be
    ///   measured, which is the one thing the whole judgement rests on.
    public func judge(
        for target: DeliveryTarget,
        format: AudioFormat = .wave,
        depth: BitDepth = .twentyFour
    ) throws -> DeliveryVerdict {
        var handle: OpaquePointer?
        try EngineError.check(
            prv_delivery_judge(rawHandle, target.code, format.code, depth.code, &handle)
        )
        guard let handle else { throw EngineError.invalidHandle }
        defer { prv_delivery_destroy(handle) }

        var compliance: Int32 = 0
        var measuredLUFS = 0.0
        var measuredTruePeak = 0.0
        var gain = 0.0
        var resulting = 0.0
        var headroom = 0.0
        var dither: Int32 = 0

        try EngineError.check(
            prv_delivery_verdict(
                handle, &compliance, &measuredLUFS, &measuredTruePeak,
                &gain, &resulting, &headroom, &dither
            )
        )

        return DeliveryVerdict(
            compliance: Compliance(code: compliance),
            measuredLUFS: measuredLUFS,
            measuredTruePeak: measuredTruePeak,
            gainDB: gain,
            resultingTruePeak: resulting,
            headroomDB: headroom,
            needsDither: dither != 0
        )
    }
}
