//! Audio analysis: what the system knows about a track before it plays it.
//!
//! # What this crate produces
//!
//! Master Prompt #20 requires the analysis engine to determine tempo, beat
//! positions, downbeats, key, structure, energy and loudness, and to report a
//! confidence with each. ADR-0006 makes that output the input to every musical
//! decision the product takes: the planner searches over analysed facts, and
//! the explanation the user reads is a rendering of those facts rather than a
//! story about them.
//!
//! That places an unusual demand on this crate. It is not enough to be right
//! often. Being *wrong with a high confidence* is worse than being wrong,
//! because the confidence is what the user reads before deciding whether to
//! check. Every stage here reports what it measured alongside how well the
//! measurement was determined, and the two are computed separately.
//!
//! # Deterministic, on device, dependency free
//!
//! ADR-0001 keeps the core free of runtime dependencies, and ADR-0006 requires
//! the same inputs to produce the same output on every platform. Both rule out
//! the usual transform libraries, several of which dispatch on runtime CPU
//! features and so differ in the last bits between machines. For a spectrum
//! that is inaudible; for a *decision derived from* a spectrum it can change
//! the answer, and a preview that disagrees with an export is a defect a user
//! will find. So the transform is written here, and verified against the
//! definition it is an optimisation of.
//!
//! # Off the audio thread, deliberately
//!
//! Nothing in this crate obeys ADR-0002. Analysis allocates, runs for seconds
//! and produces its result once. The realtime contract governs `prv-dsp`, which
//! is where audio is rendered; forcing analysis into the same shape would make
//! it worse at its job without making playback any safer.
//!
//! # Layout
//!
//! | Module | Question it answers |
//! |--------|--------------------|
//! | [`fft`] | What frequencies are present? |
//! | [`window`] | How is a block cut out without smearing the answer? |
//! | [`spectrum`] | How does a whole track become a stream of spectra? |
//! | [`onset`] | Where does something start? |
//! | [`tempo`] | How fast, and how sure? |
//! | [`beats`] | Where exactly is each beat, and which one begins the bar? |
//! | [`confidence`] | How is "how sure" said in one way everywhere? |

mod confidence;
mod error;
mod fft;
mod num;
mod onset;
mod spectrum;
mod window;

#[cfg(test)]
mod testing;

pub mod beats;
pub mod tempo;

pub use confidence::{Confidence, ConfidenceLabel};
pub use error::AnalysisError;
pub use fft::{Complex, Fft, RealFft};
pub use onset::{NoveltyCurve, Onset};
pub use spectrum::{
    SpectrumFrame, Stft, RHYTHM_HOP_SIZE, RHYTHM_WINDOW_SIZE, TONAL_HOP_SIZE, TONAL_WINDOW_SIZE,
};
pub use window::{Window, WindowShape};
