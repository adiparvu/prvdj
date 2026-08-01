//! Measurement of heap activity, used to prove the render path allocated nothing.
//!
//! # Why measure rather than trust
//!
//! ADR-0002 forbids allocation on the audio thread. Rules of that consequence
//! do not survive staff turnover and deadline pressure on good intentions alone,
//! and an allocation is easy to introduce by accident: a `Vec` that grows, a
//! trait object that boxes, a format string, a closure that captures by value.
//! None of them look like allocation at the call site.
//!
//! This module makes the rule measurable. A test installs
//! [`CountingAllocator`] as the global allocator, runs a realistic render loop,
//! and asserts that the count did not move. What was a convention becomes a
//! gate.
//!
//! # Counting is per thread
//!
//! Counters are thread-local, so a test measuring the render thread is unaffected
//! by other threads allocating concurrently — which matters because the test
//! harness runs tests in parallel.
//!
//! # Cost
//!
//! One thread-local increment per allocation. This allocator is intended for
//! test binaries; production builds use the platform allocator directly.

use core::alloc::{GlobalAlloc, Layout};
use core::cell::Cell;
use core::fmt;

thread_local! {
    /// Allocations made by this thread since the counters were last reset.
    static ALLOCATIONS: Cell<u64> = const { Cell::new(0) };
    /// Deallocations made by this thread since the counters were last reset.
    static DEALLOCATIONS: Cell<u64> = const { Cell::new(0) };
}

/// Records one allocation against the current thread.
///
/// `try_with` rather than `with`: during thread teardown the thread-local may
/// already be destroyed, and this must never fail there.
fn record_allocation() {
    let _ = ALLOCATIONS.try_with(|count| count.set(count.get().wrapping_add(1)));
}

/// Records one deallocation against the current thread.
fn record_deallocation() {
    let _ = DEALLOCATIONS.try_with(|count| count.set(count.get().wrapping_add(1)));
}

/// Allocations made by the current thread since the last [`reset`].
#[must_use]
pub fn allocations() -> u64 {
    ALLOCATIONS.try_with(Cell::get).unwrap_or(0)
}

/// Deallocations made by the current thread since the last [`reset`].
#[must_use]
pub fn deallocations() -> u64 {
    DEALLOCATIONS.try_with(Cell::get).unwrap_or(0)
}

/// Resets both counters for the current thread.
pub fn reset() {
    let _ = ALLOCATIONS.try_with(|count| count.set(0));
    let _ = DEALLOCATIONS.try_with(|count| count.set(0));
}

/// A window over which heap activity is measured.
///
/// # Example
///
/// ```
/// use prv_rt::alloc_guard::AllocationScope;
///
/// let scope = AllocationScope::begin();
/// // ... work that must not allocate ...
/// assert_eq!(scope.allocations(), 0);
/// ```
#[derive(Debug, Clone, Copy)]
pub struct AllocationScope {
    allocations_at_start: u64,
    deallocations_at_start: u64,
}

impl AllocationScope {
    /// Opens a measurement window at the current counts.
    #[must_use]
    pub fn begin() -> Self {
        Self {
            allocations_at_start: allocations(),
            deallocations_at_start: deallocations(),
        }
    }

    /// Allocations since the window opened.
    #[must_use]
    pub fn allocations(&self) -> u64 {
        allocations().wrapping_sub(self.allocations_at_start)
    }

    /// Deallocations since the window opened.
    ///
    /// Counted as well as allocations because a destructor running on the audio
    /// thread is just as forbidden as an allocation, and a handoff that returns
    /// an object to a non-realtime thread for disposal is exactly the pattern
    /// that avoids it.
    #[must_use]
    pub fn deallocations(&self) -> u64 {
        deallocations().wrapping_sub(self.deallocations_at_start)
    }

    /// Returns `true` if no heap activity occurred in the window.
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.allocations() == 0 && self.deallocations() == 0
    }
}

impl fmt::Display for AllocationScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} allocations, {} deallocations",
            self.allocations(),
            self.deallocations()
        )
    }
}

/// A global allocator that counts heap activity per thread.
///
/// Wrap the platform allocator with it in a test binary:
///
/// ```ignore
/// #[global_allocator]
/// static ALLOCATOR: prv_rt::alloc_guard::CountingAllocator<std::alloc::System> =
///     prv_rt::alloc_guard::CountingAllocator::new(std::alloc::System);
/// ```
#[derive(Debug, Clone, Copy, Default)]
pub struct CountingAllocator<A> {
    inner: A,
}

impl<A> CountingAllocator<A> {
    /// Wraps an allocator.
    pub const fn new(inner: A) -> Self {
        Self { inner }
    }
}

// SAFETY: every method forwards to the wrapped allocator with the layout and
// pointer it was given, unchanged. The counting side effect touches only
// thread-local integers and allocates nothing itself, so the memory-safety
// contract of `GlobalAlloc` is exactly that of the inner allocator.
unsafe impl<A: GlobalAlloc> GlobalAlloc for CountingAllocator<A> {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        record_allocation();
        // SAFETY: `layout` is forwarded unchanged from a valid caller.
        unsafe { self.inner.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        record_deallocation();
        // SAFETY: `ptr` and `layout` are forwarded unchanged from a valid caller.
        unsafe { self.inner.dealloc(ptr, layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        record_allocation();
        // SAFETY: `layout` is forwarded unchanged from a valid caller.
        unsafe { self.inner.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // A reallocation is an allocation from the audio thread's point of
        // view: it may move memory and it may call into the allocator's slow
        // path. Counting it as one is the conservative and correct choice.
        record_allocation();
        // SAFETY: all arguments are forwarded unchanged from a valid caller.
        unsafe { self.inner.realloc(ptr, layout, new_size) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_scope_over_no_work_is_clean() {
        let scope = AllocationScope::begin();
        let sum: u64 = (0..1_000_u64).sum();
        assert_eq!(sum, 499_500);
        // Arithmetic on the stack allocates nothing, whether or not the
        // counting allocator is installed in this binary.
        assert_eq!(scope.allocations(), 0);
    }

    #[test]
    fn counters_are_readable_and_resettable() {
        reset();
        assert_eq!(allocations(), 0);
        assert_eq!(deallocations(), 0);
        let scope = AllocationScope::begin();
        assert!(scope.is_clean());
        assert_eq!(scope.to_string(), "0 allocations, 0 deallocations");
    }
}
