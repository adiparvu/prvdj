//! One confidence scale, defined once.
//!
//! # Why this is a type and not an `f32`
//!
//! Master Prompt #25 gives the user five confidence labels. ADR-0006 adds the
//! constraint that makes them worth having: the mapping from a measurement to a
//! label is defined centrally and used identically everywhere, and no screen
//! picks its own thresholds.
//!
//! Without that rule, "high confidence" drifts. The key detector's author calls
//! 0.7 high because that is good for key detection; the tempo estimator's author
//! calls 0.9 high because tempo is easier; a designer rounds a number in a view.
//! The user, who sees only the word, learns that the label means nothing and
//! stops reading it — and a label nobody reads is worse than no label, because
//! it occupied the space where a real signal could have been.
//!
//! So the number is a domain type, and turning it into a word happens in
//! exactly one function.
//!
//! # Calibration status
//!
//! The thresholds below are **provisional**. ADR-0006 requires them to be
//! calibrated against held-out labelled data in Phase 3: the intent is that
//! results labelled *High* are right about as often as a user would expect from
//! the word, which is an empirical claim that cannot be settled by choosing
//! round numbers.
//!
//! They are written here as named constants with their reasoning, rather than
//! inlined, so that calibration is a change to five numbers in one file and not
//! an archaeology exercise. Risk R-04 tracks this.

use core::fmt;

use crate::num::narrow;

/// The lower bound of [`ConfidenceLabel::VeryHigh`].
///
/// Reserved for measurements that are essentially structural rather than
/// statistical — a tempo locked to a machine-programmed grid, a key with one
/// unambiguous profile match. The bar is high because this label is what
/// licenses the product to act without asking.
const VERY_HIGH: f32 = 0.90;

/// The lower bound of [`ConfidenceLabel::High`].
const HIGH: f32 = 0.75;

/// The lower bound of [`ConfidenceLabel::Medium`].
const MEDIUM: f32 = 0.50;

/// The lower bound of [`ConfidenceLabel::Low`].
///
/// Below this the result is shown as experimental. Master Prompt #25 requires
/// the product to be honest about uncertainty rather than confident by default,
/// and a result the system does not believe should be labelled so plainly that
/// a user does not have to interpret it.
const LOW: f32 = 0.25;

/// How well determined a measurement was, from zero to one.
///
/// Zero means the stage ran but found nothing to support any answer. One means
/// the answer is forced by the data. Neither extreme occurs in practice; both
/// are representable so that arithmetic on the type never has to special-case
/// its own range.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct Confidence(f32);

impl Confidence {
    /// No support at all for the reported answer.
    pub const NONE: Self = Self(0.0);

    /// Complete certainty, which real measurements do not reach.
    pub const CERTAIN: Self = Self(1.0);

    /// Creates a confidence, clamping to the valid range.
    ///
    /// Clamping rather than rejecting is deliberate. The callers are scoring
    /// functions whose ratios can land a hair outside the range through
    /// rounding, and a stage that returned an error because a score came out at
    /// 1.0000001 would be failing for a reason that has nothing to do with the
    /// music. Values that are not numbers become [`Confidence::NONE`], because
    /// a stage that produced a non-number knows nothing.
    #[must_use]
    pub fn new(value: f32) -> Self {
        if value.is_nan() {
            return Self::NONE;
        }
        Self(value.clamp(0.0, 1.0))
    }

    /// Creates a confidence from a double-precision score.
    #[must_use]
    pub fn from_f64(value: f64) -> Self {
        Self::new(narrow(value))
    }

    /// The underlying value.
    #[must_use]
    pub const fn value(self) -> f32 {
        self.0
    }

    /// The label the user sees.
    #[must_use]
    pub fn label(self) -> ConfidenceLabel {
        if self.0 >= VERY_HIGH {
            ConfidenceLabel::VeryHigh
        } else if self.0 >= HIGH {
            ConfidenceLabel::High
        } else if self.0 >= MEDIUM {
            ConfidenceLabel::Medium
        } else if self.0 >= LOW {
            ConfidenceLabel::Low
        } else {
            ConfidenceLabel::Experimental
        }
    }

    /// Combines the confidence of a result with that of what it was derived
    /// from.
    ///
    /// The weakest link, not the product and not the average.
    ///
    /// The product is what independent probabilities would give, but these are
    /// not independent — a beat grid derived from a tempo shares all of the
    /// tempo's uncertainty — so multiplying would understate confidence in a way
    /// that compounds down a chain of five stages until everything is labelled
    /// experimental. The average is worse: it lets a confident later stage
    /// launder an unreliable earlier one, which is precisely the failure this
    /// scale exists to prevent. A downbeat cannot be better known than the
    /// beats it was chosen from.
    #[must_use]
    pub fn and_then(self, other: Self) -> Self {
        if self.0 <= other.0 {
            self
        } else {
            other
        }
    }

