import Foundation
import PRVCore

/// A track the library knows about, and where its audio is.
public struct MediaItem: Sendable, Equatable, Identifiable {
    public var id: UInt64
    public var title: String
    public var artist: String
    /// Where the audio lives. Opaque to everything above the decoder.
    public var location: URL

    public init(id: UInt64, title: String, artist: String, location: URL) {
        self.id = id
        self.title = title
        self.artist = artist
        self.location = location
    }
}

/// Turns a file into samples.
///
/// # Why this is a protocol and not a class
///
/// It is the one place a platform genuinely differs. AVFoundation decodes on
/// Apple platforms; a test decodes from an array; a future Linux build decodes
/// with something else. Everything above this line is written once.
public protocol MediaDecoder: Sendable {
    /// Decodes a whole track to mono samples at `sampleRate`.
    ///
    /// Mono because that is what analysis wants, and analysis is the only
    /// caller that needs the whole track at once. Playback reads through
    /// ``AudioSource`` instead, block by block.
    func decodeMono(_ item: MediaItem, sampleRate: UInt32) throws -> [Float]
}

/// Sends rendered blocks to a device.
///
/// # The render callback is the contract
///
/// An implementation calls `render` from whatever thread its device uses, and
/// that thread obeys the realtime rules: no allocation, no locking, no file
/// access. The protocol cannot enforce that — nothing in Swift can — so it is
/// stated here, where somebody writing a second implementation will read it.
public protocol AudioOutput: AnyObject {
    /// Begins pulling blocks.
    ///
    /// `render` is handed a channel-major buffer and a frame count and must
    /// fill it. It is called on the device's own thread.
    func start(
        channels: Int,
        sampleRate: UInt32,
        render: @escaping @Sendable (UnsafeMutableBufferPointer<Float>, Int) -> Void
    ) throws

    /// Stops pulling blocks. Idempotent.
    func stop()

    /// Whether blocks are being pulled.
    var isRunning: Bool { get }
}

/// Keeps something the user would not want written to a log.
///
/// Deliberately tiny. `prv-security` already decided *what* may be stored and
/// who may read it; this only says where the bytes go, which is the one part
/// that is a platform's business.
public protocol SecretStore: Sendable {
    func store(_ secret: Data, forKey key: String) throws
    func secret(forKey key: String) throws -> Data?
    func removeSecret(forKey key: String) throws
}

/// Reads and writes a project's operation log.
public protocol ProjectStore: Sendable {
    func save(_ data: Data, named name: String) throws
    func load(named name: String) throws -> Data?
    func names() throws -> [String]
}

/// What went wrong in the platform layer.
///
/// Separate from ``EngineError``: those come from the core's own decisions, and
/// these come from the world. Merging them would make "the deck has nothing
/// loaded" and "the disk is full" the same kind of thing.
public enum PlatformError: Error, Equatable, Sendable {
    /// The file could not be read or does not exist.
    case cannotRead(String)
    /// The audio device would not start.
    case deviceUnavailable
    /// The format is one this build cannot decode.
    case unsupportedFormat(String)
    /// A secret could not be stored or read.
    case secureStoreFailed(String)
    /// The platform this build runs on has no implementation of this port.
    ///
    /// Not a defect: a Linux build genuinely has no CoreAudio. Naming it is what
    /// stops a missing adapter from looking like a crash.
    case notAvailableOnThisPlatform(String)
}
