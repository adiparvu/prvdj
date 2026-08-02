import Foundation
import Testing

@testable import PRVCore

/// A decoder that hands back a constant, so a test can tell audio from silence
/// by looking at one sample.
final class ConstantSource: AudioSource {
    let value: Float
    let available: Int
    private(set) var calls = 0

    init(value: Float, available: Int) {
        self.value = value
        self.available = available
    }

    func read(
        track: UInt64,
        sourceOffset: Int64,
        into buffer: UnsafeMutableBufferPointer<Float>,
        channels: Int,
        capacity: Int,
        destination: Int,
        frames: Int
    ) -> Int {
        calls += 1
        let wanted = min(frames, available)
        for channel in 0..<channels {
            for frame in 0..<wanted {
                let index = channel * capacity + destination + frame
                if index < buffer.count {
                    buffer[index] = value
                }
            }
        }
        return wanted
    }
}

/// A decoder that claims more than it wrote. The core must not believe it.
final class OverReportingSource: AudioSource {
    func read(
        track: UInt64,
        sourceOffset: Int64,
        into buffer: UnsafeMutableBufferPointer<Float>,
        channels: Int,
        capacity: Int,
        destination: Int,
        frames: Int
    ) -> Int {
        frames * 4
    }
}

@Suite("The boundary, as Swift sees it")
struct EngineTests {

    @Test("the loaded library is the one this wrapper was built for")
    func versionsAgree() {
        // The first thing a host does, and the check that is still meaningful
        // when everything else has moved.
        #expect(Engine.abiVersion != 0)
        #expect(Engine.abiVersion >> 16 == Engine.expectedMajor)
        #expect(Engine.isCompatible)
    }

    @Test("a host can start an engine, place a track and hear it")
    func endToEnd() throws {
        let engine = try Engine(sampleRate: 48_000, channels: 2, maxBlockFrames: 512)
        let source = ConstantSource(value: 0.5, available: 8_192)
        try engine.setSource(source)

        let placement = try engine.placeTrack(track: 7, position: 0, length: 4_096)
        #expect(placement != 0, "a placement identity of zero is not a name")
        #expect(try engine.duration() == 4_096)
        #expect(try engine.placementCount() == 1)

        try engine.apply(.load)
        try engine.apply(.loadSucceeded)
        try engine.apply(.play)
        #expect(try engine.playbackState() == .playing)

        var block = [Float](repeating: 0, count: 2 * 256)
        try block.withUnsafeMutableBufferPointer { buffer in
            try engine.render(into: buffer, channels: 2, frames: 256)
        }

        #expect(source.calls > 0, "the renderer never asked for audio")
        #expect(block.contains { $0 != 0 }, "the block came back silent")
        #expect(try engine.renderWasComplete())
        #expect(try engine.position() == 256, "the playhead did not advance by one block")
    }

