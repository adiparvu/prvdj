//! The two numeric conversions this crate performs constantly.
//!
//! Signal analysis converts between counts and reals on almost every line: a bin
//! index becomes a frequency, a frame count becomes a duration, a `f64`
//! accumulator becomes a stored `f32`. The workspace denies lossy casts by
//! default, and rightly so — a silent truncation in the time domain is the class
//! of bug that shows up as a beat grid drifting an hour into a set.
//!
//! Rather than scatter an allow at every arithmetic site, which would train the
//! reader to skip past them, the two conversions live here with their bounds
//! argued once.

/// Widens a count to a real number.
///
/// Every count in this crate is a sample index, a bin index or a frame index.
/// The largest of those is the sample index: at 768 kHz — the highest rate the
/// engine accepts — a `f64` represents every integer exactly up to 2^53, which
/// is over three hundred years of audio. The conversion is therefore exact for
/// any input this crate can encounter, and the precision-loss warning is
/// describing a case that cannot arise.
#[allow(
    clippy::cast_precision_loss,
    reason = "counts here are bounded far below 2^53, where f64 is exact"
)]
#[inline]
pub(crate) fn count_to_f64(count: usize) -> f64 {
    count as f64
}

/// Widens a signed count to a real number.
///
/// Same argument as [`count_to_f64`]; the signed form exists for lag arithmetic,
/// which is naturally signed.
#[allow(
    clippy::cast_precision_loss,
    reason = "counts here are bounded far below 2^53, where f64 is exact"
)]
#[inline]
pub(crate) fn signed_to_f64(count: i64) -> f64 {
    count as f64
}

/// Narrows an analysis value to the storage precision used for curves.
///
/// Novelty, energy and chroma curves are stored as `f32`. That is not a
/// compromise: they are *derived* quantities whose useful dynamic range is a few
/// decades, and a full-length track produces hundreds of thousands of them, so
/// the halved memory is worth more than digits that carry no information. The
/// arithmetic that produces them is `f64` throughout; only the result is
/// narrowed.
///
/// Values outside the `f32` range become infinities rather than wrapping, and
/// callers that must not see an infinity check for it. Non-finite input passes
/// through unchanged so that a defect upstream stays visible instead of being
/// laundered into a plausible number.
#[allow(
    clippy::cast_possible_truncation,
    reason = "storage precision for derived curves; the argument is in the doc comment"
)]
#[inline]
pub(crate) fn narrow(value: f64) -> f32 {
    value as f32
}

/// Rounds a non-negative real to the nearest count, saturating at both ends.
///
/// Used where a real-valued position — a lag in frames, a beat time — must
/// become an index. Saturation rather than wrapping is deliberate: an index
/// clamped to the end of a buffer produces a wrong answer that is obvious, and
/// an index that wrapped to zero produces one that is not.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "explicitly clamped to a valid usize range on the line above"
)]
#[inline]
pub(crate) fn round_to_count(value: f64) -> usize {
    if value.is_nan() || value <= 0.0 {
        return 0;
    }
    if value.is_infinite() {
        return usize::MAX;
    }
    let rounded = value.round();
    if rounded >= count_to_f64(usize::MAX) {
        return usize::MAX;
    }
    rounded as usize
}

/// Converts microseconds to seconds.
///
/// `prv-time` stores tempo as an exact integer number of microseconds per beat.
/// Analysis works in real-valued sample positions, so the two meet here — once,
/// in a named function, rather than at each of the several sites that would
/// otherwise each write their own constant.
#[inline]
pub(crate) fn micros_to_seconds(micros: u64) -> f64 {
    // `u64` up to about nine quadrillion converts exactly; a tempo's
    // microseconds per beat is at most a few million.
    let seconds = u32::try_from(micros).map_or_else(
        |_| count_to_f64(usize::try_from(micros).unwrap_or(usize::MAX)),
        f64::from,
    );
    seconds / 1_000_000.0
}

/// Divides two counts as reals.
///
/// Integer division is denied across the workspace because it silently discards
/// a remainder that usually mattered. Where this crate genuinely wants a ratio
/// rather than a quotient, it wants a real one.
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
    fn round_to_count_saturates_rather_than_wrapping() {
        assert_eq!(round_to_count(-1.0), 0);
        assert_eq!(round_to_count(f64::NAN), 0);
        assert_eq!(round_to_count(f64::NEG_INFINITY), 0);
        assert_eq!(round_to_count(f64::INFINITY), usize::MAX);
        assert_eq!(round_to_count(2.5), 3);
        assert_eq!(round_to_count(2.4), 2);
    }

    #[test]
    fn ratio_of_zero_denominator_is_zero_rather_than_infinite() {
        // An empty window is a legitimate edge case at the start of a curve.
        // Returning zero keeps downstream arithmetic finite; returning infinity
        // would poison every accumulator it touched.
        assert!((ratio(5, 0) - 0.0).abs() < f64::EPSILON);
        assert!((ratio(1, 4) - 0.25).abs() < f64::EPSILON);
    }

    #[test]
    fn narrow_preserves_non_finite_values() {
        assert!(narrow(f64::NAN).is_nan());
        assert!(narrow(f64::INFINITY).is_infinite());
    }
}
