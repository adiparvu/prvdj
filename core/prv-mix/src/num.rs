//! The numeric conversions this crate performs, with their bounds argued once.
//!
//! Same discipline as `prv-analysis::num`, and for the same reason: the
//! workspace denies lossy casts, the planner converts between counts and reals
//! on almost every line, and an allow at every arithmetic site would train the
//! reader to skip past them.

/// Widens a count to a real number.
///
/// Every count here is a track index, a beam index or a set length. All are
/// bounded by the size of a music library, which is far below the 2^53 where
/// `f64` stops representing integers exactly.
#[allow(
    clippy::cast_precision_loss,
    reason = "counts here are bounded far below 2^53, where f64 is exact"
)]
#[inline]
pub(crate) fn count_to_f64(count: usize) -> f64 {
    count as f64
}

/// Widens a frame count to a real number.
///
/// Sample positions, which at 768 kHz reach 2^53 after roughly three centuries
/// of continuous audio.
#[allow(
    clippy::cast_precision_loss,
    reason = "sample positions are bounded far below 2^53, where f64 is exact"
)]
#[inline]
pub(crate) fn signed_to_f64(count: i64) -> f64 {
    count as f64
}

/// Narrows a computed score to the precision scores are stored in.
///
/// Scores are `f32` because they are compared, ranked and displayed, never
/// accumulated over long chains — the arithmetic that produces them is `f64`
/// throughout and only the result is narrowed. Non-finite input passes through
/// unchanged so that a defect upstream stays visible rather than being
/// laundered into a plausible number.
#[allow(
    clippy::cast_possible_truncation,
    reason = "storage precision for scores; the argument is in the doc comment"
)]
#[inline]
pub(crate) fn narrow(value: f64) -> f32 {
    value as f32
}

/// Converts microseconds to seconds.
///
/// `prv-time` stores tempo as an exact integer number of microseconds per beat;
/// the renderer works in real-valued frame positions. The two meet here, once.
#[inline]
pub(crate) fn micros_to_seconds(micros: u64) -> f64 {
    let value = u32::try_from(micros).map_or_else(
        |_| count_to_f64(usize::try_from(micros).unwrap_or(usize::MAX)),
        f64::from,
    );
    value / 1_000_000.0
}

/// Rounds a real to the nearest frame count, saturating rather than wrapping.
///
/// Saturation is deliberate: a position clamped to the end of a set is a wrong
/// answer that is obvious, and one that wrapped to a negative is not.
#[allow(
    clippy::cast_possible_truncation,
    reason = "explicitly clamped to the i64 range on the lines above"
)]
#[inline]
pub(crate) fn round_to_i64(value: f64) -> i64 {
    if value.is_nan() {
        return 0;
    }
    let rounded = value.round();
    if rounded >= signed_to_f64(i64::MAX) {
        return i64::MAX;
    }
    if rounded <= signed_to_f64(i64::MIN) {
        return i64::MIN;
    }
    rounded as i64
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
        assert_eq!(count_to_f64(0), 0.0);
        assert_eq!(count_to_f64(100_000), 100_000.0);
        assert_eq!(signed_to_f64(-44_100), -44_100.0);
        assert_eq!(narrow(0.25), 0.25_f32);
    }

    #[test]
    fn rounding_saturates_rather_than_wrapping() {
        assert_eq!(round_to_i64(f64::NAN), 0);
        assert_eq!(round_to_i64(f64::INFINITY), i64::MAX);
        assert_eq!(round_to_i64(f64::NEG_INFINITY), i64::MIN);
        assert_eq!(round_to_i64(2.5), 3);
        assert_eq!(round_to_i64(-2.5), -3);
    }

    #[test]
    fn microseconds_convert_to_seconds() {
        assert_eq!(micros_to_seconds(1_000_000), 1.0);
        assert_eq!(micros_to_seconds(468_750), 0.468_75);
    }

    #[test]
    fn narrow_preserves_non_finite_values() {
        assert!(narrow(f64::NAN).is_nan());
        assert!(narrow(f64::INFINITY).is_infinite());
    }
}
