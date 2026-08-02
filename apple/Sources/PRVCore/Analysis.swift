import CPRVBridge
import Foundation

/// What the analysis found out about one track.
///
/// # Absent is not zero
///
/// Every reading here is optional, and that is the point. A track with no
/// discernible pulse has no tempo — not a tempo of zero, and not a guessed 120.
/// The distinction survives all the way out to Swift because the alternative is
/// a number the planner trusts, and a whole set built on it.
///
/// # This is not the audio thread
///
/// ``analyse(samples:sampleRate:)`` allocates and takes seconds on a long track.
/// It belongs to the background domain. Calling it from a render callback would
/// drop out.
public final class Analysis {
    private let handle: OpaquePointer

    /// The handle, for the delivery judgement in this module.
    ///
    /// `internal`: everything above `PRVCore` should be unable to reach a raw
    /// pointer at all, which is the whole point of the wrapper.
    var rawHandle: OpaquePointer { handle }

    /// Analyses a track from mono samples.
    ///
    /// The audio is borrowed for the duration of the call and never retained, so
    /// the caller may free it immediately afterwards.
    ///
    /// - Throws: ``EngineError/refused`` when the pipeline could not run at all,
    ///   which happens for audio too short to hold a window.
    public init(samples: [Float], sampleRate: UInt32) throws {
        var created: OpaquePointer?
        try samples.withUnsafeBufferPointer { buffer in
            try EngineError.check(
                prv_analysis_run(
                    buffer.baseAddress,
                    UInt64(buffer.count),
                    sampleRate,
                    &created
                )
            )
        }
        guard let created else { throw EngineError.invalidHandle }
        handle = created
    }

    deinit {
        prv_analysis_destroy(handle)
    }

    /// A tempo, and how sure the estimator is of it.
    public struct Tempo: Sendable, Equatable {
        public let bpm: Double
        /// Zero to one.
        public let confidence: Float
    }

    /// A key, and how sure the detector is of it.
    public struct Key: Sendable, Equatable {
        /// Semitones above C.
        public let semitones: Int32
        public let isMinor: Bool
        /// Zero to one.
        public let confidence: Float
    }

    /// How loud the track is.
    public struct Loudness: Sendable, Equatable {
        /// Gated integrated loudness, in LUFS.
        public let integrated: Double
        /// The loudness range, in loudness units.
        ///
        /// A club master runs around 3 to 5; a well-recorded live album runs 10
        /// or more. It says whether a track survives being played next to
        /// another without its quiet parts disappearing.
        public let range: Double
    }

    /// A place the analysis says a transition could happen.
    public struct TransitionPoint: Sendable, Equatable {
        /// Frames into the track.
        public let position: Int64
        /// How quiet it is there, zero to one. Quieter is better to mix on.
        public let energy: Float
    }

    /// The tempo, or `nil` when no pulse was found.
    ///
    /// `nil` rather than a thrown error, because "this track has no steady
    /// tempo" is an ordinary fact about ambient and spoken-word material, not a
    /// malfunction worth a `do`/`catch` at every call site.
    public var tempo: Tempo? {
        var bpm = 0.0
        var confidence: Float = 0
        guard prv_analysis_tempo(handle, &bpm, &confidence) == PRV_OK.rawValue else {
            return nil
        }
        return Tempo(bpm: bpm, confidence: confidence)
    }

    /// The key, or `nil` when none was found.
    public var key: Key? {
        var semitones: Int32 = 0
        var isMinor: Int32 = 0
        var confidence: Float = 0
        guard
            prv_analysis_key(handle, &semitones, &isMinor, &confidence) == PRV_OK.rawValue
        else { return nil }
        return Key(semitones: semitones, isMinor: isMinor != 0, confidence: confidence)
    }

    /// The loudness, or `nil` when it could not be measured.
    public var loudness: Loudness? {
        var integrated = 0.0
        var range = 0.0
        guard prv_analysis_loudness(handle, &integrated, &range) == PRV_OK.rawValue else {
            return nil
        }
        return Loudness(integrated: integrated, range: range)
    }

    /// The true peak in dBFS, or `nil` when it could not be measured.
    public var truePeakDBFS: Double? {
        var value = 0.0
        guard prv_analysis_true_peak(handle, &value) == PRV_OK.rawValue else { return nil }
        return value
    }

    /// Overall energy from zero to one, or `nil` when no structure was found.
    public var energy: Float? {
        var value: Float = 0
        guard prv_analysis_energy(handle, &value) == PRV_OK.rawValue else { return nil }
        return value
    }

    /// How long the analysed audio was, in frames.
    public var duration: Int64 {
        var value: Int64 = 0
        guard prv_analysis_duration(handle, &value) == PRV_OK.rawValue else { return 0 }
        return value
    }

    /// Every place a transition could happen, in order.
    ///
    /// Empty when no structure was found, which is a weaker claim than an error:
    /// the track is still usable, it simply offers nowhere obvious to mix.
    public var transitionPoints: [TransitionPoint] {
        var count: UInt64 = 0
        guard
            prv_analysis_transition_point_count(handle, &count) == PRV_OK.rawValue
        else { return [] }

        var points: [TransitionPoint] = []
        points.reserveCapacity(Int(count))
        for index in 0..<count {
            var position: Int64 = 0
            var energy: Float = 0
            guard
                prv_analysis_transition_point(handle, index, &position, &energy)
                    == PRV_OK.rawValue
            else { continue }
            points.append(TransitionPoint(position: position, energy: energy))
        }
        return points
    }

    /// This analysis as a planner candidate, if enough was found to plan with.
    ///
    /// # Why this returns an optional rather than filling in defaults
    ///
    /// A candidate with no tempo cannot be beat-matched and a candidate with no
    /// energy cannot be placed on a curve. Substituting 120 BPM and 0.5 would
    /// produce a set that looks planned and was not, which is worse than a track
    /// the user is told could not be analysed.
    public func candidate(track: UInt64) -> Candidate? {
        guard let tempo, let energy else { return nil }
        return Candidate(
            track: track,
            duration: duration,
            bpm: tempo.bpm,
            energy: energy,
            key: key.map {
                Candidate.Key(
                    semitones: $0.semitones,
                    isMinor: $0.isMinor,
                    confidence: $0.confidence
                )
            },
            loudnessLUFS: loudness.map { Float($0.integrated) } ?? -14,
            hasVocals: nil
        )
    }

    /// The exit points this analysis found, for a track already added.
    public func mixPoints(track: UInt64) -> [MixPoint] {
        transitionPoints.map {
            MixPoint(track: track, position: $0.position, energy: $0.energy, isExit: true)
        }
    }
}
