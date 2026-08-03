//! The wrapper every entry point goes through.
//!
//! # A panic must not reach C
//!
//! Unwinding out of an `extern "C"` function is undefined behaviour. Not
//! "usually fine", not "aborts" — undefined, and on some targets it corrupts the
//! stack of a host that has no idea Rust is involved. Every entry point in this
//! crate therefore runs its body inside [`catch_unwind`](std::panic::catch_unwind)
//! and turns a panic into [`Status::Panicked`].
//!
//! This is a backstop, not a strategy. The core forbids the panicking
//! constructs outright (`unwrap_used`, `expect_used`, `panic`, `indexing_slicing`
//! are all denied at the workspace root), so reaching the catch means a defect
//! got past those lints — arithmetic overflow in a debug build, most likely.
//! Converting undefined behaviour into a reportable status is worth doing even
//! for a case that should not arise, because the case that should not arise is
//! exactly the one nobody has a plan for.
//!
//! # Why the realtime path uses it too
//!
//! ADR-0002 forbids panicking on the audio thread, so in principle the guard is
//! dead weight there. In practice a panic on the audio thread with no guard is
//! undefined behaviour inside `CoreAudio`'s render thread, which is the single
//! worst place in the system for it. `catch_unwind` costs nothing when nothing
//! panics — it is zero-cost on the happy path on every supported target — so the
//! guard is not an exception the realtime path is allowed to skip.

use std::panic::{catch_unwind, AssertUnwindSafe};

use crate::status::Status;

/// Runs `body`, converting a panic into [`Status::Panicked`].
///
/// # Unwind safety
///
/// [`AssertUnwindSafe`] is used deliberately. The standard `UnwindSafe` bound
/// exists to stop a caller observing a value left half-updated by a panic, and
/// it cannot be satisfied by anything holding a `&mut`, which is every call that
/// does work here.
///
/// The assertion is sound because of what the boundary does with the result: a
/// panic returns [`Status::Panicked`], which is documented as "the effect of
/// this operation is unknown". A host that receives it is told, in the status's
/// own documentation, not to retry and to report the defect. Nothing here
/// pretends the object is in a good state — it says out loud that it might not
/// be, which is the honest version of what `UnwindSafe` is trying to prevent.
pub(crate) fn guarded<F>(body: F) -> Status
where
    F: FnOnce() -> Status,
{
    catch_unwind(AssertUnwindSafe(body)).unwrap_or(Status::Panicked)
}

/// Turns a raw pointer into a shared reference, or reports why it could not.
///
/// # Safety
///
/// `pointer` must either be null or point to a live, initialised `T` that stays
/// valid and unaliased for `'a`. The caller is the entry point, and the promise
/// comes from the header's documented contract.
pub(crate) unsafe fn as_ref<'a, T>(pointer: *const T) -> Result<&'a T, Status> {
    if pointer.is_null() {
        return Err(Status::NullPointer);
    }
    // SAFETY: non-null was checked above; validity and lifetime are the entry
    // point's documented precondition, stated in the header for every parameter.
    Ok(unsafe { &*pointer })
}

/// Turns a raw pointer into an exclusive reference, or reports why it could not.
///
/// # Safety
///
/// As [`as_ref`], and additionally `pointer` must not be aliased for `'a`.
pub(crate) unsafe fn as_mut<'a, T>(pointer: *mut T) -> Result<&'a mut T, Status> {
    if pointer.is_null() {
        return Err(Status::NullPointer);
    }
    // SAFETY: non-null was checked above; exclusivity and validity are the entry
    // point's documented precondition.
    Ok(unsafe { &mut *pointer })
}

/// How many bytes a declared capacity actually is, saturating rather than
/// wrapping.
///
/// A capacity that will not fit a `usize` cannot describe memory this process
/// can address, so treating it as "as much as there is" is both safe and the
/// only interpretation that does not silently truncate to something small.
pub(crate) fn buffer_capacity(capacity: u64) -> usize {
    usize::try_from(capacity).unwrap_or(usize::MAX)
}

