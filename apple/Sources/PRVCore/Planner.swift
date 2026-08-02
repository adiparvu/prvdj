import CPRVBridge
import Foundation

/// The shape of a set's energy over its length.
///
/// A small closed set rather than a free-form curve, deliberately: these are the
/// shapes a DJ plans toward and a listener recognises. A free-form curve would be
/// more expressive and would mostly express mistakes.
public enum EnergyShape: Sendable, CaseIterable {
    /// Rising steadily. The warm-up set.
    case rising
    /// Rising to a peak around two thirds through, then easing. A headline set.
    case arc
    /// Held at a steady high level. Peak time.
    case plateau
    /// Two peaks with a deliberate dip between them.
    case wave
    /// Falling steadily. The close-down set, and the sunset.
    case falling

    var code: Int32 {
        switch self {
        case .rising: PRV_ENERGY_RISING.rawValue
        case .arc: PRV_ENERGY_ARC.rawValue
        case .plateau: PRV_ENERGY_PLATEAU.rawValue
        case .wave: PRV_ENERGY_WAVE.rawValue
        case .falling: PRV_ENERGY_FALLING.rawValue
        }
    }
}

/// How far the planner may depart from established practice.
///
/// This never relaxes a hard constraint. A clashing key is not generated at any
/// setting; creativity widens the *soft* limits only, and that distinction is
/// enforced in the core rather than promised here.
public enum Creativity: Sendable, CaseIterable {
    case conservative
    case balanced
    case adventurous

    var code: Int32 {
        switch self {
        case .conservative: PRV_CREATIVITY_CONSERVATIVE.rawValue
        case .balanced: PRV_CREATIVITY_BALANCED.rawValue
        case .adventurous: PRV_CREATIVITY_ADVENTUROUS.rawValue
        }
    }
}

/// What the analysis knows about a track, in the form the planner wants.
public struct Candidate: Sendable, Equatable {
    /// The library's identity for this track.
    public var track: UInt64
    /// How long it is, in frames.
    public var duration: Int64
    /// Its tempo, in beats per minute.
    public var bpm: Double
    /// How energetic it is, from zero to one.
    public var energy: Float
    /// Its key, if one is known.
    public var key: Key?
    /// Its integrated loudness.
    public var loudnessLUFS: Float
    /// Whether it has vocals, if anybody has looked.
    ///
    /// `nil` is a different answer from `false` and scores differently: a track
    /// nobody has analysed is not a track known to be instrumental.
    public var hasVocals: Bool?

    public init(
        track: UInt64,
        duration: Int64,
        bpm: Double,
        energy: Float,
        key: Key? = nil,
        loudnessLUFS: Float = -14,
        hasVocals: Bool? = nil
    ) {
        self.track = track
        self.duration = duration
        self.bpm = bpm
        self.energy = energy
        self.key = key
        self.loudnessLUFS = loudnessLUFS
        self.hasVocals = hasVocals
    }

    /// A musical key and how sure the analysis is of it.
    public struct Key: Sendable, Equatable {
        /// Semitones above C. Wrapped, so a host counting from elsewhere is
        /// using a convention rather than making a mistake.
        public var semitones: Int32
        public var isMinor: Bool
        /// Zero to one. At or below zero means the key is unknown.
        public var confidence: Float

        public init(semitones: Int32, isMinor: Bool, confidence: Float = 1) {
            self.semitones = semitones
            self.isMinor = isMinor
            self.confidence = confidence
        }
    }
}

/// A place a track can be left or entered.
public struct MixPoint: Sendable, Equatable {
    public var track: UInt64
    /// Frames into the track.
    public var position: Int64
    /// How quiet it is there, from zero to one. Quieter is better to mix on.
    public var energy: Float
    /// An exit if true, an entry if false.
    public var isExit: Bool

    public init(track: UInt64, position: Int64, energy: Float, isExit: Bool) {
        self.track = track
        self.position = position
        self.energy = energy
        self.isExit = isExit
    }
}

/// One track in a planned set.
public struct PlannedTrack: Sendable, Equatable {
    public let track: UInt64
    /// Where it begins in the set, in frames.
    public let start: Int64
    /// How long it plays for, in frames.
    public let duration: Int64
    /// The score of the move *into* this track, from zero to one.
    ///
    /// `1.0` for the opening track, which was chosen rather than transitioned
    /// into.
    public let transitionScore: Float

    /// Built by the planner, and by anything that needs to describe a plan the
    /// planner has not made — a preview, or a test of what an interface does
    /// with a set that misses its target length.
    public init(track: UInt64, start: Int64, duration: Int64, transitionScore: Float = 1) {
        self.track = track
        self.start = start
        self.duration = duration
        self.transitionScore = transitionScore
    }
}

