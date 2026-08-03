import Foundation
import Testing

@testable import PRVCore

/// Synchronisation, as far as Swift can see it.
///
/// The core produces bytes and reads bytes; nothing here opens a socket, and
/// that is the point — the same four calls serve a cloud service, a local
/// network, a memory stick and a file attached to an email.
@Suite("Two projects and the bytes between them")
struct SyncTests {

    /// A project with one placement in it, on a device that has said who it is.
    private func project(device: UInt64, tracks: Int = 1) throws -> Engine {
        let engine = try Engine(sampleRate: 48_000, channels: 2, maxBlockFrames: 512)
        try engine.setDevice(device)
        for index in 0..<tracks {
            try engine.placeTrack(
                track: UInt64(index + 1),
                position: Int64(index) * 4_096,
                length: 4_096
            )
        }
        return engine
    }

    @Test("an edit made on one machine arrives on the other")
    func anEditTravels() throws {
        let studio = try project(device: 1)
        let laptop = try Engine(sampleRate: 48_000, channels: 2, maxBlockFrames: 512)
        try laptop.setDevice(2)

        let message = try studio.syncMessage(for: laptop.syncState())
        let report = try laptop.merge(message)

        #expect(report.applied > 0)
        #expect(!report.needsReview)
        #expect(!report.needsANewerVersion)
        #expect(try laptop.duration() == studio.duration())
        #expect(try laptop.placementCount() == studio.placementCount())
    }

    @Test("only what the other side is missing is sent")
    func sendingIsIncremental() throws {
        let studio = try project(device: 1, tracks: 8)
        let laptop = try Engine(sampleRate: 48_000, channels: 2, maxBlockFrames: 512)
        try laptop.setDevice(2)

        let first = try studio.syncMessage(for: laptop.syncState())
        try laptop.merge(first)

        // Everything has arrived, so the next exchange carries nothing but its
        // own framing. That is the whole reason a version vector goes first.
        let second = try studio.syncMessage(for: laptop.syncState())
        #expect(second.count < first.count / 4)

        let report = try laptop.merge(second)
        #expect(report.applied == 0)
    }

    @Test("a message delivered twice is recognised rather than applied twice")
    func repeatedDeliveryCostsNothing() throws {
        let studio = try project(device: 1)
        let laptop = try Engine(sampleRate: 48_000, channels: 2, maxBlockFrames: 512)
        try laptop.setDevice(2)

        let message = try studio.syncMessage(for: laptop.syncState())
        let first = try laptop.merge(message)
        let again = try laptop.merge(message)

        #expect(first.applied > 0)
        #expect(again.applied == 0)
        #expect(again.alreadyPresent == first.applied)
        #expect(try laptop.placementCount() == studio.placementCount())
    }

    @Test("both sides end up with the same project, and neither is in charge")
    func bothDirectionsConverge() throws {
        let studio = try project(device: 1, tracks: 3)
        let laptop = try project(device: 2, tracks: 2)

        let outbound = try studio.syncMessage(for: laptop.syncState())
        let inbound = try laptop.syncMessage(for: studio.syncState())
        try laptop.merge(outbound)
        try studio.merge(inbound)

        #expect(try studio.placementCount() == laptop.placementCount())
        // Five, not three: two devices editing offline must not name the same
        // placement, or each machine's clips would land on top of the other's.
        #expect(try studio.placementCount() == 5)
        #expect(try studio.duration() == laptop.duration())
    }

