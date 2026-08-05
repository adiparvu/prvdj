import Foundation
import PRVCore
import Testing

@testable import PRVKit

/// A click track with a real pulse, long enough for the structure stage.
func pulsed(bpm: Double, seconds: Int, rate: UInt32 = 44_100) -> [Float] {
    let period = Int(60.0 / bpm * Double(rate))
    let length = seconds * Int(rate)
    var samples = [Float](repeating: 0, count: length)
    var index = 0
    while index < length {
        for offset in 0..<min(1_000, length - index) {
            samples[index + offset] += 0.8 * (1 - Float(offset) / 1_000) * sin(Float(offset) * 0.05)
        }
        index += max(period, 1)
    }
    return samples
}

@Suite("The session: import, plan, play")
struct SessionTests {
    private static let rate: UInt32 = 44_100

    private func item(_ id: UInt64) -> MediaItem {
        MediaItem(
            id: id,
            title: "Track \(id)",
            artist: "Someone",
            location: URL(fileURLWithPath: "/dev/null")
        )
    }

    private func session(tracks: Int, seconds: Int = 32) throws -> (Session, [MediaItem]) {
        var audio: [UInt64: [Float]] = [:]
        var items: [MediaItem] = []
        for index in 0..<UInt64(tracks) {
            audio[index] = pulsed(bpm: 126 + Double(index % 3), seconds: seconds, rate: Self.rate)
            items.append(item(index))
        }
        let session = try Session(
            sampleRate: Self.rate,
            decoder: InMemoryDecoder(tracks: audio)
        )
        return (session, items)
    }

    @Test("importing a track decodes it, analyses it and offers it to the planner")
    func importing() throws {
        let (session, items) = try session(tracks: 1)
        let facts = try session.import(items[0])

        #expect(session.library.count == 1)
        #expect(facts.bpm != nil, "a click track has a tempo")
        #expect(facts.isPlannable)
    }

    @Test("a track too short to analyse is still imported and still playable")
    func shortTrackIsKept() throws {
        // Dropping it would make a user's file vanish with no explanation. It is
        // imported, marked unplannable, and the interface says why.
        var audio: [UInt64: [Float]] = [:]
        audio[1] = pulsed(bpm: 128, seconds: 3, rate: Self.rate)
        let session = try Session(sampleRate: Self.rate, decoder: InMemoryDecoder(tracks: audio))

        let facts = try session.import(item(1))
        #expect(session.library.count == 1, "the track was dropped")
        #expect(!facts.isPlannable)
    }

    @Test("a file the decoder cannot read fails the import rather than the session")
    func undecodableFile() throws {
        let session = try Session(sampleRate: Self.rate, decoder: InMemoryDecoder(tracks: [:]))
        #expect(throws: PlatformError.self) {
            try session.import(item(7))
        }
        #expect(session.library.isEmpty)
    }

    @Test("the whole workflow: import, plan, adopt, play, render")
    func wholeWorkflow() throws {
        let (session, items) = try session(tracks: 3)
        for item in items {
            try session.import(item)
        }

        let alternatives = try session.planSet(minutes: 1, shape: .rising)
        #expect(!alternatives.isEmpty)
        #expect(alternatives[0].count >= 2)

        try session.adopt(alternative: 0)
        let afterAdopting = try session.snapshot()
        #expect(afterAdopting.placementCount == UInt64(alternatives[0].count))

        try session.play()
        #expect(try session.snapshot().playback == .playing)

        var block = [Float](repeating: 0, count: 2 * 256)
        try block.withUnsafeMutableBufferPointer { buffer in
            try session.renderBlock(into: buffer, frames: 256)
        }
        #expect(block.contains { $0 != 0 }, "the adopted set rendered silence")
    }

    @Test("planning with nothing usable is refused, and says so")
    func planningWithNothing() throws {
        let session = try Session(sampleRate: Self.rate, decoder: InMemoryDecoder(tracks: [:]))
        #expect(throws: EngineError.refused) {
            _ = try session.planSet(minutes: 60, shape: .arc)
        }
    }

    @Test("a snapshot is one consistent picture, not four separate reads")
    func snapshotIsConsistent() throws {
        let (session, items) = try session(tracks: 3)
        for item in items { try session.import(item) }
        _ = try session.planSet(minutes: 1, shape: .plateau)
        try session.adopt(alternative: 0)

        let snapshot = try session.snapshot()
        #expect(snapshot.duration > 0)
        #expect(snapshot.progress >= 0 && snapshot.progress <= 1)
        try session.seek(to: snapshot.duration / 2)
        #expect(try session.snapshot().progress > 0.4)
    }

    @Test("playing works without any audio device attached")
    func noDeviceNeeded() throws {
        // What makes the workflow testable, and what a preview relies on.
        let (session, items) = try session(tracks: 3)
        for item in items { try session.import(item) }
        _ = try session.planSet(minutes: 1, shape: .rising)
        try session.adopt(alternative: 0)
        try session.play()
        try session.pause()
    }
}

