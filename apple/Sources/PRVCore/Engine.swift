import CPRVBridge
import Foundation

/// What the transport is doing.
///
/// Mirrors the core's own state machine. Written out rather than derived from
/// the raw values so that a state added to the boundary is a compile error here
/// rather than a silent `unknown`.
public enum PlaybackState: Sendable, Equatable {
    case stopped
    case loading
    case ready
    case playing
    case paused
    case seeking
    /// Playback has run ahead of the decoder and is waiting for audio.
    ///
    /// Deliberately distinct from `paused`: the transport intends to continue
    /// and will do so without the user doing anything. A performer has to be
    /// able to tell those apart at a glance.
    case buffering
    /// The output device changed or was lost, and the chain is being rebuilt.
    case recovering
    case error
    /// A state this build of the wrapper does not know.
    case unrecognised(code: Int32)

    init(code: Int32) {
        self =
            switch code {
            case PRV_PLAYBACK_STOPPED.rawValue: .stopped
            case PRV_PLAYBACK_LOADING.rawValue: .loading
            case PRV_PLAYBACK_READY.rawValue: .ready
            case PRV_PLAYBACK_PLAYING.rawValue: .playing
            case PRV_PLAYBACK_PAUSED.rawValue: .paused
            case PRV_PLAYBACK_SEEKING.rawValue: .seeking
            case PRV_PLAYBACK_BUFFERING.rawValue: .buffering
            case PRV_PLAYBACK_RECOVERING.rawValue: .recovering
            case PRV_PLAYBACK_ERROR.rawValue: .error
            default: .unrecognised(code: code)
            }
    }

    /// Whether audio is being rendered.
    public var isAudible: Bool { self == .playing }

    /// Whether the transport is on its way somewhere without the user's help.
    ///
    /// What an interface shows a spinner for, as opposed to a paused badge.
    public var isTransient: Bool {
        switch self {
        case .loading, .seeking, .buffering, .recovering: true
        default: false
        }
    }
}

/// Something that can happen to the transport.
public enum TransportEvent: Sendable, Equatable {
    case load
    case loadSucceeded
    case loadFailed
    case unload
    case play
    case pause
    case stop
    case seekRequested
    case seekCompleted
    case bufferExhausted
    case bufferRefilled
    case deviceLost
    case deviceRestored
    case fault
    case reset

    var code: Int32 {
        switch self {
        case .load: PRV_EVENT_LOAD.rawValue
        case .loadSucceeded: PRV_EVENT_LOAD_SUCCEEDED.rawValue
        case .loadFailed: PRV_EVENT_LOAD_FAILED.rawValue
        case .unload: PRV_EVENT_UNLOAD.rawValue
        case .play: PRV_EVENT_PLAY.rawValue
        case .pause: PRV_EVENT_PAUSE.rawValue
        case .stop: PRV_EVENT_STOP.rawValue
        case .seekRequested: PRV_EVENT_SEEK_REQUESTED.rawValue
        case .seekCompleted: PRV_EVENT_SEEK_COMPLETED.rawValue
        case .bufferExhausted: PRV_EVENT_BUFFER_EXHAUSTED.rawValue
        case .bufferRefilled: PRV_EVENT_BUFFER_REFILLED.rawValue
        case .deviceLost: PRV_EVENT_DEVICE_LOST.rawValue
        case .deviceRestored: PRV_EVENT_DEVICE_RESTORED.rawValue
        case .fault: PRV_EVENT_FAULT.rawValue
        case .reset: PRV_EVENT_RESET.rawValue
        }
    }
}

/// Where the engine gets audio.
///
/// Implemented by whatever owns decoded audio — on Apple platforms that is
/// `PRVKit`'s decoder; in a test it is an array.
///
/// # This is called on the audio thread
///
/// Which means the same rules apply to it as to everything else there: do not
/// allocate, do not lock, do not touch the filesystem. Return fewer frames than
/// asked for rather than blocking; the engine records the shortfall and carries
/// on, which is a glitch the user can be told about instead of a dropout they
/// merely hear.
public protocol AudioSource: AnyObject {
    /// Fills `buffer` with `frames` frames of `track`, starting `sourceOffset`
    /// frames into it and writing from `destination` onward within each channel.
    ///
    /// `buffer` is channel-major: `capacity` frames of channel 0, then
    /// `capacity` frames of channel 1. Returns how many frames were written.
    func read(
        track: UInt64,
        sourceOffset: Int64,
        into buffer: UnsafeMutableBufferPointer<Float>,
        channels: Int,
        capacity: Int,
        destination: Int,
        frames: Int
    ) -> Int
}

