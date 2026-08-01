//! Wait-free publication of state out of the audio thread.
//!
//! # The problem this solves
//!
//! The audio thread knows where playback is, what the meters read and what the
//! transport is doing. The interface, the waveform renderer and the diagnostics
//! view all need that information, sixty to a hundred and twenty times a second.
//!
//! Reading it under a lock would let a reader stall the audio thread. Reading it
//! field by field without one would let a reader observe a position from one
//! block and a meter value from the next — a torn value, which shows up as a
//! playhead that disagrees with the waveform.
//!
//! A triple buffer solves both. Three slots: one the writer is filling, one
//! holding the most recently published value, one the reader is looking at.
//! Publication is a single atomic swap. Neither side ever waits, and readers
//! always see a value that was complete at some instant.
//!
//! # What readers are guaranteed
//!
//! Each read returns the most recent *complete* value. Readers may miss
//! intermediate values if they read less often than the writer publishes, which
//! is exactly the desired behaviour: an interface refreshing at 120 Hz has no
//! use for the 375 blocks the audio thread produced in between, it wants the
//! latest.
//!
//! # Why `Copy`
//!
//! Restricting to `Copy` values removes destructors from the picture entirely,
//! and no destructor may run on the audio thread. Transport snapshots and meter
//! readings are plain data, so this costs nothing.

use core::cell::UnsafeCell;
use core::fmt;
use core::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// Bits of the shared state that hold the slot index.
const INDEX_MASK: usize = 0b11;

/// Bit set by the writer to signal that a newer value is available.
const FRESH_FLAG: usize = 0b100;

/// The three slots and the atomic that arbitrates between them.
///
/// Slots are separate fields rather than an array so that selecting one is a
/// match on a three-valued index rather than a bounds-checked index operation.
/// That keeps the read path free of any branch that could fail.
struct Shared<T> {
    slot_a: UnsafeCell<T>,
    slot_b: UnsafeCell<T>,
    slot_c: UnsafeCell<T>,
    /// Index of the most recently published slot, with [`FRESH_FLAG`] set if
    /// the reader has not yet taken it.
    state: AtomicUsize,
}

impl<T> Shared<T> {
    /// Returns the slot for an index.
    ///
    /// The index is always 0, 1 or 2: the three values are seeded distinct at
    /// construction and only ever exchanged between the writer, the reader and
    /// the shared state, never created anew.
    fn slot(&self, index: usize) -> &UnsafeCell<T> {
        match index {
            0 => &self.slot_a,
            1 => &self.slot_b,
            _ => &self.slot_c,
        }
    }
}

// SAFETY: the writer and the reader never hold the same index at the same time.
// The three indices 0, 1 and 2 are distributed at construction — one to the
// writer, one to the reader, one to the shared state — and every subsequent
// operation is an atomic swap that exchanges an index for the one in the shared
// state. A swap gives each side exclusive ownership of whatever index it
// receives, so no two owners can name the same slot. The `AcqRel` ordering on
// the swap publishes the writer's stores to the reader.
unsafe impl<T: Send> Send for Shared<T> {}
// SAFETY: see the argument above.
unsafe impl<T: Send> Sync for Shared<T> {}

/// The publishing half. Lives on the audio thread.
pub struct Writer<T: Copy> {
    shared: Arc<Shared<T>>,
    index: usize,
}

/// The observing half. Lives on the interface or diagnostics thread.
pub struct Reader<T: Copy> {
    shared: Arc<Shared<T>>,
    index: usize,
}

impl<T: Copy> fmt::Debug for Writer<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Writer").finish_non_exhaustive()
    }
}

impl<T: Copy> fmt::Debug for Reader<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Reader").finish_non_exhaustive()
    }
}

/// Creates a triple buffer seeded with `initial`.
///
/// All memory is allocated here, before either half reaches the audio thread.
/// Neither [`Writer::publish`] nor [`Reader::read`] allocates.
#[must_use]
pub fn channel<T: Copy + Send>(initial: T) -> (Writer<T>, Reader<T>) {
    let shared = Arc::new(Shared {
        slot_a: UnsafeCell::new(initial),
        slot_b: UnsafeCell::new(initial),
        slot_c: UnsafeCell::new(initial),
        // Slot 2 is published and not fresh; the writer takes 0, the reader 1.
        state: AtomicUsize::new(2),
    });

    (
        Writer {
            shared: Arc::clone(&shared),
            index: 0,
        },
        Reader { shared, index: 1 },
    )
}

