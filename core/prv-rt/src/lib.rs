//! Realtime-safe primitives for the audio thread.
//!
//! # The contract this crate exists to uphold
//!
//! ADR-0002 states the audio thread's rules without exception: no heap
//! allocation, no locking, no waiting, no system calls, no panics, and bounded
//! execution time. Master Prompt #18 states them first; Module Specification
//! #002 repeats them; Master Prompt #22 requires that the transport clock
//! survive whatever else in the process has failed.
//!
//! A single allocation inside a 128-frame callback at 48 kHz — a window of 2.7
//! milliseconds — is an audible dropout in front of an audience. Rules of that
//! consequence cannot rest on reviewer vigilance, so this crate provides the
//! mechanisms that make obeying them the path of least resistance, and the
//! measurement that proves they were obeyed.
//!
//! # What is here
//!
//! - [`spsc`] — a wait-free single-producer, single-consumer queue. The only
//!   sanctioned way for other threads to send work *into* the audio thread.
//! - [`triple_buffer`] — wait-free publication of state *out of* the audio
//!   thread. Readers always observe a complete, self-consistent value and never
//!   block the writer.
//! - [`smoothing`] — parameter ramps. No continuous parameter is ever applied as
//!   a step, because steps are clicks.
//! - [`buffer`] — planar audio buffers whose memory is allocated once, before
//!   they become reachable from the callback.
//! - [`alloc_guard`] — an allocation-counting allocator used by tests to prove
//!   the render path allocated nothing.
//!
//! # Unsafe code
//!
//! The workspace denies `unsafe_code`. This crate is the deliberate exception,
//! because wait-free data structures cannot be expressed in safe Rust: they
//! require shared mutable access whose discipline is enforced by protocol rather
//! than by the borrow checker.
//!
//! The exception is kept narrow and auditable. Unsafe appears in exactly three
//! places — [`spsc`], [`triple_buffer`] and the allocator wrapper in
//! [`alloc_guard`] — every block carries a written safety argument, and the
//! invariants those arguments depend on are stated on the types themselves. No
//! other crate in the core is permitted `unsafe`.
//!
//! # Verification
//!
//! `tests/realtime_contract.rs` runs a complete simulated render loop — command
//! intake, clock advance, parameter smoothing, buffer fill, snapshot
//! publication — and asserts that it performs exactly zero heap allocations.
//! That test is a required gate on every change.

#![allow(
    unsafe_code,
    reason = "wait-free structures require shared mutable access; see the module documentation above. Confined to `spsc` and `triple_buffer`, each with a written safety argument."
)]

pub mod alloc_guard;
pub mod buffer;
pub mod smoothing;
pub mod spsc;
pub mod triple_buffer;

pub use buffer::{AudioBuffer, BufferError};
pub use smoothing::LinearSmoother;
pub use spsc::{Consumer, Producer};
pub use triple_buffer::{Reader, Writer};