/// Turns a caller's pointer and length into a slice to write into.
///
/// A zero capacity is how a caller asks "how big is this" without providing
/// anywhere to put it, so an empty slice is the honest representation rather
/// than a dangling one — and it is the only case in which a null pointer is
/// accepted.
///
/// # Safety
///
/// `pointer` must be writable for `capacity` bytes, or null when `capacity` is
/// zero. The slice must not be aliased for `'a`.
pub(crate) unsafe fn writable<'a>(pointer: *mut u8, capacity: u64) -> Result<&'a mut [u8], Status> {
    let capacity = buffer_capacity(capacity);
    if capacity == 0 {
        return Ok(&mut []);
    }
    if pointer.is_null() {
        return Err(Status::NullPointer);
    }
    // SAFETY: non-null was checked above; writability for `capacity` bytes is the
    // entry point's documented precondition.
    Ok(unsafe { core::slice::from_raw_parts_mut(pointer, capacity) })
}

/// Turns a caller's pointer and length into a slice to read from.
///
/// # Safety
///
/// `pointer` must be readable for `len` bytes, or null when `len` is zero, and
/// must not be written by anything else for `'a`.
pub(crate) unsafe fn readable<'a>(pointer: *const u8, len: u64) -> Result<&'a [u8], Status> {
    let len = buffer_capacity(len);
    if len == 0 {
        return Ok(&[]);
    }
    if pointer.is_null() {
        return Err(Status::NullPointer);
    }
    // SAFETY: non-null was checked above; readability for `len` bytes is the
    // entry point's documented precondition.
    Ok(unsafe { core::slice::from_raw_parts(pointer, len) })
}

/// Runs `body` with the error arm folded into the status.
///
/// Saves every entry point from writing the same `match` around a `?`-chain,
/// which is the kind of repetition that eventually gets one arm wrong.
pub(crate) fn guarded_try<F>(body: F) -> Status
where
    F: FnOnce() -> Result<(), Status>,
{
    guarded(|| match body() {
        Ok(()) => Status::Ok,
        Err(status) => status,
    })
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::panic,
        clippy::unwrap_used,
        clippy::indexing_slicing,
        reason = "this module's whole subject is what happens when something panics, \
                  so it has to be able to write the code that panics"
    )]

    use super::*;

    #[test]
    fn a_panic_becomes_a_status_rather_than_undefined_behaviour() {
        // The reason the module exists. Without the guard this is UB inside the
        // host's stack frame.
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let status = guarded(|| panic!("a defect that got past the lints"));
        std::panic::set_hook(previous);

        assert_eq!(status, Status::Panicked);
    }

    #[test]
    fn a_normal_return_passes_through_untouched() {
        assert_eq!(guarded(|| Status::Ok), Status::Ok);
        assert_eq!(guarded(|| Status::InvalidState), Status::InvalidState);
    }

    #[test]
    fn the_error_arm_of_a_try_body_becomes_its_status() {
        assert_eq!(guarded_try(|| Ok(())), Status::Ok);
        assert_eq!(
            guarded_try(|| Err(Status::BufferTooSmall)),
            Status::BufferTooSmall
        );
    }

    #[test]
    fn a_panic_inside_a_try_body_is_caught_too() {
        // The `?` chain is where the real work happens, so the guard has to wrap
        // it rather than sit beside it.
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let status = guarded_try(|| {
            let empty: Vec<i32> = Vec::new();
            let _ = empty[4];
            Ok(())
        });
        std::panic::set_hook(previous);

        assert_eq!(status, Status::Panicked);
    }

    #[test]
    fn a_null_pointer_is_reported_rather_than_dereferenced() {
        // SAFETY: null is one of the two cases `as_ref` accepts, and it is the
        // case under test.
        let result = unsafe { as_ref::<u32>(core::ptr::null()) };
        assert_eq!(result.err(), Some(Status::NullPointer));

        // SAFETY: as above, for the exclusive form.
        let result = unsafe { as_mut::<u32>(core::ptr::null_mut()) };
        assert_eq!(result.err(), Some(Status::NullPointer));
    }

    #[test]
    fn a_live_pointer_comes_back_as_the_value_it_points_at() {
        let mut value = 42_u32;

        // SAFETY: `value` is a live local that outlives the borrow.
        let seen = unsafe { as_ref(&raw const value) }.expect("not null");
        assert_eq!(*seen, 42);

        // SAFETY: as above, and the shared borrow ended on the line before.
        let seen = unsafe { as_mut(&raw mut value) }.expect("not null");
        *seen = 7;
        assert_eq!(value, 7);
    }
}