    @Test("playing a deck with nothing loaded is refused and says why")
    func refusesImpossibleTransition() throws {
        let engine = try Engine(sampleRate: 48_000, channels: 2, maxBlockFrames: 512)

        // The core returns these rather than ignoring them, and so must this
        // layer. Swallowing it would give the user a play button that does
        // nothing and explains nothing.
        #expect(throws: EngineError.invalidState) {
            try engine.apply(.play)
        }
    }

    @Test("an error carries the core's own explanation rather than a translation")
    func errorsExplainThemselves() {
        // The text comes from the layer that made the decision. A message
        // written here would be a second copy, and the second copy is the one
        // that goes stale.
        let error = EngineError.invalidState
        #expect(!error.explanation.isEmpty)
        #expect(error.explanation != "the core gave no explanation")
        #expect(EngineError.from(code: 0) == nil, "zero is success")
    }

    @Test("every status code maps to exactly one error and back")
    func statusRoundTrip() {
        let errors: [EngineError] = [
            .nullPointer, .invalidArgument, .invalidHandle,
            .invalidState, .bufferTooSmall, .refused, .panicked,
        ]
        for error in errors {
            #expect(EngineError.from(code: error.code) == error)
        }
        // And a number no version defines is carried rather than guessed at.
        #expect(EngineError.from(code: 9_999) == .unrecognised(code: 9_999))
    }

    @Test("a shape the core cannot serve is refused at construction")
    func refusesImpossibleShapes() {
        #expect(throws: EngineError.invalidArgument) {
            _ = try Engine(sampleRate: 0, channels: 2, maxBlockFrames: 512)
        }
        #expect(throws: EngineError.invalidArgument) {
            _ = try Engine(sampleRate: 48_000, channels: 0, maxBlockFrames: 512)
        }
        #expect(throws: EngineError.invalidArgument) {
            _ = try Engine(sampleRate: 48_000, channels: 2, maxBlockFrames: 0)
        }
    }

    @Test("a block larger than the engine was built for is refused, not truncated")
    func refusesOversizedBlock() throws {
        // Truncating would hand back a half-filled buffer, and the frames never
        // written would play as whatever was there before.
        let engine = try Engine(sampleRate: 48_000, channels: 2, maxBlockFrames: 256)
        var block = [Float](repeating: 0, count: 2 * 1_024)

        #expect(throws: EngineError.invalidArgument) {
            try block.withUnsafeMutableBufferPointer { buffer in
                try engine.render(into: buffer, channels: 2, frames: 1_024)
            }
        }
    }

    @Test("a buffer too small for the block is caught before the boundary")
    func refusesUndersizedBuffer() throws {
        // Caught in Swift rather than in Rust, because by the time the pointer
        // has crossed there is no length left to check it against.
        let engine = try Engine(sampleRate: 48_000, channels: 2, maxBlockFrames: 512)
        var block = [Float](repeating: 0, count: 8)

        #expect(throws: EngineError.bufferTooSmall) {
            try block.withUnsafeMutableBufferPointer { buffer in
                try engine.render(into: buffer, channels: 2, frames: 256)
            }
        }
    }

    @Test("an engine with no source renders silence rather than faulting")
    func noSourceIsSilence() throws {
        let engine = try Engine(sampleRate: 48_000, channels: 2, maxBlockFrames: 512)
        try engine.placeTrack(track: 1, position: 0, length: 4_096)

        var block = [Float](repeating: 7, count: 2 * 64)
        try block.withUnsafeMutableBufferPointer { buffer in
            try engine.render(into: buffer, channels: 2, frames: 64)
        }

        #expect(
            block.allSatisfy { $0 == 0 },
            "the buffer was not cleared, so the host would hear its own leftovers"
        )
        #expect(try engine.renderWasComplete() == false)
    }

    @Test("a source that over-reports what it wrote is not believed")
    func overReportingSourceIsClamped() throws {
        let engine = try Engine(sampleRate: 48_000, channels: 2, maxBlockFrames: 512)
        try engine.setSource(OverReportingSource())
        try engine.placeTrack(track: 1, position: 0, length: 4_096)

        var block = [Float](repeating: 0, count: 2 * 64)
        // The point is that this does not corrupt anything or throw.
        try block.withUnsafeMutableBufferPointer { buffer in
            try engine.render(into: buffer, channels: 2, frames: 64)
        }
    }

    @Test("a stopped transport renders without the playhead running away")
    func stoppedTransportDoesNotAdvance() throws {
        // Every host renders while stopped, because the audio device keeps
        // asking.
        let engine = try Engine(sampleRate: 48_000, channels: 2, maxBlockFrames: 512)
        var block = [Float](repeating: 0, count: 2 * 128)

        for _ in 0..<8 {
            try block.withUnsafeMutableBufferPointer { buffer in
                try engine.render(into: buffer, channels: 2, frames: 128)
            }
        }
        #expect(try engine.position() == 0)
    }

    @Test("seeking puts the playhead where it was sent")
    func seeking() throws {
        let engine = try Engine(sampleRate: 48_000, channels: 2, maxBlockFrames: 512)
        try engine.seek(to: 1_024)
        #expect(try engine.position() == 1_024)
    }

    @Test("detaching a source is a defined state, not a fault")
    func detachingSource() throws {
        let engine = try Engine(sampleRate: 48_000, channels: 2, maxBlockFrames: 512)
        try engine.setSource(ConstantSource(value: 1, available: 1_024))
        try engine.setSource(nil)
        try engine.placeTrack(track: 1, position: 0, length: 1_024)

        var block = [Float](repeating: 3, count: 2 * 32)
        try block.withUnsafeMutableBufferPointer { buffer in
            try engine.render(into: buffer, channels: 2, frames: 32)
        }
        #expect(block.allSatisfy { $0 == 0 })
    }

    @Test("the source outlives the engine's ability to call it")
    func sourceStaysAlive() throws {
        // The lifetime bug this wrapper exists to make impossible: the core
        // holds a raw pointer and calls through it from the audio thread. If the
        // only other reference went away, the callback would run against freed
        // memory at the worst possible moment.
        let engine = try Engine(sampleRate: 48_000, channels: 2, maxBlockFrames: 512)
        do {
            let temporary = ConstantSource(value: 0.25, available: 4_096)
            try engine.setSource(temporary)
        }
        try engine.placeTrack(track: 1, position: 0, length: 4_096)

        var block = [Float](repeating: 0, count: 2 * 64)
        try block.withUnsafeMutableBufferPointer { buffer in
            try engine.render(into: buffer, channels: 2, frames: 64)
        }
        #expect(block.contains { $0 != 0 }, "the source was collected while still registered")
    }

    @Test("playback states carry the distinction a performer needs")
    func transientStates() {
        // Buffering is not pausing. One is the transport on its way somewhere
        // without help; the other is waiting for a person.
        #expect(PlaybackState.buffering.isTransient)
        #expect(PlaybackState.seeking.isTransient)
        #expect(PlaybackState.recovering.isTransient)
        #expect(!PlaybackState.paused.isTransient)
        #expect(PlaybackState.playing.isAudible)
        #expect(!PlaybackState.paused.isAudible)
    }
}

