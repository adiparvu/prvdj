//! Describing a parameter: what values it takes and how a control maps onto it.
//!
//! # Identity lives elsewhere
//!
//! *Which* parameter is [`prv_project::ParameterAddress`], and it lives in the
//! document model because it is a value the document persists and synchronises
//! (ADR-0007). *What values it takes* is here, because it is not persisted: a
//! descriptor is what the engine or a plugin declares at load time, and it can
//! change between builds without any project file meaning something different.
//!
//! That split is what makes automation robust to a parameter's range being
//! widened later. A lane stores a number from zero to one; the descriptor in
//! force at the time converts. Store decibels instead and every existing curve
//! silently changes meaning the day the range moves.

use core::fmt;

/// What a parameter accepts, and how it should be presented.
///
/// # Why the descriptor is separate from the address
///
/// An address says *which* parameter; a descriptor says what values it takes.
/// Keeping them apart is what lets automation store a normalised value from
/// zero to one — which is stable when a parameter's range is later widened —
/// while an interface still shows the user decibels or hertz.
///
/// It is also what lets a plugin describe its own parameters at load time
/// without the host having compiled anything about them.
#[derive(Debug, Clone, PartialEq)]
pub struct ParameterDescriptor {
    minimum: f32,
    maximum: f32,
    default: f32,
    unit: ParameterUnit,
    curve: ParameterCurve,
}

impl ParameterDescriptor {
    /// Describes a parameter.
    ///
    /// # Errors
    ///
    /// Returns [`DescriptorError::InvalidRange`] when the bounds are not
    /// finite or the minimum is not below the maximum, and clamps a default
    /// outside the range rather than rejecting it — a default is a suggestion,
    /// and refusing to load a plugin over one would be disproportionate.
    pub fn new(
        minimum: f32,
        maximum: f32,
        default: f32,
        unit: ParameterUnit,
        curve: ParameterCurve,
    ) -> Result<Self, DescriptorError> {
        if !minimum.is_finite() || !maximum.is_finite() || minimum >= maximum {
            return Err(DescriptorError::InvalidRange { minimum, maximum });
        }
        let default = if default.is_finite() {
            default.clamp(minimum, maximum)
        } else {
            minimum
        };
        Ok(Self {
            minimum,
            maximum,
            default,
            unit,
            curve,
        })
    }

    /// The lowest permitted value.
    #[must_use]
    pub const fn minimum(&self) -> f32 {
        self.minimum
    }

    /// The highest permitted value.
    #[must_use]
    pub const fn maximum(&self) -> f32 {
        self.maximum
    }

    /// The value the parameter takes before anything sets it.
    #[must_use]
    pub const fn default(&self) -> f32 {
        self.default
    }

    /// What the value means.
    #[must_use]
    pub const fn unit(&self) -> ParameterUnit {
        self.unit
    }

    /// How a normalised position maps onto the range.
    #[must_use]
    pub const fn curve(&self) -> ParameterCurve {
        self.curve
    }

    /// Converts a real value into the normalised zero-to-one form automation
    /// stores.
    #[must_use]
    pub fn normalise(&self, value: f32) -> f32 {
        let span = f64::from(self.maximum) - f64::from(self.minimum);
        if span <= 0.0 {
            return 0.0;
        }
        let clamped = if value.is_finite() {
            value.clamp(self.minimum, self.maximum)
        } else {
            self.minimum
        };
        let linear = (f64::from(clamped) - f64::from(self.minimum)) / span;
        crate::num::narrow(self.curve.to_normalised(linear))
    }

    /// Converts a normalised position back into a real value.
    #[must_use]
    pub fn denormalise(&self, normalised: f32) -> f32 {
        let position = if normalised.is_finite() {
            f64::from(normalised.clamp(0.0, 1.0))
        } else {
            0.0
        };
        let linear = self.curve.to_linear(position);
        let span = f64::from(self.maximum) - f64::from(self.minimum);
        crate::num::narrow(f64::from(self.minimum) + linear * span)
    }
}

/// What a parameter's value means.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ParameterUnit {
    /// A bare number.
    Plain,
    /// Decibels.
    Decibels,
    /// Hertz.
    Hertz,
    /// A percentage from zero to one hundred.
    Percent,
    /// Seconds.
    Seconds,
    /// Beats.
    Beats,
    /// A switch.
    Toggle,
}

/// How a normalised position maps onto a parameter's range.
///
/// The mapping belongs to the parameter rather than to the control that draws
/// it, because automation stores normalised values: a curve stored in the
/// interface would mean the same automation lane produced different sound
/// depending on which control last wrote it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ParameterCurve {
    /// Position maps directly onto value.
    Linear,

    /// Position maps onto the logarithm of the value.
    ///
    /// What frequency controls need. A filter swept linearly from 20 Hz to
    /// 20 kHz spends nine tenths of its travel above 2 kHz, where almost no
    /// musical decisions are made; a logarithmic sweep gives each octave the
    /// same distance, which is how the ear hears it.
    Logarithmic,

    /// Position maps onto the square of the value.
    ///
    /// What level controls need: it puts finer resolution near silence, where
    /// a decibel matters most.
    Squared,
}

impl ParameterCurve {
    /// Maps a linear fraction of the range onto a normalised position.
    fn to_normalised(self, linear: f64) -> f64 {
        let clamped = linear.clamp(0.0, 1.0);
        match self {
            Self::Linear => clamped,
            // A decade of headroom below the top, which spans the useful range
            // of both a filter sweep and a fader without ever taking the
            // logarithm of zero.
            Self::Logarithmic => {
                let floor = 1.0_f64 / 1_000.0;
                let value = floor + clamped * (1.0 - floor);
                (value / floor).log10() / (1.0 / floor).log10()
            }
            Self::Squared => clamped.sqrt(),
        }
    }

