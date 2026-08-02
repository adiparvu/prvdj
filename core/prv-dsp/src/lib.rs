//! The realtime signal path.
//!
//! # What is here
//!
//! - [`Processor`] — the contract every element of the signal path implements.
//! - [`Chain`] — a series of processors, run in order, allocation-free.
//! - [`Gain`] — level, smoothed.
//! - [`ThreeBandEq`] — the DJ equaliser, with a true kill.
//! - [`DjFilter`] — the single-knob low-pass/high-pass sweep.
//!
//! # The division that makes this safe
//!
//! Every processor has two phases, and the split is the whole design.
//!
//! [`Processor::prepare`] runs off the audio thread. It may allocate, compute
//! coefficients, size delay lines — anything. It is called before the processor
//! is reachable from the callback.
//!
//! [`Processor::process`] runs *on* the audio thread and obeys ADR-0002: no
//! allocation, no locking, no syscalls, no panics, and work bounded by frame
//! count alone. A processor that needs to allocate in `process` is a processor
//! that has been designed wrongly.
//!
//! # Numerical precision
//!
//! Filter coefficients and state are `f64`, samples are `f32`.
//!
//! This is not caution for its own sake. A biquad with a corner at 200 Hz and a
//! sample rate of 48 kHz has poles very close to the unit circle; in `f32` the
//! coefficient quantisation moves the corner audibly and the state accumulates
//! error that shows as a raised noise floor in the bass. The extra cost buys
//! correctness in exactly the band a DJ equaliser is used on hardest.
//!
//! # Denormals
//!
//! Filter state decaying toward zero passes through denormal numbers, where some
//! processors slow down by an order of magnitude. On the audio thread that is a
//! dropout. Every filter flushes state below a threshold, so the decay ends at
//! zero rather than grinding through the denormal range.

mod biquad;
mod chain;
mod eq;
mod filter;
mod gain;
mod limiter;
mod meter;
mod processor;
mod resample;
mod stretch;

pub use biquad::{Biquad, BiquadCoefficients};
pub use chain::Chain;
pub use eq::{ThreeBandEq, DEFAULT_HIGH_CROSSOVER_HZ, DEFAULT_LOW_CROSSOVER_HZ};
pub use filter::DjFilter;
pub use gain::Gain;
pub use limiter::Limiter;
pub use meter::{k_weighting, LoudnessMeter};
pub use processor::{PrepareConfig, ProcessContext, Processor};
pub use resample::{PitchShift, Resampler};
pub use stretch::TimeStretch;
