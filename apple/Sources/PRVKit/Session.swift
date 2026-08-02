import Foundation
import PRVCore

/// A decoder that keeps whole tracks in memory and serves blocks from them.
///
/// # Why the playback source is memory-backed
///
/// The audio thread may not open a file, allocate or block. Streaming from disk
/// means a reader thread, a ring buffer and a starvation policy — real work, and
/// work that belongs to a later phase. Holding decoded audio is the honest
/// interim: it costs memory proportional to the set, it never underruns, and the
/// glitch it cannot have is the one an audience would notice.
///
/// When streaming arrives it replaces this class and nothing above it changes,
/// because everything above it talks to ``AudioSource``.
public final class LoadedAudio: AudioSource, @unchecked Sendable {
    /// Decoded mono samples, by track.
    private var tracks: [UInt64: [Float]] = [:]

    public init() {}

    /// Holds a track's decoded audio.
    public func hold(track: UInt64, samples: [Float]) {
        tracks[track] = samples
    }

    /// Forgets a track's audio.
    public func release(track: UInt64) {
        tracks[track] = nil
    }

    /// How many tracks are held.
    public var count: Int { tracks.count }

    /// The samples held for a track, if any.
    public func samples(for track: UInt64) -> [Float]? { tracks[track] }

    public func read(
        track: UInt64,
        sourceOffset: Int64,
        into buffer: UnsafeMutableBufferPointer<Float>,
        channels: Int,
        capacity: Int,
        destination: Int,
        frames: Int
    ) -> Int {
        guard let samples = tracks[track], sourceOffset >= 0 else { return 0 }
        let start = Int(sourceOffset)
        guard start < samples.count else { return 0 }

        let available = min(frames, samples.count - start)
        guard available > 0 else { return 0 }

        // Mono source fanned out to every channel. A stereo decoder would give
        // each channel its own slice; the shape of this loop is the same.
        for channel in 0..<channels {
            let base = channel * capacity + destination
            for frame in 0..<available {
                let index = base + frame
                guard index < buffer.count else { break }
                buffer[index] = samples[start + frame]
            }
        }
        return available
    }
}

/// What the interface needs to know, all at once.
///
/// A value rather than a set of properties on the session, so an interface reads
/// a consistent picture: a snapshot cannot show a playhead from one moment and a
/// state from another, which is exactly the flicker a transport display gets
/// when it polls four things separately.
public struct SessionSnapshot: Sendable, Equatable {
    public var playback: PlaybackState
    /// Frames.
    public var position: Int64
    /// Frames.
    public var duration: Int64
    public var placementCount: UInt64
    /// Whether every placement the last render touched was read in full.
    public var audioComplete: Bool

    /// Built by the session, and by anything that needs to describe a state the
    /// session has not reached — a preview, or a test of what an interface does
    /// with `buffering`, which is a state no fixture can be driven into.
    public init(
        playback: PlaybackState,
        position: Int64,
        duration: Int64,
        placementCount: UInt64,
        audioComplete: Bool
    ) {
        self.playback = playback
        self.position = position
        self.duration = duration
        self.placementCount = placementCount
        self.audioComplete = audioComplete
    }

    /// How far through the set the playhead is, from zero to one.
    public var progress: Double {
        guard duration > 0 else { return 0 }
        return min(1, max(0, Double(position) / Double(duration)))
    }
}

/// Everything the application does, in one object.
///
/// # What this adds over `PRVCore`
///
/// `PRVCore` makes the boundary safe to call. This makes it a *workflow*: import
/// a file, analyse it, add it to the library, plan a set, apply it, play it. The
/// core decides; this sequences.
///
/// It deliberately holds no rules of its own. Every refusal here comes back from
/// the core — which is why there is no validation in this file, and why that
/// absence is the thing to preserve when it grows.
public final class Session {
    private let engine: Engine
    private let planner: Planner
    private let decoder: MediaDecoder
    private let audio = LoadedAudio()
    private let output: AudioOutput?

    /// The sample rate everything runs at.
    public let sampleRate: UInt32

    /// Tracks the session knows about, in the order they were imported.
    public private(set) var library: [MediaItem] = []

    /// What the analysis found, by track.
    public private(set) var analyses: [UInt64: TrackFacts] = [:]

    /// The readings an interface shows beside a track.
    public struct TrackFacts: Sendable, Equatable {
        public var bpm: Double?
        public var keySemitones: Int32?
        public var keyIsMinor: Bool?
        public var loudnessLUFS: Double?
        public var energy: Float?
        /// Whether enough was found to plan with.
        public var isPlannable: Bool

        public init(
            bpm: Double? = nil,
            keySemitones: Int32? = nil,
            keyIsMinor: Bool? = nil,
            loudnessLUFS: Double? = nil,
            energy: Float? = nil,
            isPlannable: Bool
        ) {
            self.bpm = bpm
            self.keySemitones = keySemitones
            self.keyIsMinor = keyIsMinor
            self.loudnessLUFS = loudnessLUFS
            self.energy = energy
            self.isPlannable = isPlannable
        }
    }

