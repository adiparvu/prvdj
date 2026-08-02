//! What a call can go wrong with, and how it says so.
//!
//! # Why an integer and a static string
//!
//! Every other error type in the core is an enum with fields, because a caller
//! in the same language can match on it. Across a C ABI nothing can be matched
//! on, so the boundary needs a representation that survives being an integer.
//!
//! The alternative — returning an allocated message the caller frees — puts an
//! allocation on every failure path, including the ones inside an audio
//! callback, and it invents an ownership rule that every host language then has
//! to be told about correctly. A static string has neither problem:
//! [`Status::message`] hands back a pointer into the binary's own read-only
//! data, which is valid for as long as the library is loaded and belongs to
//! nobody.
//!
//! # Zero is success, and it is the only success
//!
//! Not "zero or positive". A function that returns a count *and* an error
//! through the same integer is a function whose callers eventually forget the
//! sign, so counts are written through out-parameters and the return value is
//! always and only a status.

use core::fmt;

/// The result of a call across the boundary.
///
/// Represented as `int32_t`. The numbers are part of the ABI: a host compiled
/// against version 1 must keep working against version 1, so a variant is never
/// renumbered and a retired variant's number is never reused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(i32)]
#[non_exhaustive]
pub enum Status {
    /// The call did what was asked.
    Ok = 0,

    /// A required pointer was null.
    ///
    /// Separate from [`Self::InvalidArgument`] because it is the one mistake a
    /// host makes by accident rather than by misunderstanding, and naming it
    /// exactly is the difference between a five-minute fix and an afternoon.
    NullPointer = 1,

    /// An argument was outside the range the call accepts.
    InvalidArgument = 2,

    /// The handle does not refer to a live object.
    InvalidHandle = 3,

    /// The object is not in a state where this call means anything.
    ///
    /// Pressing play on a deck with nothing loaded, for instance. The core
    /// returns these rather than ignoring them, and so does the boundary.
    InvalidState = 4,

    /// A caller-provided buffer was too small.
    ///
    /// The required size is written to the out-parameter, so the caller can
    /// allocate once and call again rather than guessing upward.
    BufferTooSmall = 5,

    /// The core refused the operation and said why in its own terms.
    ///
    /// The detail is available through the call's own out-parameters; this is
    /// the coarse "it did not happen" that a host switches on.
    Refused = 6,

    /// A panic was caught at the boundary.
    ///
    /// # This is a defect report, not an error condition
    ///
    /// Letting a Rust panic unwind into C is undefined behaviour, so every entry
    /// point catches. Receiving this means the core has a bug; the library is
    /// still loaded and the handle is still valid, but the operation's effect is
    /// unknown and the host should report it rather than retry.
    Panicked = 7,
}

