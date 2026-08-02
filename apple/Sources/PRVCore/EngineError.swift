import CPRVBridge
import Foundation

/// What a call into the core can go wrong with.
///
/// # Why this is an enum and the C side is an integer
///
/// The boundary returns `int32_t` because that is what survives a C ABI. Nothing
/// above this file should ever see one. A Swift caller that has to remember
/// which number means "the deck has nothing loaded" is a caller that will
/// eventually compare against the wrong one, and the compiler will not help.
///
/// So the integer is turned into a value with a name exactly once, here, and
/// every call above throws.
public enum EngineError: Error, Equatable, Sendable {
    /// A required pointer was null.
    case nullPointer
    /// An argument was outside the range the call accepts.
    case invalidArgument
    /// The handle does not refer to a live object.
    case invalidHandle
    /// The object is not in a state where this call means anything.
    ///
    /// Pressing play on a deck with nothing loaded, for instance. Not a failure
    /// to act on so much as an answer.
    case invalidState
    /// A buffer was too small.
    case bufferTooSmall
    /// The core refused the operation.
    case refused
    /// The core panicked.
    ///
    /// A defect report, not a condition. The engine is still alive and the
    /// handle is still valid, but what the operation did is unknown. Report it;
    /// do not retry it.
    case panicked
    /// A status this build of the wrapper does not know.
    ///
    /// Reached when the loaded library is newer than this wrapper — which the
    /// version check is meant to prevent, and which is worth representing
    /// anyway. The number is carried so a bug report can name it.
    case unrecognised(code: Int32)

    /// Turns a status code into an error, or `nil` when the call succeeded.
    ///
    /// Deliberately returns an optional rather than throwing: the mapping and
    /// the throwing are different jobs, and keeping them apart is what lets this
    /// be tested without a live engine.
    static func from(code: Int32) -> EngineError? {
        switch code {
        case PRV_OK.rawValue: nil
        case PRV_NULL_POINTER.rawValue: .nullPointer
        case PRV_INVALID_ARGUMENT.rawValue: .invalidArgument
        case PRV_INVALID_HANDLE.rawValue: .invalidHandle
        case PRV_INVALID_STATE.rawValue: .invalidState
        case PRV_BUFFER_TOO_SMALL.rawValue: .bufferTooSmall
        case PRV_REFUSED.rawValue: .refused
        case PRV_PANICKED.rawValue: .panicked
        default: .unrecognised(code: code)
        }
    }

    /// Throws if `code` is not success.
    static func check(_ code: Int32) throws {
        if let error = from(code: code) {
            throw error
        }
    }

    /// The core's own description of this status.
    ///
    /// Read from the library rather than written here, so the explanation a user
    /// sees comes from the layer that decided it rather than from a translation
    /// that can drift.
    public var explanation: String {
        guard let pointer = prv_status_message(code) else {
            return "the core gave no explanation"
        }
        return String(cString: pointer)
    }

    /// The integer the boundary uses for this error.
    public var code: Int32 {
        switch self {
        case .nullPointer: PRV_NULL_POINTER.rawValue
        case .invalidArgument: PRV_INVALID_ARGUMENT.rawValue
        case .invalidHandle: PRV_INVALID_HANDLE.rawValue
        case .invalidState: PRV_INVALID_STATE.rawValue
        case .bufferTooSmall: PRV_BUFFER_TOO_SMALL.rawValue
        case .refused: PRV_REFUSED.rawValue
        case .panicked: PRV_PANICKED.rawValue
        case .unrecognised(let code): code
        }
    }
}

extension EngineError: CustomStringConvertible {
    public var description: String { explanation }
}
