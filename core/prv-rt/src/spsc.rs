//! A wait-free single-producer, single-consumer queue.
//!
//! This is the only sanctioned way to send work into the audio thread.
//!
//! # Why a queue rather than shared state
//!
//! A mutex around shared deck state would let a lower-priority thread hold the
//! lock while the audio thread waits for it — priority inversion, and a missed
//! buffer. ADR-0002 rejects that outright. A queue removes the shared mutable
//! state entirely: the producer writes, the consumer reads, and neither ever
//! waits for the other.
//!
//! # Who blocks
//!
//! Neither side blocks. If the queue is full the *producer* is refused, never
//! the consumer. A refused push means the control path is overloaded and is
//! recorded as a diagnostic, because it indicates a defect rather than a normal
//! condition.
//!
//! # Commands must not allocate or drop on the audio thread
//!
//! Values sent through this queue should be plain data. Anything requiring
//! allocation is prepared off-thread and handed over as a ready-made object; the
//! audio thread swaps a pointer and returns the displaced object through a
//! second queue for a non-realtime thread to drop. No destructor should ever run
//! on the audio thread.

use core::cell::UnsafeCell;
use core::fmt;
use core::mem::MaybeUninit;
use core::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// Padding to the width of a cache line, so that the producer's and consumer's
/// counters do not share one.
///
/// Without this, every producer store invalidates the cache line the consumer is
/// reading and vice versa — false sharing, which can cost an order of magnitude
/// in throughput on exactly the path that must never be slow.
#[repr(align(64))]
struct CacheAligned<T>(T);

/// Storage shared between the two halves of the queue.
struct Shared<T> {
    slots: Box<[UnsafeCell<MaybeUninit<T>>]>,
    /// `capacity - 1`, where capacity is a power of two, so that wrapping is a
    /// mask rather than a division.
    mask: usize,
    /// Next index to read. Written only by the consumer.
    head: CacheAligned<AtomicUsize>,
    /// Next index to write. Written only by the producer.
    tail: CacheAligned<AtomicUsize>,
}

// SAFETY: `Shared` hands out access to its slots under a protocol that
// guarantees the producer and consumer never touch the same slot at the same
// time:
//
//   * The producer only ever writes to index `tail & mask`, and only when
//     `tail - head < capacity` — that is, only to a slot the consumer has
//     already passed.
//   * The consumer only ever reads index `head & mask`, and only when
//     `head != tail` — that is, only from a slot the producer has finished
//     writing and published with a `Release` store.
//   * `head` is written only by the consumer and `tail` only by the producer,
//     so neither counter is ever contended for writing.
//
// The `Release`/`Acquire` pairing on the counters establishes the
// happens-before edge that makes the slot contents visible across threads.
// `T: Send` is required because values move between threads.
unsafe impl<T: Send> Send for Shared<T> {}
// SAFETY: see the argument above; concurrent access from exactly one producer
// and one consumer is sound under the stated protocol.
unsafe impl<T: Send> Sync for Shared<T> {}

impl<T> Drop for Shared<T> {
    fn drop(&mut self) {
        // Both halves are gone, so no concurrent access is possible here.
        // Anything still queued must be dropped, or its destructor never runs.
        let head = self.head.0.load(Ordering::Relaxed);
        let tail = self.tail.0.load(Ordering::Relaxed);
        let mut index = head;
        while index != tail {
            if let Some(slot) = self.slots.get(index & self.mask) {
                // SAFETY: indices in `head..tail` are exactly the initialised
                // slots, and this runs with exclusive access.
                unsafe {
                    (*slot.get()).assume_init_drop();
                }
            }
            index = index.wrapping_add(1);
        }
    }
}

/// The sending half. Lives on non-realtime threads.
pub struct Producer<T> {
    shared: Arc<Shared<T>>,
}

/// The receiving half. Lives on the audio thread.
pub struct Consumer<T> {
    shared: Arc<Shared<T>>,
}

impl<T> fmt::Debug for Producer<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Producer")
            .field("capacity", &self.capacity())
            .field("len", &self.len())
            .finish()
    }
}

