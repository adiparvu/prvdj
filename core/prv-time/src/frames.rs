use core::fmt;
use core::ops::{Add, AddAssign, Neg, Sub, SubAssign};

use crate::error::TimeError;
use crate::sample_rate::SampleRate;

/// A signed count of sample frames.
///
/// A *frame* is one sample across all channels: at 48 kHz, one second is 48 000
/// frames regardless of whether the audio is mono, stereo or multichannel.
/// Counting frames rather than samples removes the most common off-by-a-factor
/// error in audio code.
///
/// The count is signed because relative offsets are as common as absolute
/// positions — a transition that begins eight bars before the end of a track is
/// naturally expressed as a negative offset. Absolute positions are non-negative
/// by convention, enforced by the transport rather than by the type, so that
/// arithmetic stays ergonomic.
///
/// # Range
///
/// A 64-bit frame count at 192 kHz spans roughly 1.5 million years. Overflow is
/// therefore not a practical concern, but arithmetic is still checked because
/// silent wrapping in position maths produces misalignment that is very hard to
/// diagnose.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Frames(i64);

impl Frames {
    /// Zero frames.
    pub const ZERO: Self = Self(0);

    /// Creates a frame count.
    #[must_use]
    pub const fn new(frames: i64) -> Self {
        Self(frames)
    }

    /// Returns the raw count.
    #[must_use]
    pub const fn get(self) -> i64 {
        self.0
    }

    /// Returns `true` if the count is negative.
    #[must_use]
    pub const fn is_negative(self) -> bool {
        self.0 < 0
    }

    /// Returns the absolute count.
    ///
    /// Saturates at [`i64::MAX`] for [`i64::MIN`], which cannot be negated.
    #[must_use]
    pub const fn abs(self) -> Self {
        Self(self.0.saturating_abs())
    }

    /// Adds two counts, returning `None` on overflow.
    #[must_use]
    pub const fn checked_add(self, other: Self) -> Option<Self> {
        match self.0.checked_add(other.0) {
            Some(sum) => Some(Self(sum)),
            None => None,
        }
    }

    /// Subtracts two counts, returning `None` on overflow.
    #[must_use]
    pub const fn checked_sub(self, other: Self) -> Option<Self> {
        match self.0.checked_sub(other.0) {
            Some(difference) => Some(Self(difference)),
            None => None,
        }
    }

    /// Converts a duration in seconds to a frame count at the given rate.
    ///
    /// Rounds to the nearest frame. Used at the edges of the system where a
    /// duration arrives in seconds; internal arithmetic stays in frames.
    ///
    /// # Errors
    ///
    /// Returns [`TimeError::ConversionOverflow`] if `seconds` is not finite or
    /// the result does not fit a 64-bit frame count.
    pub fn from_seconds(seconds: f64, rate: SampleRate) -> Result<Self, TimeError> {
        if !seconds.is_finite() {
            return Err(TimeError::ConversionOverflow);
        }
        let frames = (seconds * f64::from(rate.hz())).round();
        // The bound check below is what makes the subsequent cast safe.
        if frames < -(2f64.powi(63)) || frames >= 2f64.powi(63) {
            return Err(TimeError::ConversionOverflow);
        }
        #[allow(
            clippy::cast_possible_truncation,
            reason = "range checked immediately above"
        )]
        Ok(Self(frames as i64))
    }

    /// Converts the count to a duration in seconds at the given rate.
    ///
    /// This is a lossy convenience for display and for interfaces that speak in
    /// seconds. Positional arithmetic must never round-trip through it.
    #[must_use]
    #[allow(
        clippy::cast_precision_loss,
        reason = "display-only conversion; positional arithmetic never uses this path"
    )]
    pub fn as_seconds(self, rate: SampleRate) -> f64 {
        self.0 as f64 / f64::from(rate.hz())
    }
}

impl Add for Frames {
    type Output = Self;

    /// Adds two counts, saturating rather than wrapping on overflow.
    fn add(self, other: Self) -> Self {
        Self(self.0.saturating_add(other.0))
    }
}

impl Sub for Frames {
    type Output = Self;

    /// Subtracts two counts, saturating rather than wrapping on overflow.
    fn sub(self, other: Self) -> Self {
        Self(self.0.saturating_sub(other.0))
    }
}

impl AddAssign for Frames {
    fn add_assign(&mut self, other: Self) {
        *self = *self + other;
    }
}

impl SubAssign for Frames {
    fn sub_assign(&mut self, other: Self) {
        *self = *self - other;
    }
}

impl Neg for Frames {
    type Output = Self;

    fn neg(self) -> Self {
        Self(self.0.saturating_neg())
    }
}

impl fmt::Display for Frames {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} frames", self.0)
    }
}

impl From<i64> for Frames {
    fn from(frames: i64) -> Self {
        Self(frames)
    }
}

impl From<u32> for Frames {
    fn from(frames: u32) -> Self {
        Self(i64::from(frames))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arithmetic_is_exact() {
        let a = Frames::new(22_050);
        let b = Frames::new(1_000);
        assert_eq!((a + b).get(), 23_050);
        assert_eq!((a - b).get(), 21_050);
        assert_eq!((-b).get(), -1_000);
        assert_eq!(b.abs().get(), 1_000);
        assert_eq!((-b).abs().get(), 1_000);
    }

    #[test]
    fn addition_saturates_rather_than_wrapping() {
        let huge = Frames::new(i64::MAX);
        assert_eq!((huge + Frames::new(1)).get(), i64::MAX);
        assert_eq!(huge.checked_add(Frames::new(1)), None);
    }

    #[test]
    fn subtraction_saturates_rather_than_wrapping() {
        let tiny = Frames::new(i64::MIN);
        assert_eq!((tiny - Frames::new(1)).get(), i64::MIN);
        assert_eq!(tiny.checked_sub(Frames::new(1)), None);
    }

    #[test]
    fn seconds_round_trip_within_one_frame() {
        let rate = SampleRate::HZ_48000;
        for seconds in [0.0_f64, 0.5, 1.0, 3.5, 240.0, 21_600.0] {
            let frames = Frames::from_seconds(seconds, rate);
            assert!(frames.is_ok());
            if let Ok(frames) = frames {
                let back = frames.as_seconds(rate);
                assert!(
                    (back - seconds).abs() < 1.0 / f64::from(rate.hz()),
                    "{seconds}s round-tripped to {back}s"
                );
            }
        }
    }

    #[test]
    fn rejects_non_finite_seconds() {
        let rate = SampleRate::HZ_48000;
        assert_eq!(
            Frames::from_seconds(f64::NAN, rate),
            Err(TimeError::ConversionOverflow)
        );
        assert_eq!(
            Frames::from_seconds(f64::INFINITY, rate),
            Err(TimeError::ConversionOverflow)
        );
    }

    #[test]
    fn rejects_seconds_beyond_representable_range() {
        let rate = SampleRate::HZ_192000;
        assert_eq!(
            Frames::from_seconds(1e15, rate),
            Err(TimeError::ConversionOverflow)
        );
    }
}