/// The only part of an engine the audio thread may touch.
///
/// # Why this exists rather than making `Engine` sendable
///
/// The boundary's rule is that one thread calls `render` and one other thread
/// calls everything else. Marking ``Engine`` `@unchecked Sendable` would express
/// that as a comment and let the compiler wave through `placeTrack` from a
/// render callback — which is an allocation, on the audio thread, and exactly
/// the defect ADR-0002 exists to prevent.
///
/// A handle carrying only the render call turns the rule into a type. The
/// audio thread is handed one of these and cannot reach anything else, so the
/// mistake stops compiling instead of stopping the music.
///
/// `@unchecked` is still required because the pointer is opaque to Swift's
/// concurrency checking. What it now covers is one function rather than a class.
public struct RenderHandle: @unchecked Sendable {
    fileprivate let handle: OpaquePointer

    /// How many channels the engine was built for.
    public let channels: Int

    /// Renders one block. See ``Engine/render(into:channels:frames:)``.
    public func render(into buffer: UnsafeMutableBufferPointer<Float>, frames: Int) throws {
        guard let base = buffer.baseAddress else { throw EngineError.nullPointer }
        guard buffer.count >= channels * frames else { throw EngineError.bufferTooSmall }
        try EngineError.check(
            prv_engine_render(handle, base, UInt32(clamping: channels), UInt32(clamping: frames))
        )
    }
}

/// The core, as Swift sees it.
///
/// # What this type is responsible for
///
/// Exactly one thing: making it impossible to use the C boundary wrongly. It
/// owns the handle and destroys it, it turns every status into a thrown error,
/// and it keeps the audio source alive for as long as the engine can call it.
/// It adds no behaviour of its own — everything it can do, the core decided.
///
/// # Thread confinement
///
/// The boundary is not synchronised, and this class does not make it so. The
/// arrangement the architecture overview describes is one audio thread calling
/// ``render(into:channels:frames:)`` and nothing else, and one other thread
/// calling everything else. A `final class` rather than an actor because an
/// actor cannot be called from a realtime context — awaiting is exactly what the
/// audio thread must never do.
public final class Engine {
    /// The opaque handle. Never null between `init` and `deinit`.
    private let handle: OpaquePointer

    /// The handle, for the one other type in this module that needs it.
    ///
    /// `internal` rather than `public`: `Planner.apply(to:)` has to name the
    /// engine it is applying to, and everything above this module should be
    /// unable to get at a raw pointer at all. That is the whole point of the
    /// wrapper.
    var rawHandle: OpaquePointer { handle }

    /// The registered source.
    ///
    /// Held strongly and deliberately. The core keeps a raw pointer to the box
    /// below and will call through it from the audio thread; if the only other
    /// reference went away, the callback would run against freed memory at the
    /// worst possible moment.
    private var source: AudioSource?

    /// The box the raw `user_data` pointer refers to.
    ///
    /// A separate allocation rather than `Unmanaged.passUnretained(self)`
    /// because the callback needs the *source*, and routing it through the
    /// engine would mean the audio thread reading a property the main thread can
    /// write.
    private final class SourceBox {
        var source: AudioSource?
        init(source: AudioSource?) { self.source = source }
    }
    private let box: UnsafeMutablePointer<SourceBox>

    /// How many channels the engine was built for.
    public let channels: Int

    /// The largest block the engine will render.
    public let maxBlockFrames: Int

    /// The version of the boundary this library was loaded against.
    public static var abiVersion: UInt32 { prv_abi_version() }

    /// The major version this wrapper was written for.
    ///
    /// Read from the header at compile time, so the two cannot disagree without
    /// somebody recompiling.
    public static var expectedMajor: UInt32 { UInt32(PRV_ABI_MAJOR) }

    /// Whether the loaded library can serve this wrapper.
    public static var isCompatible: Bool {
        prv_abi_is_compatible(expectedMajor) != 0
    }

    /// Builds an engine.
    ///
    /// - Throws: ``EngineError/invalidArgument`` when the shape is one the core
    ///   refuses, and ``EngineError/refused`` when the loaded library is a major
    ///   version this wrapper was not built for. The version check happens
    ///   *first*, before any pointer is exchanged, because it is the only check
    ///   that is still meaningful when everything else has moved.
    public init(sampleRate: UInt32, channels: Int, maxBlockFrames: Int) throws {
        guard Engine.isCompatible else {
            throw EngineError.refused
        }

        var created: OpaquePointer?
        let status = prv_engine_create(
            sampleRate,
            UInt32(clamping: channels),
            UInt32(clamping: maxBlockFrames),
            &created
        )
        try EngineError.check(status)
        guard let created else {
            // The boundary promises a handle on success. Reaching here would
            // mean it broke that promise, which is worth naming rather than
            // force-unwrapping through.
            throw EngineError.invalidHandle
        }

        self.handle = created
        self.channels = channels
        self.maxBlockFrames = maxBlockFrames
        self.box = UnsafeMutablePointer<SourceBox>.allocate(capacity: 1)
        self.box.initialize(to: SourceBox(source: nil))
    }