impl<T> fmt::Debug for Consumer<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Consumer")
            .field("capacity", &self.capacity())
            .field("len", &self.len())
            .finish()
    }
}

/// Creates a queue with at least `capacity` slots.
///
/// The capacity is rounded up to the next power of two so that index wrapping is
/// a mask rather than a division — a division on the audio thread is a
/// measurable cost for no benefit.
///
/// All memory is allocated here, before either half can reach the audio thread.
/// Nothing in [`Producer::push`] or [`Consumer::pop`] allocates.
///
/// A capacity of zero is rounded up to one, so the queue is always usable.
#[must_use]
pub fn channel<T: Send>(capacity: usize) -> (Producer<T>, Consumer<T>) {
    let capacity = capacity.max(1).next_power_of_two();
    let mut slots = Vec::with_capacity(capacity);
    slots.resize_with(capacity, || UnsafeCell::new(MaybeUninit::uninit()));

    let shared = Arc::new(Shared {
        slots: slots.into_boxed_slice(),
        mask: capacity - 1,
        head: CacheAligned(AtomicUsize::new(0)),
        tail: CacheAligned(AtomicUsize::new(0)),
    });

    (
        Producer {
            shared: Arc::clone(&shared),
        },
        Consumer { shared },
    )
}

impl<T> Producer<T> {
    /// Number of slots in the queue.
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.shared.mask + 1
    }

    /// Number of values currently queued.
    ///
    /// A momentary estimate: the consumer may drain concurrently.
    #[must_use]
    pub fn len(&self) -> usize {
        let tail = self.shared.tail.0.load(Ordering::Relaxed);
        let head = self.shared.head.0.load(Ordering::Acquire);
        tail.wrapping_sub(head)
    }

    /// Returns `true` if no values are queued.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns `true` if the queue cannot accept another value.
    #[must_use]
    pub fn is_full(&self) -> bool {
        self.len() >= self.capacity()
    }

    /// Enqueues a value.
    ///
    /// # Errors
    ///
    /// Returns the value unchanged if the queue is full. A full queue means the
    /// audio thread is not draining commands as fast as they are produced, which
    /// is a defect to be diagnosed rather than a condition to be retried in a
    /// tight loop.
    pub fn push(&mut self, value: T) -> Result<(), T> {
        let tail = self.shared.tail.0.load(Ordering::Relaxed);
        let head = self.shared.head.0.load(Ordering::Acquire);

        if tail.wrapping_sub(head) >= self.capacity() {
            return Err(value);
        }

        let Some(slot) = self.shared.slots.get(tail & self.shared.mask) else {
            // Unreachable: the index is masked into range. Returning the value
            // keeps this function total rather than introducing a panic path.
            return Err(value);
        };

        // SAFETY: `tail - head < capacity` establishes that the consumer has
        // already passed this slot, so the producer has exclusive access to it.
        unsafe {
            (*slot.get()).write(value);
        }

        // Release: publishes the slot contents written above to the consumer's
        // matching Acquire load of `tail`.
        self.shared
            .tail
            .0
            .store(tail.wrapping_add(1), Ordering::Release);
        Ok(())
    }
}

