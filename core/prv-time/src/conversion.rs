//! Exact integer conversion between sample frames and musical ticks.
//!
//! Extracted so that the transport clock and the tempo map cannot drift apart:
//! two implementations of the same arithmetic is two implementations to keep in
//! agreement, and they would not stay in agreement.
//!
//! Every conversion uses 128-bit intermediates and rounds to nearest, so results
//! are bit-identical on every platform. ADR-0006 requires the planner to be
//! reproducible; that guarantee starts here.

use crate::musical_time::TICKS_PER_BEAT;
use crate::sample_rate::SampleRate;
use crate::tempo::Tempo;

/// Microseconds in a second.
const MICROS_PER_SECOND: i128 = 1_000_000;

/// Divides two integers, rounding to the nearest value and away from zero on a
/// tie.
///
/// Rounding to nearest rather than truncating halves the worst-case error and
/// makes the conversion symmetric: a tick position converted to frames and back
/// returns the original tick whenever one tick spans at least one frame.
///
/// `denominator` is positive at every call site; the zero guard exists so the
/// function is total rather than because it can be reached.
pub(crate) const fn div_round_nearest(numerator: i128, denominator: i128) -> i128 {
    if denominator == 0 {
        return 0;
    }
    #[allow(
        clippy::integer_division,
        reason = "rounding is applied explicitly by the bias term"
    )]
    if numerator >= 0 {
        (numerator + denominator / 2) / denominator
    } else {
        (numerator - denominator / 2) / denominator
    }
}

/// Clamps a 128-bit intermediate into the 64-bit domain type.
///
/// Reaching either bound requires a position beyond any physically possible
/// session. Clamping explicitly is preferred to a silent wrap, which would place
/// the playhead somewhere arbitrary.
pub(crate) const fn saturate_to_i64(value: i128) -> i64 {
    if value > i64::MAX as i128 {
        i64::MAX
    } else if value < i64::MIN as i128 {
        i64::MIN
    } else {
        #[allow(
            clippy::cast_possible_truncation,
            reason = "both bounds checked immediately above"
        )]
        {
            value as i64
        }
    }
}

/// Converts a span of frames to a span of ticks at a constant tempo.
pub(crate) fn ticks_from_frames(frames: i64, rate: SampleRate, tempo: Tempo) -> i64 {
    let numerator = i128::from(frames)
        .saturating_mul(MICROS_PER_SECOND)
        .saturating_mul(i128::from(TICKS_PER_BEAT));
    let denominator = i128::from(rate.hz_u64()).saturating_mul(i128::from(tempo.micros_per_beat()));
    saturate_to_i64(div_round_nearest(numerator, denominator))
}

/// Converts a span of ticks to a span of frames at a constant tempo.
pub(crate) fn frames_from_ticks(ticks: i64, rate: SampleRate, tempo: Tempo) -> i64 {
    let numerator = i128::from(ticks)
        .saturating_mul(i128::from(rate.hz_u64()))
        .saturating_mul(i128::from(tempo.micros_per_beat()));
    let denominator = MICROS_PER_SECOND.saturating_mul(i128::from(TICKS_PER_BEAT));
    saturate_to_i64(div_round_nearest(numerator, denominator))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rounding_is_symmetric_about_zero() {
        assert_eq!(div_round_nearest(10, 4), 3);
        assert_eq!(div_round_nearest(-10, 4), -3);
        assert_eq!(div_round_nearest(2, 4), 1);
        assert_eq!(div_round_nearest(-2, 4), -1);
        assert_eq!(div_round_nearest(0, 4), 0);
        assert_eq!(div_round_nearest(1, 0), 0);
    }

    #[test]
    fn saturation_clamps_rather_than_wrapping() {
        assert_eq!(saturate_to_i64(i128::from(i64::MAX) + 1), i64::MAX);
        assert_eq!(saturate_to_i64(i128::from(i64::MIN) - 1), i64::MIN);
        assert_eq!(saturate_to_i64(42), 42);
    }

    #[test]
    fn one_beat_at_128_bpm_is_22500_frames_at_48k() {
        let frames = frames_from_ticks(
            i64::from(TICKS_PER_BEAT),
            SampleRate::HZ_48000,
            Tempo::BPM_128,
        );
        assert_eq!(frames, 22_500);
        assert_eq!(
            ticks_from_frames(22_500, SampleRate::HZ_48000, Tempo::BPM_128),
            i64::from(TICKS_PER_BEAT)
        );
    }

    #[test]
    fn conversions_are_sign_symmetric() {
        let rate = SampleRate::HZ_48000;
        let tempo = Tempo::BPM_120;
        for beats in 1..64_i64 {
            let ticks = beats * i64::from(TICKS_PER_BEAT);
            let forward = frames_from_ticks(ticks, rate, tempo);
            let backward = frames_from_ticks(-ticks, rate, tempo);
            assert_eq!(forward, -backward, "conversion must be odd-symmetric");
        }
    }
}