    /// A message from a build that does not exist yet, carrying one operation
    /// this one cannot read. Built by hand, because the encoder cannot make it.
    private func aMessageFromTheFuture(author: UInt64, sequence: UInt64) -> [UInt8] {
        var entry: [UInt8] = []
        entry.append(contentsOf: withUnsafeBytes(of: author.littleEndian, Array.init))
        entry.append(contentsOf: withUnsafeBytes(of: sequence.littleEndian, Array.init))
        entry.append(contentsOf: withUnsafeBytes(of: Int64(0).littleEndian, Array.init))
        entry.append(contentsOf: withUnsafeBytes(of: UInt32(0).littleEndian, Array.init))
        entry.append(contentsOf: withUnsafeBytes(of: UInt16(60_000).littleEndian, Array.init))
        entry.append(contentsOf: withUnsafeBytes(of: UInt32(4).littleEndian, Array.init))
        entry.append(contentsOf: Array("soon".utf8))

        var message: [UInt8] = Array("PRVL".utf8)
        message.append(contentsOf: withUnsafeBytes(of: UInt16(1).littleEndian, Array.init))
        message.append(contentsOf: withUnsafeBytes(of: UInt16(0).littleEndian, Array.init))
        message.append(contentsOf: withUnsafeBytes(of: UInt32(4).littleEndian, Array.init))
        message.append(contentsOf: withUnsafeBytes(of: UInt32(1).littleEndian, Array.init))
        message.append(
            contentsOf: withUnsafeBytes(of: UInt32(entry.count).littleEndian, Array.init))
        message.append(contentsOf: entry)
        return message
    }

    @Test("an install a version behind carries work to the one that can read it")
    func aStaleInstallIsARelay() throws {
        let relay = try project(device: 2)
        let report = try relay.merge(aMessageFromTheFuture(author: 3, sequence: 1))

        #expect(report.applied == 0)
        #expect(report.carried == 1)
        #expect(report.needsANewerVersion)
        #expect(try relay.carriedCount() == 1)

        let onward = try Engine(sampleRate: 48_000, channels: 2, maxBlockFrames: 512)
        try onward.setDevice(4)
        let arrived = try onward.merge(relay.syncMessage(for: onward.syncState()))

        // The relay's own edit and the one it cannot read, in the same message.
        #expect(arrived.applied == 1)
        #expect(arrived.carried == 1)
        #expect(try onward.carriedCount() == 1)
    }

    @Test("work still beyond this build survives an attempt to promote it")
    func promotingIsSafeToRepeat() throws {
        let relay = try project(device: 2)
        try relay.merge(aMessageFromTheFuture(author: 3, sequence: 1))

        #expect(try relay.promoteCarried() == 0)
        #expect(try relay.carriedCount() == 1)
    }

    @Test("bytes that are not a message are refused rather than guessed at")
    func rubbishIsRefused() throws {
        let engine = try project(device: 1)
        let rubbish: [UInt8] = Array("this is not a project".utf8)

        #expect(throws: EngineError.invalidArgument) {
            try engine.merge(rubbish)
        }
        #expect(throws: EngineError.invalidArgument) {
            _ = try engine.syncMessage(for: rubbish)
        }
        // Including the empty case, which a truncated read produces.
        #expect(throws: EngineError.invalidArgument) {
            try engine.merge([])
        }
    }

    @Test("an identity is required to be plausible and cannot change underneath a log")
    func identityIsDeclaredOnce() throws {
        let engine = try Engine(sampleRate: 48_000, channels: 2, maxBlockFrames: 512)

        #expect(throws: EngineError.invalidArgument) {
            try engine.setDevice(0)
        }
        try engine.setDevice(7)
        try engine.placeTrack(track: 1, position: 0, length: 4_096)

        #expect(throws: EngineError.invalidState) {
            try engine.setDevice(8)
        }
    }

    @Test("an empty project still has something to say about what it has seen")
    func anEmptyProjectHasState() throws {
        let engine = try Engine(sampleRate: 48_000, channels: 2, maxBlockFrames: 512)
        let state = try engine.syncState()

        // Not empty: a peer has to be able to tell "I have seen nothing" from
        // "I did not answer".
        #expect(!state.isEmpty)

        let message = try engine.syncMessage(for: state)
        #expect(!message.isEmpty)
        #expect(try engine.merge(message).applied == 0)
    }
}