    /// Builds a session.
    ///
    /// `output` is optional so the whole workflow can be exercised without a
    /// device — which is what makes this class testable on a machine with no
    /// audio hardware, and what a preview uses.
    public init(
        sampleRate: UInt32 = 48_000,
        channels: Int = 2,
        maxBlockFrames: Int = 512,
        decoder: MediaDecoder,
        output: AudioOutput? = nil
    ) throws {
        self.sampleRate = sampleRate
        self.decoder = decoder
        self.output = output
        engine = try Engine(
            sampleRate: sampleRate,
            channels: channels,
            maxBlockFrames: maxBlockFrames
        )
        planner = try Planner()
        try engine.setSource(audio)
    }

    /// Imports a track: decode it, analyse it, and offer it to the planner.
    ///
    /// - Returns: what the analysis found. A track that could not be analysed
    ///   well enough to plan with is still imported and still playable — it
    ///   simply cannot be chosen by the planner, and ``TrackFacts/isPlannable``
    ///   says so rather than the track disappearing.
    @discardableResult
    public func `import`(_ item: MediaItem) throws -> TrackFacts {
        let samples = try decoder.decodeMono(item, sampleRate: sampleRate)
        audio.hold(track: item.id, samples: samples)
        library.append(item)

        guard let analysis = try? Analysis(samples: samples, sampleRate: sampleRate) else {
            let facts = TrackFacts(isPlannable: false)
            analyses[item.id] = facts
            return facts
        }

        var plannable = false
        if let candidate = analysis.candidate(track: item.id) {
            try planner.add(candidate)
            for point in analysis.mixPoints(track: item.id) {
                try planner.add(point)
            }
            plannable = true
        }

        let facts = TrackFacts(
            bpm: analysis.tempo?.bpm,
            keySemitones: analysis.key?.semitones,
            keyIsMinor: analysis.key?.isMinor,
            loudnessLUFS: analysis.loudness?.integrated,
            energy: analysis.energy,
            isPlannable: plannable
        )
        analyses[item.id] = facts
        return facts
    }

    /// Plans a set of a given length.
    ///
    /// - Returns: the alternatives, each as its tracklist. Empty is not a
    ///   possible return — the core refuses rather than producing nothing, and
    ///   the throw carries why.
    public func planSet(
        minutes: Int,
        shape: EnergyShape,
        creativity: Creativity = .balanced
    ) throws -> [[PlannedTrack]] {
        let frames = Int64(minutes) * 60 * Int64(sampleRate)
        let count = try planner.plan(
            targetFrames: frames,
            sampleRate: sampleRate,
            shape: shape,
            creativity: creativity
        )
        return try (0..<count).map { index in
            try planner.select(index)
            return try planner.tracks()
        }
    }

    /// Puts one of the planned alternatives on the timeline.
    public func adopt(alternative index: UInt64) throws {
        try planner.select(index)
        try planner.apply(to: engine)
    }

    /// How far the current plan's set falls from what was asked for, as text an
    /// interface can show beside the length.
    public func plannedDuration() throws -> Int64 {
        try planner.duration()
    }

    /// Starts playback, opening the device if there is one.
    ///
    /// The transport is driven through its own states rather than jumped: the
    /// core refuses `play` on a deck that was never loaded, and that refusal is
    /// worth honouring rather than routing around.
    public func play() throws {
        if try engine.playbackState() == .stopped {
            try engine.apply(.load)
            try engine.apply(.loadSucceeded)
        }
        try engine.apply(.play)

        guard let output, !output.isRunning else { return }
        // The audio thread gets a render handle and nothing else. It cannot
        // reach `placeTrack` or the planner, because those are not on the type
        // it holds — the rule is enforced by the compiler rather than by this
        // comment.
        let render = engine.renderHandle
        try output.start(channels: render.channels, sampleRate: sampleRate) { buffer, frames in
            // Nothing here allocates. A failed render is silence for one block,
            // which is what the device does anyway if nobody fills its buffer.
            try? render.render(into: buffer, frames: frames)
        }
    }

    /// Pauses playback and stops pulling blocks.
    public func pause() throws {
        try engine.apply(.pause)
        output?.stop()
    }

    /// Renders one block by hand, for a host with no device.
    ///
    /// What the tests use, and what an offline export would use.
    public func renderBlock(into buffer: UnsafeMutableBufferPointer<Float>, frames: Int) throws {
        try engine.render(into: buffer, channels: engine.channels, frames: frames)
    }

    /// Everything an interface needs, consistent as of now.
    public func snapshot() throws -> SessionSnapshot {
        SessionSnapshot(
            playback: try engine.playbackState(),
            position: try engine.position(),
            duration: try engine.duration(),
            placementCount: try engine.placementCount(),
            audioComplete: try engine.renderWasComplete()
        )
    }

    /// Moves the playhead.
    public func seek(to position: Int64) throws {
        try engine.seek(to: position)
    }
}