@Suite("Planning a set")
struct PlannerTests {

    /// Twelve mutually compatible five-minute records at 48 kHz.
    private static let trackFrames: Int64 = 48_000 * 300

    private func library(_ planner: Planner, count: UInt64 = 12) throws {
        for index in 0..<count {
            // Keys adjacent on the wheel and tempi within a couple of per cent,
            // so neither is the binding constraint on what can follow what.
            let semitones: Int32 = [9, 4, 2][Int(index % 3)]
            try planner.add(
                Candidate(
                    track: index,
                    duration: Self.trackFrames,
                    bpm: 126 + Double(index % 3),
                    energy: 0.3 + 0.05 * Float(index % 12),
                    key: Candidate.Key(semitones: semitones, isMinor: true),
                    loudnessLUFS: -8,
                    hasVocals: false
                )
            )
        }
    }

    @Test("a library plans a set that can be read back as a tracklist")
    func planAndRead() throws {
        let planner = try Planner()
        try library(planner)
        #expect(try planner.candidateCount() == 12)

        let alternatives = try planner.plan(
            targetFrames: Self.trackFrames * 6,
            sampleRate: 48_000,
            shape: .arc
        )
        #expect(alternatives >= 1)

        let tracks = try planner.tracks()
        #expect(tracks.count >= 2, "a set of one track is not a set")

        var seen = Set<UInt64>()
        var previousStart: Int64 = -1
        for planned in tracks {
            #expect(seen.insert(planned.track).inserted, "a track was used twice")
            #expect(planned.start > previousStart, "the set did not move forward")
            #expect((0...1).contains(planned.transitionScore))
            previousStart = planned.start
        }
        #expect(try planner.score() > 0)
    }

    @Test("the whole product: library, plan, timeline, audio")
    func endToEnd() throws {
        // Everything the application does, in the order a user does it.
        let engine = try Engine(sampleRate: 48_000, channels: 2, maxBlockFrames: 512)
        try engine.setSource(ConstantSource(value: 0.4, available: 8_192))

        let planner = try Planner()
        try library(planner)
        try planner.plan(
            targetFrames: Self.trackFrames * 6,
            sampleRate: 48_000,
            shape: .plateau
        )

        let planned = try planner.trackCount()
        try planner.apply(to: engine)
        #expect(
            try engine.placementCount() == planned,
            "the project does not hold the set that was planned"
        )