@Suite("Held audio")
struct LoadedAudioTests {
    @Test("a read past the end of a track returns nothing rather than garbage")
    func readPastEnd() {
        let audio = LoadedAudio()
        audio.hold(track: 1, samples: [1, 2, 3, 4])
        var block = [Float](repeating: 0, count: 8)

        let written = block.withUnsafeMutableBufferPointer { buffer in
            audio.read(
                track: 1,
                sourceOffset: 100,
                into: buffer,
                channels: 1,
                capacity: 8,
                destination: 0,
                frames: 4
            )
        }
        #expect(written == 0)
        #expect(block.allSatisfy { $0 == 0 })
    }

    @Test("a partial read reports what it actually wrote")
    func partialRead() {
        // The renderer treats the difference as silence and records it. Claiming
        // four frames while writing two would make it treat stale memory as
        // audio.
        let audio = LoadedAudio()
        audio.hold(track: 1, samples: [1, 2, 3, 4])
        var block = [Float](repeating: 0, count: 8)

        let written = block.withUnsafeMutableBufferPointer { buffer in
            audio.read(
                track: 1,
                sourceOffset: 2,
                into: buffer,
                channels: 1,
                capacity: 8,
                destination: 0,
                frames: 4
            )
        }
        #expect(written == 2)
        #expect(block[0] == 3 && block[1] == 4)
    }

    @Test("a track nobody held reads as silence")
    func unheldTrack() {
        let audio = LoadedAudio()
        var block = [Float](repeating: 9, count: 4)
        let written = block.withUnsafeMutableBufferPointer { buffer in
            audio.read(
                track: 42, sourceOffset: 0, into: buffer,
                channels: 1, capacity: 4, destination: 0, frames: 4
            )
        }
        #expect(written == 0)
    }

    @Test("mono audio is fanned out to every channel")
    func monoFanOut() {
        let audio = LoadedAudio()
        audio.hold(track: 1, samples: [0.5, 0.5])
        var block = [Float](repeating: 0, count: 2 * 4)
        _ = block.withUnsafeMutableBufferPointer { buffer in
            audio.read(
                track: 1, sourceOffset: 0, into: buffer,
                channels: 2, capacity: 4, destination: 0, frames: 2
            )
        }
        #expect(block[0] == 0.5 && block[1] == 0.5, "channel 0 was not filled")
        #expect(block[4] == 0.5 && block[5] == 0.5, "channel 1 was not filled")
    }

    @Test("releasing a track stops it being read")
    func releasing() {
        let audio = LoadedAudio()
        audio.hold(track: 1, samples: [1, 2])
        #expect(audio.count == 1)
        audio.release(track: 1)
        #expect(audio.count == 0)
        #expect(audio.samples(for: 1) == nil)
    }
}