    deinit {
        prv_engine_destroy(handle)
        box.deinitialize(count: 1)
        box.deallocate()
    }

    /// Registers where audio comes from.
    ///
    /// Passing `nil` detaches the source, after which the engine renders silence
    /// and reports every placement incomplete — a defined state, not a fault.
    public func setSource(_ source: AudioSource?) throws {
        self.source = source
        box.pointee.source = source

        let callback: PrvReadAudio? =
            source == nil
            ? nil
            : { userData, track, sourceOffset, planar, channels, capacity, destination, frames in
                guard let userData, let planar else { return 0 }
                let box = userData.assumingMemoryBound(to: SourceBox.self)
                guard let source = box.pointee.source else { return 0 }

                let count = Int(channels) * Int(capacity)
                let buffer = UnsafeMutableBufferPointer(start: planar, count: count)
                let written = source.read(
                    track: track,
                    sourceOffset: sourceOffset,
                    into: buffer,
                    channels: Int(channels),
                    capacity: Int(capacity),
                    destination: Int(destination),
                    frames: Int(frames)
                )
                return UInt32(clamping: written)
            }

        try EngineError.check(
            prv_engine_set_source(handle, callback, UnsafeMutableRawPointer(box))
        )
    }

    /// Applies a transport event.
    ///
    /// - Throws: ``EngineError/invalidState`` when the transition is not one the
    ///   machine defines. That is an answer, not a malfunction: an interface
    ///   should say why the button did nothing rather than swallow it.
    public func apply(_ event: TransportEvent) throws {
        try EngineError.check(prv_engine_transport(handle, event.code))
    }

    /// Moves the playhead to an absolute frame position.
    public func seek(to position: Int64) throws {
        try EngineError.check(prv_engine_seek(handle, position))
    }

    /// The playhead position, in frames.
    public func position() throws -> Int64 {
        var value: Int64 = 0
        try EngineError.check(prv_engine_position(handle, &value))
        return value
    }

    /// What the transport is doing.
    public func playbackState() throws -> PlaybackState {
        var value: Int32 = 0
        try EngineError.check(prv_engine_playback_state(handle, &value))
        return PlaybackState(code: value)
    }

    /// Places a track on the timeline.
    ///
    /// - Parameter sourceOffset: how far into the track playback begins.
    /// - Parameter timestamp: when the edit was made. Supplied by the caller
    ///   because the core has no clock of its own.
    /// - Returns: the placement's identity.
    @discardableResult
    public func placeTrack(
        track: UInt64,
        position: Int64,
        length: Int64,
        sourceOffset: Int64 = 0,
        lane: UInt32 = 0,
        timestamp: Date = Date()
    ) throws -> UInt64 {
        var placement: UInt64 = 0
        let micros = Int64(timestamp.timeIntervalSince1970 * 1_000_000)
        try EngineError.check(
            prv_engine_place_track(
                handle,
                track,
                position,
                length,
                sourceOffset,
                lane,
                micros,
                &placement
            )
        )
        return placement
    }

    /// How long the project is, in frames.
    public func duration() throws -> Int64 {
        var value: Int64 = 0
        try EngineError.check(prv_engine_duration(handle, &value))
        return value
    }

    /// How many placements the project holds.
    public func placementCount() throws -> UInt64 {
        var value: UInt64 = 0
        try EngineError.check(prv_engine_placement_count(handle, &value))
        return value
    }

    /// Renders one block.
    ///
    /// `buffer` is channel-major and must hold at least `channels * frames`
    /// floats.
    ///
    /// # This is the audio thread
    ///
    /// No allocation happens here and none happens below it. `throws` is used
    /// rather than a returned status because Swift's error path allocates
    /// nothing when nothing is thrown, and a render that fails has already
    /// stopped being realtime.
    public func render(
        into buffer: UnsafeMutableBufferPointer<Float>,
        channels: Int,
        frames: Int
    ) throws {
        guard let base = buffer.baseAddress else {
            throw EngineError.nullPointer
        }
        guard buffer.count >= channels * frames else {
            throw EngineError.bufferTooSmall
        }
        try EngineError.check(
            prv_engine_render(handle, base, UInt32(clamping: channels), UInt32(clamping: frames))
        )
    }

    /// A handle the audio thread may hold.
    ///
    /// Obtained once, on the thread that owns the engine, and handed to the
    /// device. It borrows the engine's handle and does not extend its lifetime,
    /// so the engine must outlive the device — which is the caller's job and is
    /// what ``PRVKit`` arranges by owning both.
    public var renderHandle: RenderHandle {
        RenderHandle(handle: handle, channels: channels)
    }

