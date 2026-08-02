import CPRVBridge
import Foundation

/// How much the application volunteers.
public enum ExperienceMode: Sendable, CaseIterable {
    /// Explains as it goes. What somebody meets the product in.
    case guided
    /// The default.
    case standard
    /// Assumes you know, and stays out of the way.
    case professional

    var code: Int32 {
        switch self {
        case .guided: PRV_MODE_GUIDED.rawValue
        case .standard: PRV_MODE_STANDARD.rawValue
        case .professional: PRV_MODE_PROFESSIONAL.rawValue
        }
    }

    init(code: Int32) {
        self = ExperienceMode.allCases.first { $0.code == code } ?? .standard
    }
}

/// Where the user's attention is.
public enum Attention: Sendable, CaseIterable {
    /// Editing. Interruptions are fine.
    case atTheDesk
    /// Playing to a room.
    ///
    /// Anything that can wait is held. A dialogue over a set is worse than the
    /// problem it reports, almost always.
    case performing

    var code: Int32 {
        switch self {
        case .atTheDesk: PRV_ATTENTION_AT_THE_DESK.rawValue
        case .performing: PRV_ATTENTION_PERFORMING.rawValue
        }
    }
}

/// Something that was held back, and how often it happened.
public struct HeldNotice: Sendable, Equatable {
    /// The notice's ABI code. A host turns it into a sentence.
    public let notice: Int32
    /// How many times it happened while held.
    ///
    /// Six identical warnings during a set are one problem that happened six
    /// times. Showing six dialogues afterwards would be the notification doing
    /// more damage than the fault.
    public let occurrences: UInt32

    /// Whether this concerns the sound happening right now.
    public var concernsTheSound: Bool { Experience.concernsTheSound(notice) }
}

/// How the application behaves, and what it has queued to say.
///
/// # Settings and notifications are one object because they are one decision
///
/// Whether a notice is shown depends on the mode, so a host holding them apart
/// would have to reimplement that rule — and would get it wrong the first time
/// somebody switched to performing mid-set.
public final class Experience {
    private let handle: OpaquePointer

    public init() throws {
        var created: OpaquePointer?
        try EngineError.check(prv_experience_create(&created))
        guard let created else { throw EngineError.invalidHandle }
        handle = created
    }

    deinit {
        prv_experience_destroy(handle)
    }

    /// How much the application volunteers.
    public var mode: ExperienceMode {
        var value: Int32 = 0
        guard prv_experience_mode(handle, &value) == PRV_OK.rawValue else { return .standard }
        return ExperienceMode(code: value)
    }

    public func setMode(_ mode: ExperienceMode) throws {
        try EngineError.check(prv_experience_set_mode(handle, mode.code))
    }

    /// Sets where the user's attention is.
    ///
    /// Switching to ``Attention/performing`` is what stops anything that can
    /// wait from appearing over a set.
    public func setAttention(_ attention: Attention) throws {
        try EngineError.check(prv_experience_set_attention(handle, attention.code))
    }

    /// Reads a boolean setting by its ABI index.
    public func flag(_ setting: Int32) throws -> Bool {
        var value: Int32 = 0
        try EngineError.check(prv_experience_flag(handle, setting, &value))
        return value != 0
    }

    /// Sets a boolean setting by its ABI index.
    public func setFlag(_ setting: Int32, to value: Bool) throws {
        try EngineError.check(prv_experience_set_flag(handle, setting, value ? 1 : 0))
    }

    /// Raises a notice.
    ///
    /// - Returns: whether it will be shown now. `false` does not mean discarded
    ///   — a notice raised while performing is held and comes back from
    ///   ``release()``.
    @discardableResult
    public func raise(_ notice: Int32) throws -> Bool {
        var shown: Int32 = 0
        try EngineError.check(prv_experience_raise(handle, notice, &shown))
        return shown != 0
    }

    /// Whether anything is waiting to be shown.
    public var hasWaiting: Bool {
        var value: Int32 = 0
        guard prv_experience_has_waiting(handle, &value) == PRV_OK.rawValue else { return false }
        return value != 0
    }

    /// Whether a notice concerns the sound happening right now.
    ///
    /// The one class that may interrupt a performance: a performer not told the
    /// right deck is silent finds out from the room.
    ///
    /// A static because it is a fact about the notice rather than about any
    /// particular experience — and because a host asking it should not have to
    /// reach for the C module to do so. Anything above `PRVCore` that needs
    /// `CPRVBridge` is a gap in this layer, not a convenience.
    public static func concernsTheSound(_ notice: Int32) -> Bool {
        var value: Int32 = 0
        guard prv_notice_concerns_the_sound(notice, &value) == PRV_OK.rawValue else {
            return false
        }
        return value != 0
    }

    /// How many notices this build of the boundary defines.
    ///
    /// A host enumerating them needs an upper bound, and hard-coding one in
    /// every host is how they drift apart.
    public static var noticeCount: Int32 {
        var count: Int32 = 0
        while concernsTheSoundIsDefined(count) { count += 1 }
        return count
    }

    /// Whether the boundary recognises a notice code at all.
    private static func concernsTheSoundIsDefined(_ notice: Int32) -> Bool {
        var value: Int32 = 0
        return prv_notice_concerns_the_sound(notice, &value) == PRV_OK.rawValue
    }

    /// Hands over everything held back during a performance.
    ///
    /// Held, not dropped. Dropping would be simpler and would lose the one
    /// notice that mattered.
    public func release() throws -> [HeldNotice] {
        var count: UInt64 = 0
        try EngineError.check(prv_experience_release(handle, &count))
        return try (0..<count).map { index in
            var notice: Int32 = 0
            var occurrences: UInt32 = 0
            try EngineError.check(
                prv_experience_released(handle, index, &notice, &occurrences)
            )
            return HeldNotice(notice: notice, occurrences: occurrences)
        }
    }
}
