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