@Suite("The file project store")
struct FileProjectStoreTests {
    @Test("a project round-trips through the store")
    func roundTrip() throws {
        let directory = URL(fileURLWithPath: NSTemporaryDirectory())
            .appendingPathComponent("prv-store-\(UInt64.random(in: 0..<(1 << 60)))")
        defer { try? FileManager.default.removeItem(at: directory) }

        let store = FileProjectStore(directory: directory)
        let payload = Data("an operation log".utf8)
        try store.save(payload, named: "friday")

        #expect(try store.load(named: "friday") == payload)
        #expect(try store.names() == ["friday"])
        #expect(try store.load(named: "saturday") == nil, "a missing project is nil, not an error")
    }
}

@Suite("A project that survives quitting")
struct ProjectPersistenceTests {

    private func session() throws -> Session {
        try Session(decoder: InMemoryDecoder(tracks: [:]), output: nil)
    }

    @Test("a project written to disk comes back the same project")
    func roundTripThroughAFile() throws {
        let directory = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString)
        let store = FileProjectStore(directory: directory)
        defer { try? FileManager.default.removeItem(at: directory) }

        let authored = try session()
        for index in 0..<4 {
            try authored.engineForTesting.placeTrack(
                track: UInt64(index + 1),
                position: Int64(index) * 48_000,
                length: 48_000
            )
        }
        try authored.save(to: store, named: "saturday")

        // A different session, as a relaunch is.
        let reopened = try session()
        #expect(try reopened.open(from: store, named: "saturday"))

        let before = try authored.placements()
        let after = try reopened.placements()
        #expect(after.count == 4)
        #expect(Set(after.map(\.id)) == Set(before.map(\.id)))
        #expect(try reopened.snapshot().duration == authored.snapshot().duration)
    }

    @Test("adding a track after opening does not overwrite one that was saved")
    func openingDoesNotReuseIdentities() throws {
        // The defect this test was written to catch, and did: the placement
        // allocator lives in memory, so a reopened project would hand the next
        // clip an identity the file had already spent — and the new placement
        // overwrote an existing one in the fold. A track vanished on the most
        // ordinary action there is.
        let directory = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString)
        let store = FileProjectStore(directory: directory)
        defer { try? FileManager.default.removeItem(at: directory) }

        let authored = try session()
        for index in 0..<3 {
            try authored.engineForTesting.placeTrack(
                track: UInt64(index + 1),
                position: Int64(index) * 48_000,
                length: 48_000
            )
        }
        try authored.save(to: store, named: "friday")

        let reopened = try session()
        #expect(try reopened.open(from: store, named: "friday"))
        #expect(try reopened.placements().count == 3)

        try reopened.engineForTesting.placeTrack(track: 9, position: 200_000, length: 48_000)
        #expect(try reopened.placements().count == 4, "a saved clip was overwritten")

        let identities = try reopened.placements().map(\.id)
        #expect(Set(identities).count == identities.count, "two clips share an identity")
    }

    @Test("opening something that is not there is an answer, not a failure")
    func openingWhatIsNotThere() throws {
        let directory = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString)
        let store = FileProjectStore(directory: directory)
        defer { try? FileManager.default.removeItem(at: directory) }

        // What an application does on launch. Throwing would make an ordinary
        // case into a failure the caller has to catch.
        #expect(try session().open(from: store, named: "never-saved") == false)
    }

    @Test("an empty project saves to something that opens")
    func emptyProject() throws {
        let directory = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString)
        let store = FileProjectStore(directory: directory)
        defer { try? FileManager.default.removeItem(at: directory) }

        try session().save(to: store, named: "blank")
        #expect(try store.names().contains("blank"))
        #expect(try session().open(from: store, named: "blank"))
    }

    @Test("saving twice keeps one project, not two")
    func savingIsIdempotent() throws {
        let directory = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString)
        let store = FileProjectStore(directory: directory)
        defer { try? FileManager.default.removeItem(at: directory) }

        let live = try session()
        try live.save(to: store, named: "set")
        try live.engineForTesting.placeTrack(track: 1, position: 0, length: 48_000)
        try live.save(to: store, named: "set")

        #expect(try store.names().filter { $0 == "set" }.count == 1)

        let reopened = try session()
        #expect(try reopened.open(from: store, named: "set"))
        #expect(try reopened.placements().count == 1, "the second save did not take")
    }
}
