//! Delivery: what an export would be, and whether it is fit to leave.
//!
//! # What this crate is, and is not
//!
//! It is the part of exporting that can be decided *before* a single sample is
//! rendered: where the mix is going, what that place requires, whether the mix
//! meets it, what would have to change, and what went into it.
//!
//! It is not the renderer and never will be. Writing a file is input and output,
//! which ADR-0001 keeps out of the core; the platform layer does that, and asks
//! this crate what to write.
//!
//! # The report comes before the render
//!
//! Master Prompt #3C requires an export to come with a report the user can act
//! on. The useful moment for that is before the file exists: a forty-minute set
//! takes minutes to render, and telling someone afterwards that it peaks two
//! decibels too high has wasted both the time and the file.
//!
//! Everything in [`report`] is computed from the measured loudness that
//! `prv-analysis` already produces. Nothing needs the render.
//!
//! # Nothing is applied silently
//!
//! The report says what gain would bring the mix to its target and whether the
//! peaks would survive it. It does not apply either, and it deliberately
//! distinguishes "needs gain" from "would clip" — the remedies are different and
//! the user has to pick one. Master Prompt #3A puts the master chain under their
//! control, and a system that quietly normalised an export would be making a
//! mastering decision on their behalf and hiding it in a file they will hand to
//! someone else.
//!
//! # Layout
//!
//! | Module | Question it answers |
//! |--------|--------------------|
//! | [`target`] | Where is this going, and what does that place require? |
//! | [`report`] | Does the mix meet it, and if not, what would? |
//! | [`manifest`] | What went into the mix, and can it be made again? |
//!
//! # Example
//!
//! ```
//! use prv_analysis::loudness::measure;
//! use prv_export::{report, BitDepth, Compliance, DeliveryTarget, Format};
//! use prv_time::SampleRate;
//!
//! let rate = SampleRate::HZ_48000;
//! // A five-second 1 kHz tone at roughly −14 LUFS. A full-scale sine reads
//! // −3.01 LUFS, so the target is 10.99 dB below full scale.
//! let step = std::f64::consts::TAU * 1_000.0 / f64::from(rate.hz());
//! let mix: Vec<f32> = (0..240_000)
//!     .map(|n| (0.282 * (step * f64::from(n)).sin()) as f32)
//!     .collect();
//!
//! let loudness = measure(&mix, rate).unwrap();
//! let checked = report(
//!     &loudness,
//!     DeliveryTarget::Streaming,
//!     Format::Flac,
//!     BitDepth::TwentyFour,
//! );
//!
//! assert_eq!(checked.compliance(), Compliance::Ready);
//! // Reducing to 24 bits in a format that keeps what is written: dither.
//! assert!(checked.dither());
//! ```

pub mod manifest;
pub mod report;
pub mod target;

pub use manifest::{Entry, Manifest};
pub use report::{gain_to_fit_ceiling, report, Compliance, ExportReport};
pub use target::{BitDepth, DeliveryTarget, Format};