impl Status {
    /// A stable, human-readable description.
    ///
    /// The returned pointer is into static memory: it is valid for the lifetime
    /// of the loaded library, is never freed, and must not be freed by the
    /// caller. Every string is NUL-terminated.
    #[must_use]
    pub const fn message(self) -> &'static str {
        match self {
            Self::Ok => "ok\0",
            Self::NullPointer => "a required pointer was null\0",
            Self::InvalidArgument => "an argument was out of range\0",
            Self::InvalidHandle => "the handle does not refer to a live object\0",
            Self::InvalidState => "the object is not in a state where this call applies\0",
            Self::BufferTooSmall => "the provided buffer is too small\0",
            Self::Refused => "the core refused the operation\0",
            Self::Panicked => "the core panicked; this is a defect, not a condition\0",
        }
    }

    /// Whether the call did what was asked.
    #[must_use]
    pub const fn is_ok(self) -> bool {
        matches!(self, Self::Ok)
    }

    /// The integer a host sees.
    #[must_use]
    pub const fn code(self) -> i32 {
        self as i32
    }

    /// The status a code names, if any.
    ///
    /// Used by the header generator and by tests. A host never needs this —
    /// hosts switch on the integer.
    #[must_use]
    pub const fn from_code(code: i32) -> Option<Self> {
        match code {
            0 => Some(Self::Ok),
            1 => Some(Self::NullPointer),
            2 => Some(Self::InvalidArgument),
            3 => Some(Self::InvalidHandle),
            4 => Some(Self::InvalidState),
            5 => Some(Self::BufferTooSmall),
            6 => Some(Self::Refused),
            7 => Some(Self::Panicked),
            _ => None,
        }
    }

    /// Every status, in code order.
    ///
    /// The generator walks this to emit the C enum, so a variant added here and
    /// nowhere else still reaches the header.
    pub const ALL: &'static [Self] = &[
        Self::Ok,
        Self::NullPointer,
        Self::InvalidArgument,
        Self::InvalidHandle,
        Self::InvalidState,
        Self::BufferTooSmall,
        Self::Refused,
        Self::Panicked,
    ];

    /// The C spelling of this variant.
    #[must_use]
    pub const fn c_name(self) -> &'static str {
        match self {
            Self::Ok => "PRV_OK",
            Self::NullPointer => "PRV_NULL_POINTER",
            Self::InvalidArgument => "PRV_INVALID_ARGUMENT",
            Self::InvalidHandle => "PRV_INVALID_HANDLE",
            Self::InvalidState => "PRV_INVALID_STATE",
            Self::BufferTooSmall => "PRV_BUFFER_TOO_SMALL",
            Self::Refused => "PRV_REFUSED",
            Self::Panicked => "PRV_PANICKED",
        }
    }
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // The stored message carries its NUL for C's benefit; Rust's `Display`
        // must not include it.
        f.write_str(self.message().trim_end_matches('\0'))
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::panic,
        reason = "a test that cannot build its own fixture should fail loudly"
    )]

    use super::*;

    #[test]
    fn every_message_is_nul_terminated() {
        // The whole point of the static-string design. One missing terminator is
        // a read past the end of the binary's data section in every host that
        // ever prints an error.
        for status in Status::ALL {
            assert!(
                status.message().ends_with('\0'),
                "{status:?} has no terminator"
            );
        }
    }

    #[test]
    fn no_message_has_an_interior_nul() {
        // C stops at the first one, so an interior NUL silently truncates the
        // message and nothing ever reports it.
        for status in Status::ALL {
            let body = status.message().trim_end_matches('\0');
            assert!(
                !body.contains('\0'),
                "{status:?} would be truncated when read as a C string"
            );
        }
    }

    #[test]
    fn codes_are_dense_and_in_order() {
        // Dense because the header emits them without explicit values, and any
        // gap would silently shift every variant after it.
        for (index, status) in Status::ALL.iter().enumerate() {
            let expected = i32::try_from(index).expect("the table is small");
            assert_eq!(status.code(), expected, "{status:?} is out of place");
        }
    }

    #[test]
    fn a_code_round_trips_through_its_status() {
        // The property the ABI rests on: the number a host stores is the number
        // that comes back meaning the same thing.
        for status in Status::ALL {
            assert_eq!(Status::from_code(status.code()), Some(*status));
        }
        assert_eq!(Status::from_code(-1), None);
        assert_eq!(
            Status::from_code(i32::try_from(Status::ALL.len()).unwrap_or(i32::MAX)),
            None,
            "a code past the end of the table was accepted"
        );
    }

    #[test]
    fn only_zero_is_success() {
        for status in Status::ALL {
            assert_eq!(status.is_ok(), status.code() == 0);
        }
    }

    #[test]
    fn c_names_are_distinct_and_prefixed() {
        // A host reads these; a collision would compile and mean the wrong
        // thing.
        let mut seen = std::collections::BTreeSet::new();
        for status in Status::ALL {
            assert!(
                status.c_name().starts_with("PRV_"),
                "{status:?} is not namespaced"
            );
            assert!(
                seen.insert(status.c_name()),
                "{status:?} has a duplicate name"
            );
        }
    }
}
