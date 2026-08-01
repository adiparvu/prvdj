//! The numeric conversions this crate performs, with their bounds argued once.
//!
//! Same discipline as `prv-analysis::num` and `prv-mix::num`: the workspace
//! denies lossy casts, this crate converts between frame counts and reals on
//! every interpolation, and an allow at each site would train the reader to
//! skip past them.

/// Widens a frame count to a real number.
///
/// Sample positions, which at 768 kHz reach the 2^53 limit of exact `f64`
/// integers after roughly three centuries of continuous audio.
#[allow(
    clippy::cast_precision_loss,
    reason = "sample positions are bounded far below 2^53, where f64 is exact"
)]
#[inline]
pub(crate) fn signed_to_f64(count: i64) -> f64 {
    count as f64
}

/// Narrows a computed value to the precision automation stores.
///
/// Automation values are `f32` because they are normalised to zero-to-one and
/// handed straight to the signal path, which is `f32`. The interpolation that
/// produces them is `f64`; only the result is narrowed. Non-finite input passes
/// through unchanged so a defect upstream stays visible rather than being
/// laundered into a plausible number — though [`crate::automation::AutomationPoint`]
/// makes sure one cannot get in.
#[allow(
    clippy::cast_possible_truncation,
    reason = "storage precision for normalised values; the argument is in the doc comment"
)]
#[inline]
pub(crate) fn narrow(value: f64) -> f32 {
    value as f32
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::float_cmp,
        reason = "these conversions are exact for the values under test"
    )]

    use super::*;

    #[test]
    fn conversions_are_exact_for_the_values_this_crate_uses() {
        assert_eq!(signed_to_f64(0), 0.0);
        assert_eq!(signed_to_f64(-44_100), -44_100.0);
        assert_eq!(narrow(0.25), 0.25_f32);
    }

    #[test]
    fn narrow_preserves_non_finite_values() {
        assert!(narrow(f64::NAN).is_nan());
        assert!(narrow(f64::INFINITY).is_infinite());
    }
}