/// Plans sets from a library.
///
/// # Why this is separate from ``Engine``
///
/// A library and a plan are not a project. A host may plan with nothing open,
/// and may keep a project open while replanning. Tying the two together would
/// make the first impossible and the second awkward, and they share no
/// invariant that would justify it.
public final class Planner {
    private let handle: OpaquePointer

    public init() throws {
        var created: OpaquePointer?
        try EngineError.check(prv_planner_create(&created))
        guard let created else { throw EngineError.invalidHandle }
        handle = created
    }

    deinit {
        prv_planner_destroy(handle)
    }

    /// Adds one track to the library the planner chooses from.
    public func add(_ candidate: Candidate) throws {
        try EngineError.check(
            prv_planner_add_candidate(
                handle,
                candidate.track,
                candidate.duration,
                candidate.bpm,
                candidate.energy,
                candidate.key?.semitones ?? 0,
                (candidate.key?.isMinor ?? false) ? 1 : 0,
                // Zero confidence is how the boundary spells "no key known",
                // which is why an absent key maps to it rather than to a
                // separate call the caller could forget.
                candidate.key?.confidence ?? 0,
                candidate.loudnessLUFS,
                candidate.hasVocals.map { $0 ? Int32(1) : Int32(0) } ?? -1
            )
        )
    }

    /// Adds a place the analysis says a track can be left or entered.
    public func add(_ point: MixPoint) throws {
        try EngineError.check(
            prv_planner_add_mix_point(
                handle,
                point.track,
                point.position,
                point.energy,
                point.isExit ? 1 : 0
            )
        )
    }

    /// How many candidates the library holds.
    public func candidateCount() throws -> UInt64 {
        var value: UInt64 = 0
        try EngineError.check(prv_planner_candidate_count(handle, &value))
        return value
    }

    /// Forgets the library and any plan made from it.
    public func clear() throws {
        try EngineError.check(prv_planner_clear(handle))
    }

    /// Plans up to three genuinely different sets.
    ///
    /// - Parameter tempoRange: `nil` leaves it open. Half a range is not
    ///   expressible, which is deliberate — honouring one end alone would
    ///   constrain the set in a way nobody asked for.
    /// - Returns: how many alternatives were produced.
    /// - Throws: ``EngineError/refused`` when no set could be built. That is a
    ///   real answer about the library — nothing in it fits — and an interface
    ///   should say so rather than show an empty list.
    @discardableResult
    public func plan(
        targetFrames: Int64,
        sampleRate: UInt32,
        shape: EnergyShape,
        creativity: Creativity = .balanced,
        tempoRange: ClosedRange<Float>? = nil
    ) throws -> UInt64 {
        var count: UInt64 = 0
        try EngineError.check(
            prv_planner_plan(
                handle,
                targetFrames,
                sampleRate,
                shape.code,
                creativity.code,
                tempoRange?.lowerBound ?? 0,
                tempoRange?.upperBound ?? 0,
                &count
            )
        )
        return count
    }

    /// Chooses which alternative subsequent reads describe.
    public func select(_ index: UInt64) throws {
        try EngineError.check(prv_planner_select(handle, index))
    }

    /// How many tracks the selected plan holds.
    public func trackCount() throws -> UInt64 {
        var value: UInt64 = 0
        try EngineError.check(prv_planner_track_count(handle, &value))
        return value
    }

    /// How long the selected plan runs for, in frames.
    public func duration() throws -> Int64 {
        var value: Int64 = 0
        try EngineError.check(prv_planner_duration(handle, &value))
        return value
    }

    /// The selected plan's mean transition score, from zero to one.
    public func score() throws -> Float {
        var value: Float = 0
        try EngineError.check(prv_planner_score(handle, &value))
        return value
    }

    /// One track of the selected plan.
    public func track(at index: UInt64) throws -> PlannedTrack {
        var track: UInt64 = 0
        var start: Int64 = 0
        var duration: Int64 = 0
        var score: Float = 0
        try EngineError.check(
            prv_planner_track(handle, index, &track, &start, &duration, &score)
        )
        return PlannedTrack(
            track: track,
            start: start,
            duration: duration,
            transitionScore: score
        )
    }

    /// The whole selected plan, in order.
    ///
    /// Convenience over the indexed reads, for the common case of showing a
    /// tracklist. Reads a count once and then that many tracks, which is safe
    /// because making a new plan is a different call and cannot happen partway.
    public func tracks() throws -> [PlannedTrack] {
        let count = try trackCount()
        return try (0..<count).map { try track(at: $0) }
    }

    /// Applies the selected plan to an engine's project.
    ///
    /// The plan becomes ordinary operations on the log. After this there is
    /// nothing in the document that says which placements a person made and
    /// which the planner did — which is what makes a generated mix editable
    /// rather than merely promised to be.
    public func apply(to engine: Engine, timestamp: Date = Date()) throws {
        let micros = Int64(timestamp.timeIntervalSince1970 * 1_000_000)
        try EngineError.check(prv_planner_apply(handle, engine.rawHandle, micros))
    }
}