    /// Whether every placement the last render touched was read in full.
    ///
    /// Worth showing a user — "some audio was missing" — and not worth treating
    /// as an error: the render happened and what was there is correct.
    public func renderWasComplete() throws -> Bool {
        var value: Int32 = 0
        try EngineError.check(prv_engine_render_was_complete(handle, &value))
        return value != 0
    }

    // MARK: - Synchronisation

    /// Declares which device this is.
    ///
    /// Call it before the project holds anything. The value must be stable for
    /// this installation and distinct from every other — both facts about the
    /// machine, which is why the core cannot supply them. ``PRVKit`` reads one
    /// from the platform's own storage and passes it here.
    public func setDevice(_ device: UInt64) throws {
        try EngineError.check(prv_engine_set_device(handle, device))
    }

    /// What this project has already seen, for a peer to answer.
    ///
    /// The first half of a synchronisation, and the reason it is incremental:
    /// the other side replies with what this vector has not seen rather than
    /// with the whole project.
    public func syncState() throws -> [UInt8] {
        try readBytes { into, capacity, needed in
            prv_engine_sync_state(handle, into, capacity, needed)
        }
    }

    /// The operations a peer has not seen.
    ///
    /// Two calls underneath: one that builds the message and reports its size,
    /// and one that copies it. That is what lets the caller allocate exactly
    /// once for a message whose size nobody can predict.
    public func syncMessage(for peerState: [UInt8]) throws -> [UInt8] {
        var needed: UInt64 = 0
        try peerState.withUnsafeBufferPointer { peer in
            try EngineError.check(
                prv_engine_sync_prepare(handle, peer.baseAddress, UInt64(peer.count), &needed)
            )
        }
        return try readBytes { into, capacity, size in
            prv_engine_sync_outbound(handle, into, capacity, size)
        }
    }

    /// Merges what a peer sent.
    @discardableResult
    public func merge(_ message: [UInt8]) throws -> SyncReport {
        var applied: UInt64 = 0
        var alreadyPresent: UInt64 = 0
        var conflicts: UInt64 = 0
        var carried: UInt64 = 0
        try message.withUnsafeBufferPointer { bytes in
            try EngineError.check(
                prv_engine_sync_merge(
                    handle,
                    bytes.baseAddress,
                    UInt64(bytes.count),
                    &applied,
                    &alreadyPresent,
                    &conflicts,
                    &carried
                )
            )
        }
        return SyncReport(
            applied: applied,
            alreadyPresent: alreadyPresent,
            conflicts: conflicts,
            carried: carried
        )
    }

    /// Runs the boundary's ask-then-fill convention once.
    ///
    /// Every call that returns bytes writes the size it needs whether or not the
    /// buffer fitted, so the first call passes nothing and the second allocates
    /// exactly. Written once here rather than at each call site, because the
    /// version of this that gets it wrong is the one that was written out by
    /// hand for the fourth time.
    private func readBytes(
        _ call: (UnsafeMutablePointer<UInt8>?, UInt64, UnsafeMutablePointer<UInt64>?) -> Int32
    ) throws -> [UInt8] {
        var needed: UInt64 = 0
        let status = call(nil, 0, &needed)
        if status != PRV_OK.rawValue && status != PRV_BUFFER_TOO_SMALL.rawValue {
            try EngineError.check(status)
        }
        guard needed > 0 else { return [] }

        var bytes = [UInt8](repeating: 0, count: Int(needed))
        var written: UInt64 = 0
        try bytes.withUnsafeMutableBufferPointer { buffer in
            try EngineError.check(call(buffer.baseAddress, UInt64(buffer.count), &written))
        }
        return bytes
    }
}

/// What a merge did.
///
/// Four counts rather than one, because they mean four different things to
/// whoever is looking at the screen.
public struct SyncReport: Equatable, Sendable {
    /// Operations that were new and have been applied.
    public let applied: UInt64
    /// Operations already present. The normal case, not a problem: a device
    /// sends whatever the other side might not have, and overlap is expected.
    public let alreadyPresent: UInt64
    /// Pairs that disagree and need a person to decide.
    public let conflicts: UInt64
    /// Operations made by a newer version of the app.
    ///
    /// They travel — a message carrying them can be passed on byte for byte, so
    /// an older install relays rather than blocking a fleet — but they cannot be
    /// shown here. A non-zero value means "part of this project was made with a
    /// newer version", which is the true and useful thing to say.
    public let carried: UInt64

    /// Whether anything needs a person.
    public var needsReview: Bool { conflicts > 0 }

    /// Whether anything arrived that this build cannot show.
    public var needsANewerVersion: Bool { carried > 0 }
}
