import Foundation
import Testing

@testable import PRVCore

@Suite("The user's music")
struct CollectionTests {

    private func stocked() throws -> Collection {
        let collection = try Collection()
        for (id, title, artist) in [
            (UInt64(1), "Strobe", "deadmau5"),
            (2, "Alive", "Daft Punk"),
            (3, "Windowlicker", "Aphex Twin"),
        ] {
            try collection.add(
                id: id, title: title, artist: artist, album: "An Album",
                media: "file://\(id)", duration: 44_100 * 300
            )
        }
        return collection
    }

    @Test("a search finds what the user typed")
    func searching() throws {
        let collection = try stocked()
        #expect(try collection.search("dead") == [1])
        #expect(try collection.search("").count == 3, "an empty search is the whole library")
        #expect(try collection.search("nothing here").isEmpty)
    }

    @Test("fields come back as strings, including empty ones")
    func fields() throws {
        let collection = try stocked()
        #expect(try collection.text(.title, of: 1) == "Strobe")
        #expect(try collection.text(.artist, of: 1) == "deadmau5")
        // An empty field is an empty string, not a crash and not garbage from
        // whatever the buffer held before.
        #expect(try collection.text(.genre, of: 1) == "")
    }

    @Test("a title longer than the first buffer still comes back whole")
    func longTitle() throws {
        // The two-call path. A title of 300 characters does not fit the stack
        // buffer, and the second attempt is exact rather than another guess.
        let collection = try Collection()
        let long = String(repeating: "é", count: 300)
        try collection.add(id: 1, title: long, media: "file://1", duration: 100)
        #expect(try collection.text(.title, of: 1) == long)
    }

    @Test("removing hides a track and restoring brings it back intact")
    func removeAndRestore() throws {
        // A delete with no undo would be the opposite of "the user owns their
        // work", whatever it was called.
        let collection = try stocked()
        try collection.remove(id: 2)
        #expect(try collection.search("").count == 2)

        // Repeating it is not a mistake worth reporting.
        try collection.remove(id: 2)

        try collection.restore(id: 2)
        #expect(try collection.search("").count == 3)
        #expect(try collection.text(.title, of: 2) == "Alive")
    }

    @Test("a re-import is refused rather than silently replacing")
    func duplicateIdentity() throws {
        let collection = try stocked()
        #expect(throws: EngineError.refused) {
            try collection.add(id: 1, title: "Other", media: "file://x", duration: 1)
        }
    }

    @Test("renaming keeps the identity and is findable by the new name")
    func renaming() throws {
        let collection = try stocked()
        try collection.updateMetadata(
            id: 1, title: "Strobe (Edit)", artist: "deadmau5", album: "An Album"
        )
        #expect(try collection.search("edit") == [1])
    }

    @Test("an unknown track is refused rather than returning an empty string")
    func unknownTrack() throws {
        // An empty string would be indistinguishable from a track with no title.
        let collection = try stocked()
        #expect(throws: EngineError.invalidHandle) {
            _ = try collection.text(.title, of: 999)
        }
        #expect(throws: EngineError.invalidHandle) {
            _ = try collection.duration(of: 999)
        }
    }

    @Test("descending reverses the order")
    func ordering() throws {
        let collection = try stocked()
        let up = try collection.search(sortedBy: .title)
        let down = try collection.search(sortedBy: .title, descending: true)
        #expect(down == up.reversed())
    }

    @Test("non-ASCII titles survive the round trip")
    func unicode() throws {
        // The boundary refuses invalid UTF-8 rather than replacing it, and valid
        // UTF-8 has to come back byte for byte or a user's library is quietly
        // corrupted.
        let collection = try Collection()
        let title = "Björk — Jóga (Ремикс) 日本語"
        try collection.add(id: 1, title: title, media: "file://1", duration: 100)
        #expect(try collection.text(.title, of: 1) == title)
    }
}

@Suite("Getting a set out")
struct DeliveryTests {
    private static let rate: UInt32 = 44_100

    private func tone(amplitude: Float, seconds: Int) -> [Float] {
        let length = seconds * Int(Self.rate)
        return (0..<length).map { index in
            amplitude * sin(Float(index) * 0.05)
        }
    }

    @Test("a loud master is judged against a target and reports every number")
    func judging() throws {
        let analysis = try Analysis(samples: tone(amplitude: 0.5, seconds: 32), sampleRate: Self.rate)
        let verdict = try analysis.judge(for: .streaming)

        #expect(verdict.measuredLUFS.isFinite)
        #expect(verdict.measuredTruePeak.isFinite)
        #expect(verdict.resultingTruePeak.isFinite)
        // The resulting peak is the measured peak moved by exactly the gain —
        // the one part of this that needs no measurement.
        #expect(abs((verdict.measuredTruePeak + verdict.gainDB) - verdict.resultingTruePeak) < 0.001)
    }

    @Test("dither follows the depth, not the format")
    func ditherFollowsDepth() throws {
        // Dithering a float export adds noise for nothing.
        let analysis = try Analysis(samples: tone(amplitude: 0.5, seconds: 32), sampleRate: Self.rate)
        #expect(try analysis.judge(for: .club, format: .wave, depth: .float32).needsDither == false)
        #expect(try analysis.judge(for: .club, format: .wave, depth: .sixteen).needsDither)
    }

    @Test("a master far below target is not simply turned up")
    func veryQuietIsNotGained() throws {
        // Almost always a mistake upstream — a muted lane, the wrong project.
        // Applying twenty decibels produces a loud version of the wrong thing.
        let analysis = try Analysis(
            samples: tone(amplitude: 0.000_01, seconds: 32),
            sampleRate: Self.rate
        )
        let verdict = try analysis.judge(for: .streaming)
        #expect(verdict.gainDB == 0, "a near-silent master was gained up rather than questioned")
    }

    @Test("every target, format and depth is accepted")
    func everyCombinationIsDefined() throws {
        let analysis = try Analysis(samples: tone(amplitude: 0.4, seconds: 32), sampleRate: Self.rate)
        for target in DeliveryTarget.allCases {
            for format in AudioFormat.allCases {
                for depth in BitDepth.allCases {
                    let verdict = try analysis.judge(for: target, format: format, depth: depth)
                    #expect(
                        verdict.compliance != .unrecognised(code: -1),
                        "\(target)/\(format)/\(depth) produced no verdict"
                    )
                }
            }
        }
    }
}