impl<T> Consumer<T> {
    /// Number of slots in the queue.
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.shared.mask + 1
    }

    /// Number of values currently queued.
    #[must_use]
    pub fn len(&self) -> usize {
        let tail = self.shared.tail.0.load(Ordering::Acquire);
        let head = self.shared.head.0.load(Ordering::Relaxed);
        tail.wrapping_sub(head)
    }

    /// Returns `true` if no values are queued.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Dequeues a value, or returns `None` if the queue is empty.
    ///
    /// Wait-free, allocation-free and panic-free: safe to call from the audio
    /// callback.
    pub fn pop(&mut self) -> Option<T> {
        let head = self.shared.head.0.load(Ordering::Relaxed);
        // Acquire: pairs with the producer's Release store of `tail`, making
        // the slot contents visible.
        let tail = self.shared.tail.0.load(Ordering::Acquire);

        if head == tail {
            return None;
        }

        let slot = self.shared.slots.get(head & self.shared.mask)?;

        // SAFETY: `head != tail` establishes that the producer has finished
        // writing this slot and published it, so the value is initialised and
        // the consumer has exclusive access to it. It is moved out exactly once,
        // because `head` advances immediately below and only this thread writes
        // `head`.
        let value = unsafe { (*slot.get()).assume_init_read() };

        // Release: publishes the slot as free to the producer's Acquire load.
        self.shared
            .head
            .0
            .store(head.wrapping_add(1), Ordering::Release);
        Some(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;
    use std::thread;

    #[test]
    fn capacity_is_rounded_up_to_a_power_of_two() {
        let (producer, consumer) = channel::<u32>(5);
        assert_eq!(producer.capacity(), 8);
        assert_eq!(consumer.capacity(), 8);
    }

    #[test]
    fn zero_capacity_is_usable() {
        let (mut producer, mut consumer) = channel::<u32>(0);
        assert_eq!(producer.capacity(), 1);
        assert_eq!(producer.push(7), Ok(()));
        assert_eq!(producer.push(8), Err(8));
        assert_eq!(consumer.pop(), Some(7));
        assert_eq!(consumer.pop(), None);
    }

    #[test]
    fn values_come_out_in_order() {
        let (mut producer, mut consumer) = channel::<u32>(8);
        for value in 0..8 {
            assert_eq!(producer.push(value), Ok(()));
        }
        for expected in 0..8 {
            assert_eq!(consumer.pop(), Some(expected));
        }
        assert_eq!(consumer.pop(), None);
    }

    #[test]
    fn a_full_queue_refuses_the_producer_and_returns_the_value() {
        let (mut producer, mut consumer) = channel::<u32>(2);
        assert_eq!(producer.push(1), Ok(()));
        assert_eq!(producer.push(2), Ok(()));
        assert!(producer.is_full());
        assert_eq!(producer.push(3), Err(3));

        // Draining one makes room for exactly one more.
        assert_eq!(consumer.pop(), Some(1));
        assert_eq!(producer.push(3), Ok(()));
    }

    #[test]
    fn indices_wrap_without_corruption() {
        // Push and pop far more values than the queue holds, so the internal
        // counters wrap around the ring many times.
        let (mut producer, mut consumer) = channel::<u64>(4);
        for round in 0..10_000_u64 {
            assert_eq!(producer.push(round), Ok(()));
            assert_eq!(consumer.pop(), Some(round));
        }
        assert!(consumer.is_empty());
    }

    #[test]
    fn queued_values_are_dropped_when_the_queue_is_dropped() {
        use std::sync::atomic::AtomicUsize;
        static DROPS: AtomicUsize = AtomicUsize::new(0);

        struct CountsDrops;
        impl Drop for CountsDrops {
            fn drop(&mut self) {
                DROPS.fetch_add(1, Ordering::SeqCst);
            }
        }

        {
            let (mut producer, _consumer) = channel::<CountsDrops>(4);
            for _ in 0..3 {
                assert!(producer.push(CountsDrops).is_ok());
            }
        }

        assert_eq!(
            DROPS.load(Ordering::SeqCst),
            3,
            "values left in the queue must still be dropped"
        );
    }

    #[test]
    fn concurrent_producer_and_consumer_transfer_every_value_in_order() {
        // The real usage pattern: a control thread producing while the audio
        // thread consumes. Every value must arrive exactly once and in order.
        const COUNT: u64 = 200_000;
        let (mut producer, mut consumer) = channel::<u64>(64);
        let done = Arc::new(AtomicBool::new(false));
        let producer_done = Arc::clone(&done);

        let sender = thread::spawn(move || {
            let mut value = 0_u64;
            while value < COUNT {
                if producer.push(value).is_ok() {
                    value += 1;
                } else {
                    std::hint::spin_loop();
                }
            }
            producer_done.store(true, Ordering::Release);
        });

        let mut expected = 0_u64;
        while expected < COUNT {
            if let Some(value) = consumer.pop() {
                assert_eq!(value, expected, "values must arrive in order");
                expected += 1;
            } else {
                std::hint::spin_loop();
            }
        }

        assert!(sender.join().is_ok());
        assert!(done.load(Ordering::Acquire));
        assert_eq!(expected, COUNT);
    }
}