impl<T: Copy> Writer<T> {
    /// Publishes a value.
    ///
    /// Wait-free: one store and one atomic swap, both of bounded cost. Safe to
    /// call from the audio callback, once per processed block.
    pub fn publish(&mut self, value: T) {
        // SAFETY: `self.index` is owned exclusively by this writer — it was
        // either seeded at construction or received from a swap, and the reader
        // cannot hold the same index. No other thread may touch this slot.
        unsafe {
            *self.shared.slot(self.index).get() = value;
        }

        // AcqRel: the release half publishes the store above; the acquire half
        // takes ownership of whatever index was previously in the shared state.
        let previous = self
            .shared
            .state
            .swap(self.index | FRESH_FLAG, Ordering::AcqRel);
        self.index = previous & INDEX_MASK;
    }
}

impl<T: Copy> Reader<T> {
    /// Returns the most recently published value.
    ///
    /// If nothing new has been published since the last call, returns the same
    /// value again. Wait-free and allocation-free.
    pub fn read(&mut self) -> T {
        if self.shared.state.load(Ordering::Relaxed) & FRESH_FLAG != 0 {
            // AcqRel: the acquire half makes the writer's stores visible; the
            // release half hands back the slot this reader was holding.
            let previous = self.shared.state.swap(self.index, Ordering::AcqRel);
            self.index = previous & INDEX_MASK;
        }

        // SAFETY: `self.index` is owned exclusively by this reader, by the same
        // argument as in `publish`. The value is `Copy`, so reading it out
        // leaves the slot untouched.
        unsafe { *self.shared.slot(self.index).get() }
    }

    /// Returns `true` if a value has been published since the last [`Self::read`].
    #[must_use]
    pub fn has_fresh_value(&self) -> bool {
        self.shared.state.load(Ordering::Relaxed) & FRESH_FLAG != 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;
    use std::thread;

    #[test]
    fn reading_before_any_publish_returns_the_seed() {
        let (_writer, mut reader) = channel(42_u32);
        assert_eq!(reader.read(), 42);
    }

    #[test]
    fn a_published_value_becomes_visible() {
        let (mut writer, mut reader) = channel(0_u32);
        assert!(!reader.has_fresh_value());
        writer.publish(7);
        assert!(reader.has_fresh_value());
        assert_eq!(reader.read(), 7);
        assert!(!reader.has_fresh_value());
    }

    #[test]
    fn reading_twice_without_a_publish_returns_the_same_value() {
        let (mut writer, mut reader) = channel(0_u32);
        writer.publish(3);
        assert_eq!(reader.read(), 3);
        assert_eq!(reader.read(), 3);
    }

    #[test]
    fn a_slow_reader_sees_the_latest_value_not_a_stale_one() {
        // The interface refreshes far less often than the audio thread
        // publishes. It should skip the intermediate values, not queue them.
        let (mut writer, mut reader) = channel(0_u32);
        for value in 1..=1_000 {
            writer.publish(value);
        }
        assert_eq!(reader.read(), 1_000);
    }

    #[test]
    fn the_writer_never_reuses_the_readers_slot() {
        // Exercises the index bookkeeping: after many alternating operations
        // the writer and reader must still hold distinct slots.
        let (mut writer, mut reader) = channel(0_u32);
        for value in 0..1_000_u32 {
            writer.publish(value);
            assert_eq!(reader.read(), value);
            assert_ne!(
                writer.index, reader.index,
                "writer and reader must never hold the same slot"
            );
        }
    }

    #[test]
    fn values_are_never_torn_under_concurrency() {
        // A composite value whose fields must always agree. If publication were
        // not atomic, a reader would eventually observe a mismatched pair.
        #[derive(Clone, Copy, PartialEq, Eq, Debug)]
        struct Composite {
            counter: u64,
            mirror: u64,
        }

        // The seed must itself satisfy the invariant. The reader is entitled to
        // observe it before the writer publishes anything, and a seed that
        // violated the invariant would report tearing that never happened.
        let (mut writer, mut reader) = channel(Composite {
            counter: 0,
            mirror: 1,
        });
        let stop = Arc::new(AtomicBool::new(false));
        let writer_stop = Arc::clone(&stop);

        let publisher = thread::spawn(move || {
            for counter in 0..500_000_u64 {
                writer.publish(Composite {
                    counter,
                    mirror: counter.wrapping_mul(3).wrapping_add(1),
                });
            }
            writer_stop.store(true, Ordering::Release);
        });

        let mut observations = 0_u64;
        while !stop.load(Ordering::Acquire) {
            let value = reader.read();
            assert_eq!(
                value.mirror,
                value.counter.wrapping_mul(3).wrapping_add(1),
                "observed a torn value"
            );
            observations += 1;
        }

        assert!(publisher.join().is_ok());
        assert!(
            observations > 0,
            "the reader should have observed something"
        );
    }
}