    /// Whether the measurement is strong enough to act on without asking.
    ///
    /// The one place in the product that decides what "reliable enough" means,
    /// so that a change to the policy is a change here rather than in every
    /// caller.
    #[must_use]
    pub fn is_actionable(self) -> bool {
        self.0 >= HIGH
    }
}

impl fmt::Display for Confidence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:.2}", self.0)
    }
}

/// The five confidence labels of Master Prompt #25.
///
/// Ordered from least to most certain so that comparison means what it reads
/// like.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ConfidenceLabel {
    /// The system is showing a result it does not believe.
    Experimental,
    /// Plausible, and worth checking before relying on.
    Low,
    /// Probably right.
    Medium,
    /// Reliable enough to act on.
    High,
    /// Forced by the data.
    VeryHigh,
}

impl ConfidenceLabel {
    /// A stable identifier for this label.
    ///
    /// Deliberately not the display text. Master Prompt #8 requires the
    /// interface to be localised, and Master Prompt #16 puts user-facing strings
    /// in the presentation layer; a core crate that returned English prose would
    /// make the core the place translations live, which is exactly backwards.
    /// This key is what a localisation table is indexed by.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Experimental => "confidence.experimental",
            Self::Low => "confidence.low",
            Self::Medium => "confidence.medium",
            Self::High => "confidence.high",
            Self::VeryHigh => "confidence.very_high",
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::float_cmp,
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a test that cannot build its own fixture should fail loudly, and the signal \
                  generators here work in exact, bounded quantities"
    )]

    use super::*;

    #[test]
    fn the_scale_is_continuous_and_monotonic() {
        // Walking the whole range must never move a label backwards. A
        // non-monotonic mapping would let a better measurement read as less
        // certain, which no amount of calibration could fix.
        let mut previous = ConfidenceLabel::Experimental;
        let mut step = 0;
        while step <= 1000 {
            let value = crate::num::narrow(f64::from(step) / 1000.0);
            let label = Confidence::new(value).label();
            assert!(
                label >= previous,
                "label went backwards at {value}: {previous:?} then {label:?}"
            );
            previous = label;
            step += 1;
        }
        assert_eq!(previous, ConfidenceLabel::VeryHigh);
    }

    #[test]
    fn every_label_is_reachable() {
        // A label no measurement can produce is a promise to the user that the
        // system cannot keep.
        use ConfidenceLabel::{Experimental, High, Low, Medium, VeryHigh};
        assert_eq!(Confidence::new(0.0).label(), Experimental);
        assert_eq!(Confidence::new(0.3).label(), Low);
        assert_eq!(Confidence::new(0.6).label(), Medium);
        assert_eq!(Confidence::new(0.8).label(), High);
        assert_eq!(Confidence::new(1.0).label(), VeryHigh);
    }

    #[test]
    fn a_chain_is_no_stronger_than_its_weakest_stage() {
        let tempo = Confidence::new(0.95);
        let beats = Confidence::new(0.60);
        let downbeat = Confidence::new(0.99);

        let combined = tempo.and_then(beats).and_then(downbeat);
        assert_eq!(combined, beats);
        assert_eq!(combined.label(), ConfidenceLabel::Medium);

        // Order does not matter, which is what makes it safe to combine in
        // whatever order a pipeline happens to run.
        assert_eq!(downbeat.and_then(beats).and_then(tempo), combined);
    }

    #[test]
    fn out_of_range_and_non_numeric_input_is_contained() {
        assert_eq!(Confidence::new(-1.0), Confidence::NONE);
        assert_eq!(Confidence::new(7.0), Confidence::CERTAIN);
        assert_eq!(Confidence::new(f32::NAN), Confidence::NONE);
        assert_eq!(Confidence::new(f32::INFINITY), Confidence::CERTAIN);
        assert_eq!(Confidence::from_f64(f64::NAN), Confidence::NONE);
    }

    #[test]
    fn actionability_agrees_with_the_label() {
        // The two must not drift apart: a result the product acts on silently
        // while showing "Medium" would be the system doing something the
        // interface said it was unsure about.
        for step in 0..=100 {
            let value = crate::num::narrow(f64::from(step) / 100.0);
            let confidence = Confidence::new(value);
            assert_eq!(
                confidence.is_actionable(),
                confidence.label() >= ConfidenceLabel::High,
                "disagreement at {value}"
            );
        }
    }

    #[test]
    fn label_keys_are_distinct() {
        let keys = [
            ConfidenceLabel::Experimental.key(),
            ConfidenceLabel::Low.key(),
            ConfidenceLabel::Medium.key(),
            ConfidenceLabel::High.key(),
            ConfidenceLabel::VeryHigh.key(),
        ];
        for (index, key) in keys.iter().enumerate() {
            for (other_index, other) in keys.iter().enumerate() {
                assert!(
                    index == other_index || key != other,
                    "two labels share the key {key}"
                );
            }
        }
    }
}