    /// Maps a normalised position back onto a linear fraction of the range.
    ///
    /// Named `to_linear` rather than `from_normalised` because it takes `self`:
    /// a `from_` method that consumes a receiver reads as a constructor and is
    /// not one.
    fn to_linear(self, position: f64) -> f64 {
        let clamped = position.clamp(0.0, 1.0);
        match self {
            Self::Linear => clamped,
            Self::Logarithmic => {
                let floor = 1.0_f64 / 1_000.0;
                let value = floor * (1.0 / floor).powf(clamped);
                ((value - floor) / (1.0 - floor)).clamp(0.0, 1.0)
            }
            Self::Squared => clamped * clamped,
        }
    }
}

/// Errors from building a parameter descriptor.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum DescriptorError {
    /// A descriptor whose bounds are unusable.
    InvalidRange {
        /// The minimum supplied.
        minimum: f32,
        /// The maximum supplied.
        maximum: f32,
    },
}

impl fmt::Display for DescriptorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRange { minimum, maximum } => {
                write!(f, "parameter range {minimum} to {maximum} is unusable")
            }
        }
    }
}

impl core::error::Error for DescriptorError {}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::panic,
        clippy::float_cmp,
        reason = "a test that cannot build its own fixture should fail loudly"
    )]

    use super::*;

    #[test]
    fn normalising_and_denormalising_round_trip_on_every_curve() {
        // The property automation depends on. A lane stores normalised values;
        // if the round trip were lossy, every automation curve would drift a
        // little each time it was edited.
        for curve in [
            ParameterCurve::Linear,
            ParameterCurve::Logarithmic,
            ParameterCurve::Squared,
        ] {
            let descriptor =
                ParameterDescriptor::new(20.0, 20_000.0, 1_000.0, ParameterUnit::Hertz, curve)
                    .expect("valid");
            let mut step = 0;
            while step <= 100 {
                let position = crate::num::narrow(f64::from(step) / 100.0);
                let value = descriptor.denormalise(position);
                let back = descriptor.normalise(value);
                assert!(
                    (f64::from(back) - f64::from(position)).abs() < 1e-3,
                    "{curve:?} at {position} became {value} and returned {back}"
                );
                step += 1;
            }
        }
    }

    #[test]
    fn a_logarithmic_sweep_gives_each_octave_the_same_travel() {
        // The reason the curve belongs to the parameter. A filter swept
        // linearly from 20 Hz to 20 kHz spends nine tenths of its travel above
        // 2 kHz, where almost no musical decisions are made.
        let descriptor = ParameterDescriptor::new(
            20.0,
            20_000.0,
            1_000.0,
            ParameterUnit::Hertz,
            ParameterCurve::Logarithmic,
        )
        .expect("valid");

        let quarter = descriptor.denormalise(0.25);
        let half = descriptor.denormalise(0.5);
        let three_quarters = descriptor.denormalise(0.75);

        // Each quarter of the travel multiplies the frequency by a similar
        // factor, rather than adding a similar number of hertz.
        let first = f64::from(half) / f64::from(quarter);
        let second = f64::from(three_quarters) / f64::from(half);
        assert!(
            (first / second - 1.0).abs() < 0.35,
            "the ratios {first} and {second} are not comparable, so the sweep is not logarithmic"
        );

        let linear = ParameterDescriptor::new(
            20.0,
            20_000.0,
            1_000.0,
            ParameterUnit::Hertz,
            ParameterCurve::Linear,
        )
        .expect("valid");
        assert!(
            descriptor.denormalise(0.5) < linear.denormalise(0.5) / 4.0,
            "the logarithmic midpoint should sit far below the linear one"
        );
    }

    #[test]
    fn out_of_range_and_non_numeric_values_are_contained() {
        let descriptor = ParameterDescriptor::new(
            -24.0,
            6.0,
            0.0,
            ParameterUnit::Decibels,
            ParameterCurve::Linear,
        )
        .expect("valid");
        assert_eq!(descriptor.normalise(-100.0), 0.0);
        assert_eq!(descriptor.normalise(100.0), 1.0);
        assert_eq!(descriptor.normalise(f32::NAN), 0.0);
        assert_eq!(descriptor.denormalise(-1.0), -24.0);
        assert_eq!(descriptor.denormalise(2.0), 6.0);
        assert_eq!(descriptor.denormalise(f32::NAN), -24.0);
    }

    #[test]
    fn an_unusable_range_is_refused_and_a_stray_default_is_corrected() {
        // A range is a contract and an unusable one is a defect; a default is a
        // suggestion, and refusing to load a plugin over one would be
        // disproportionate.
        assert!(matches!(
            ParameterDescriptor::new(1.0, 1.0, 1.0, ParameterUnit::Plain, ParameterCurve::Linear),
            Err(DescriptorError::InvalidRange { .. })
        ));
        assert!(matches!(
            ParameterDescriptor::new(
                f32::NAN,
                1.0,
                0.0,
                ParameterUnit::Plain,
                ParameterCurve::Linear
            ),
            Err(DescriptorError::InvalidRange { .. })
        ));

        let corrected =
            ParameterDescriptor::new(0.0, 1.0, 9.0, ParameterUnit::Plain, ParameterCurve::Linear)
                .expect("valid range");
        assert_eq!(corrected.default(), 1.0);
    }
}
