import CPRVBridge
import Foundation

/// Where synchronisation is.
///
/// Written out rather than derived from the raw values so that a state added to
/// the boundary is a compile error here rather than a silent `unknown`.
public enum SyncState: Sendable, Equatable {
    /// No network worth trying.
    ///
    /// The state an installation starts in, before anything has confirmed
    /// otherwise. Assuming the optimistic one would make the first seconds of
    /// every launch a lie.
    case offline
    /// Connected, with nothing to do.
    case idle
    /// Sending what was authored here.
    case sending
    /// Receiving what was authored elsewhere.
    case receiving
    /// Two devices changed the same thing and somebody has to choose.
    case conflicted
    /// The user switched it off.
    case paused
    /// A state this build of the wrapper does not know.
    case unrecognised(code: Int32)

    init(code: Int32) {
        self =
            switch code {
            case PRV_SYNC_OFFLINE.rawValue: .offline
            case PRV_SYNC_IDLE.rawValue: .idle
            case PRV_SYNC_SENDING.rawValue: .sending
            case PRV_SYNC_RECEIVING.rawValue: .receiving
            case PRV_SYNC_CONFLICTED.rawValue: .conflicted
            case PRV_SYNC_PAUSED.rawValue: .paused
            default: .unrecognised(code: code)
            }
    }

    /// A stable identifier, for localisation.
    public var key: String {
        switch self {
        case .offline: "sync.offline"
        case .idle: "sync.idle"
        case .sending: "sync.sending"
        case .receiving: "sync.receiving"
        case .conflicted: "sync.conflicted"
        case .paused: "sync.paused"
        case .unrecognised(let code): "sync.unrecognised.\(code)"
        }
    }
}

/// Something that happened to synchronisation.
///
/// A host reports these. It does not decide what they mean — the rules about
/// what each one does to the state live in the core, so that a second
/// implementation of them cannot drift from the first.
public enum SyncEvent: Sendable, Equatable {
    case networkAvailable
    case networkLost
    case workToSend
    case workArrived
    case transferFinished
    case conflictFound
    case conflictResolved
    case pause
    case resume

    var code: Int32 {
        switch self {
        case .networkAvailable: PRV_SYNC_EVENT_NETWORK_AVAILABLE.rawValue
        case .networkLost: PRV_SYNC_EVENT_NETWORK_LOST.rawValue
        case .workToSend: PRV_SYNC_EVENT_WORK_TO_SEND.rawValue
        case .workArrived: PRV_SYNC_EVENT_WORK_ARRIVED.rawValue
        case .transferFinished: PRV_SYNC_EVENT_TRANSFER_FINISHED.rawValue
        case .conflictFound: PRV_SYNC_EVENT_CONFLICT_FOUND.rawValue
        case .conflictResolved: PRV_SYNC_EVENT_CONFLICT_RESOLVED.rawValue
        case .pause: PRV_SYNC_EVENT_PAUSE.rawValue
        case .resume: PRV_SYNC_EVENT_RESUME.rawValue
        }
    }
}

/// One consistent reading of where synchronisation is.
///
/// Taken in a single call rather than assembled from several, for the same
/// reason the transport's snapshot is: four separate reads can straddle a change
/// and produce a picture that was never true — "transferring, with nothing
/// waiting", say, which sends whoever is looking at it hunting for a bug that is
/// not there.
public struct SyncSnapshot: Sendable, Equatable {
    public let state: SyncState
    /// Always true. See ``Sync/editingIsAllowed``.
    public let editingIsAllowed: Bool
    public let isTransferring: Bool
    /// Whether something is waiting on a person.
    public let needsTheUser: Bool
    /// How much work has been done here and gone nowhere yet.
    public let waiting: UInt64
    /// Whether that is close enough to the bound to be worth mentioning.
    public let isNearlyFull: Bool
}

/// The synchronisation state of one installation.
///
/// A separate handle from ``Engine``, because this belongs to an installation
/// rather than to a project: a user with three projects open is not offline
/// three times, and pausing synchronisation pauses it for the application.
public final class Sync {
    private let handle: OpaquePointer

    public init() throws {
        var created: OpaquePointer?
        try EngineError.check(prv_sync_create(&created))
        guard let created else { throw EngineError.nullPointer }
        handle = created
    }

    deinit { prv_sync_destroy(handle) }

    /// Reports something that happened.
    public func apply(_ event: SyncEvent) throws {
        try EngineError.check(prv_sync_apply(handle, event.code))
    }

    /// The current state.
    public func state() throws -> SyncState {
        var code: Int32 = 0
        try EngineError.check(prv_sync_state(handle, &code))
        return SyncState(code: code)
    }

    /// Whether the user may go on editing.
    ///
    /// Always true, in every state. Exposed because a caller that has to ask is
    /// a caller that was considering disabling something, and the answer it gets
    /// is that offline is the normal case rather than a mode with fewer
    /// features.
    public func editingIsAllowed() throws -> Bool {
        try snapshot().editingIsAllowed
    }

    /// Records that an operation was authored here and has gone nowhere yet.
    public func hold(device: UInt64, sequence: UInt64) throws {
        try EngineError.check(prv_sync_hold(handle, device, sequence))
    }

    /// Records that an operation reached somewhere else.
    public func acknowledge(device: UInt64, sequence: UInt64) throws {
        try EngineError.check(prv_sync_acknowledge(handle, device, sequence))
    }

    /// One consistent reading of everything above.
    public func snapshot() throws -> SyncSnapshot {
        var code: Int32 = 0
        var editing: Int32 = 0
        var transferring: Int32 = 0
        var needsUser: Int32 = 0
        var waiting: UInt64 = 0
        var nearlyFull: Int32 = 0

        try EngineError.check(prv_sync_state(handle, &code))
        try EngineError.check(prv_sync_flags(handle, &editing, &transferring, &needsUser))
        try EngineError.check(prv_sync_waiting(handle, &waiting, &nearlyFull))

        return SyncSnapshot(
            state: SyncState(code: code),
            editingIsAllowed: editing != 0,
            isTransferring: transferring != 0,
            needsTheUser: needsUser != 0,
            waiting: waiting,
            isNearlyFull: nearlyFull != 0
        )
    }
}
