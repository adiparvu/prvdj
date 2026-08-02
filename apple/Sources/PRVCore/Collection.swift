import CPRVBridge
import Foundation

/// How a list of tracks is ordered.
public enum SortKey: Sendable, CaseIterable {
    case title, artist, album, dateAdded, lastPlayed, playCount, rating, duration, tempo, energy

    var code: Int32 {
        switch self {
        case .title: PRV_SORT_TITLE.rawValue
        case .artist: PRV_SORT_ARTIST.rawValue
        case .album: PRV_SORT_ALBUM.rawValue
        case .dateAdded: PRV_SORT_DATE_ADDED.rawValue
        case .lastPlayed: PRV_SORT_LAST_PLAYED.rawValue
        case .playCount: PRV_SORT_PLAY_COUNT.rawValue
        case .rating: PRV_SORT_RATING.rawValue
        case .duration: PRV_SORT_DURATION.rawValue
        case .tempo: PRV_SORT_TEMPO.rawValue
        case .energy: PRV_SORT_ENERGY.rawValue
        }
    }
}

/// The user's music.
///
/// # Reading is by index, on purpose
///
/// A search answers *how many* matched; the host reads identities back and asks
/// for the fields it is about to draw. A list showing forty rows of a
/// ten-thousand-track library does forty reads, not ten thousand.
public final class Collection {
    private let handle: OpaquePointer

    public init() throws {
        var created: OpaquePointer?
        try EngineError.check(prv_collection_create(&created))
        guard let created else { throw EngineError.invalidHandle }
        handle = created
    }

    deinit {
        prv_collection_destroy(handle)
    }

    /// Adds a track.
    ///
    /// - Throws: ``EngineError/refused`` when the identity is already in use. A
    ///   re-import is not a new track, and overwriting would lose whatever the
    ///   user had edited on the original.
    public func add(
        id: UInt64,
        title: String,
        artist: String = "",
        album: String = "",
        media: String,
        duration: Int64,
        importedAt: Date = Date()
    ) throws {
        let micros = Int64(importedAt.timeIntervalSince1970 * 1_000_000)
        try EngineError.check(
            prv_collection_add(handle, id, title, artist, album, media, duration, micros)
        )
    }

    /// Changes a track's title, artist and album. Its identity does not change.
    public func updateMetadata(
        id: UInt64,
        title: String,
        artist: String,
        album: String
    ) throws {
        try EngineError.check(
            prv_collection_update_metadata(handle, id, title, artist, album)
        )
    }

    /// Hides a track without destroying it.
    ///
    /// Its rating, tags and play count survive, and ``restore(id:)`` brings it
    /// back. Repeating the call succeeds and changes nothing — a host tidying up
    /// has not made a mistake worth reporting.
    public func remove(id: UInt64) throws {
        try EngineError.check(prv_collection_remove(handle, id))
    }

    /// Brings a removed track back, with everything it had.
    public func restore(id: UInt64) throws {
        try EngineError.check(prv_collection_restore(handle, id))
    }

    /// How many tracks the library holds.
    public var count: UInt64 {
        var value: UInt64 = 0
        guard prv_collection_count(handle, &value) == PRV_OK.rawValue else { return 0 }
        return value
    }

    /// Runs a search and returns the matching identities, in ranked order.
    ///
    /// An empty `text` matches everything, which is what a list showing the
    /// whole library asks for.
    public func search(
        _ text: String = "",
        sortedBy sort: SortKey = .title,
        descending: Bool = false
    ) throws -> [UInt64] {
        var found: UInt64 = 0
        try EngineError.check(
            prv_collection_search(handle, text, sort.code, descending ? 1 : 0, &found)
        )
        return try (0..<found).map { index in
            var id: UInt64 = 0
            try EngineError.check(prv_collection_result(handle, index, &id))
            return id
        }
    }

    /// One text field of a track.
    ///
    /// # Why this asks twice
    ///
    /// The boundary reports how many bytes a field needs whether or not it
    /// fitted, so a caller can allocate exactly. The first call here uses a
    /// stack-sized buffer that covers almost every real title; the second only
    /// happens for the rare long one, and is exact rather than another guess.
    public func text(_ field: Field, of id: UInt64) throws -> String {
        var small = [UInt8](repeating: 0, count: 128)
        var needed: UInt64 = 0

        let status = small.withUnsafeMutableBufferPointer { buffer in
            prv_collection_text_field(
                handle, id, field.code, buffer.baseAddress, UInt64(buffer.count), &needed
            )
        }
        if status == PRV_OK.rawValue {
            return Collection.string(from: small)
        }
        if let error = EngineError.from(code: status), error != .bufferTooSmall {
            throw error
        }

        var exact = [UInt8](repeating: 0, count: Int(needed))
        try exact.withUnsafeMutableBufferPointer { buffer in
            try EngineError.check(
                prv_collection_text_field(
                    handle, id, field.code, buffer.baseAddress, UInt64(buffer.count), &needed
                )
            )
        }
        return Collection.string(from: exact)
    }

    /// Which text field to read.
    public enum Field: Sendable, CaseIterable {
        case title, artist, album, genre, media

        var code: Int32 {
            switch self {
            case .title: PRV_FIELD_TITLE.rawValue
            case .artist: PRV_FIELD_ARTIST.rawValue
            case .album: PRV_FIELD_ALBUM.rawValue
            case .genre: PRV_FIELD_GENRE.rawValue
            case .media: PRV_FIELD_MEDIA.rawValue
            }
        }
    }

    /// A track's length in frames.
    public func duration(of id: UInt64) throws -> Int64 {
        var value: Int64 = 0
        try EngineError.check(prv_collection_duration(handle, id, &value))
        return value
    }

    /// Turns a NUL-terminated buffer into a string.
    ///
    /// Stops at the terminator rather than trusting the whole buffer, because
    /// everything after it is whatever the allocation held before.
    private static func string(from buffer: [UInt8]) -> String {
        let end = buffer.firstIndex(of: 0) ?? buffer.endIndex
        return String(decoding: buffer[..<end], as: UTF8.self)
    }
}