        try engine.apply(.load)
        try engine.apply(.loadSucceeded)
        try engine.apply(.play)

        var block = [Float](repeating: 0, count: 2 * 256)
        try block.withUnsafeMutableBufferPointer { buffer in
            try engine.render(into: buffer, channels: 2, frames: 256)
        }
        #expect(block.contains { $0 != 0 }, "a planned, applied set rendered silence")
    }

    @Test("a library with nothing in it is refused rather than returning no set")
    func emptyLibraryIsRefused() throws {
        // A real answer about the library. An empty plan would make "nothing you
        // own fits" indistinguishable from success.
        let planner = try Planner()
        #expect(throws: EngineError.refused) {
            try planner.plan(targetFrames: 1_000_000, sampleRate: 48_000, shape: .rising)
        }
    }

    @Test("reading a plan before making one says so")
    func readingBeforePlanning() throws {
        let planner = try Planner()
        #expect(throws: EngineError.invalidState) { try planner.trackCount() }
        #expect(throws: EngineError.invalidState) { try planner.duration() }
    }

    @Test("an alternative that does not exist is refused")
    func selectingOutOfRange() throws {
        let planner = try Planner()
        try library(planner)
        let count = try planner.plan(
            targetFrames: Self.trackFrames * 4,
            sampleRate: 48_000,
            shape: .rising
        )
        for index in 0..<count {
            try planner.select(index)
            #expect(try planner.trackCount() > 0)
        }
        #expect(throws: EngineError.invalidArgument) { try planner.select(count) }
    }

    @Test("a track with no key known is not a track known to have no key")
    func unknownKeyIsNotAbsentKey() throws {
        // `nil` maps to zero confidence, which the core scores as neutral rather
        // than as perfect. Getting this backwards would make an unanalysed
        // library look like a perfectly harmonic one.
        let planner = try Planner()
        try planner.add(
            Candidate(track: 1, duration: Self.trackFrames, bpm: 128, energy: 0.5)
        )
        #expect(try planner.candidateCount() == 1)
    }

    @Test("records that hand over early make a shorter set")
    func exitPointsShortenTheSet() throws {
        // The pacing rule, reachable from Swift. If the planner and the renderer
        // ever disagree about this again, it will be across two languages.
        let plain = try Planner()
        try library(plain)
        try plain.plan(
            targetFrames: Self.trackFrames * 6,
            sampleRate: 48_000,
            shape: .plateau
        )
        let without = try plain.duration()

        let early = try Planner()
        try library(early)
        for index in 0..<UInt64(12) {
            try early.add(
                MixPoint(
                    track: index,
                    position: Self.trackFrames / 4,
                    energy: 0.05,
                    isExit: true
                )
            )
        }
        try early.plan(
            targetFrames: Self.trackFrames * 6,
            sampleRate: 48_000,
            shape: .plateau
        )

        #expect(try early.duration() < without)
    }

    @Test("a mix point for a track nobody added is refused")
    func mixPointForUnknownTrack() throws {
        // A host calling in the wrong order. Ignoring it would lose the analysis
        // and nothing would say why the transitions came out worse.
        let planner = try Planner()
        #expect(throws: EngineError.invalidHandle) {
            try planner.add(MixPoint(track: 99, position: 1_000, energy: 0.1, isExit: true))
        }
    }

    @Test("a fact the planner cannot use is refused at the door")
    func unusableFacts() throws {
        let planner = try Planner()
        #expect(throws: EngineError.invalidArgument) {
            try planner.add(Candidate(track: 1, duration: 0, bpm: 128, energy: 0.5))
        }
        #expect(throws: EngineError.invalidArgument) {
            try planner.add(
                Candidate(track: 1, duration: Self.trackFrames, bpm: 0, energy: 0.5)
            )
        }
        #expect(try planner.candidateCount() == 0, "a refused candidate was kept")
    }
}
