//! The numeric conversions this crate performs, with their bounds argued once.
//!
//! Same discipline as the other core crates: the workspace denies lossy casts,
//! and an allow at every arithmetic site would train the reader to skip past
//! them.

/// Widens a count to a real number.
///
/// Every count here is a number of observations, bounded by
/// [`crate::profile::MAX_OBSERVATIONS`] and therefore far below the 2^53 where
/// `f64` stops representing integers exactly.
#[allow(
    clippy::cast_precision_loss,
    reason = "observation counts are bounded far below 2^53, where f64 is exact"
)]
#[inline]
pub(crate) fn count_to_f64(count: usize) -> f64 {
    count as f64
}

/// Narrows a computed value to the precision a profile stores.
///
/// Correlations and scales are `f32` because they are compared and displayed,
/// never accumulated. The arithmetic that produces them is `f64`.
#[allow(
    clippy::cast_possible_truncation,
    reason = "storage precision for correlations; the argument is in the doc comment"
)]
#[inline]
pub(crate) fn narrow(value: f64) -> f32 {
    value as f32
}

/// The ratio of two counts, as a real.
#[inline]
pub(crate) fn ratio(numerator: usize, denominator: usize) -> f64 {
    if denominator == 0 {
        return 0.0;
    }
    count_to_f64(numerator) / count_to_f64(denominator)
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
        assert_eq!(count_to_f64(4_096), 4_096.0);
        assert_eq!(narrow(0.5), 0.5_f32);
        assert_eq!(ratio(1, 4), 0.25);
    }

    #[test]
    fn a_ratio_with_no_denominator_is_zero_rather_than_infinite() {
        // A profile with no threshold would otherwise poison every strength it
        // touched.
        assert_eq!(ratio(5, 0), 0.0);
    }
}
