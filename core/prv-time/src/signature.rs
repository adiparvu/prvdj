use core::fmt;
use core::num::NonZeroU8;

use crate::error::TimeError;
use crate::musical_time::{MusicalTime, Ticks, TICKS_PER_BEAT, TICKS_PER_WHOLE_NOTE};

/// The largest time-signature denominator the engine accepts.
///
/// Bounded at 32 so that every bar length remains a whole number of ticks.
const MAX_DENOMINATOR: u8 = 32;

/// A musical time signature, such as 4/4 or 6/8.
///
/// # How signatures relate to tempo
///
/// Tempo is expressed in quarter notes per minute, independently of the
/// signature. The signature determines only how long a bar is. A 6/8 bar is six
/// eighth notes, which is three quarter notes, so at a given tempo a 6/8 bar
/// lasts three beats.
///
/// Keeping tempo and bar length independent is what allows a 7/8 section to sit
/// inside a project without the transport needing a special case: its bar is
/// simply 3.5 quarter notes long, which is an exact number of ticks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TimeSignature {
    numerator: NonZeroU8,
    denominator: u8,
}

impl TimeSignature {
    /// Four beats to the bar — the overwhelming majority of DJ material.
    pub const FOUR_FOUR: Self = Self::new_const(4, 4);
    /// Three beats to the bar.
    pub const THREE_FOUR: Self = Self::new_const(3, 4);
    /// Six eighth notes to the bar.
    pub const SIX_EIGHT: Self = Self::new_const(6, 8);

    /// Creates a time signature.
    ///
    /// # Errors
    ///
    /// Returns [`TimeError::InvalidSignatureNumerator`] if `numerator` is zero,
    /// or [`TimeError::InvalidSignatureDenominator`] if `denominator` is not a
    /// power of two between 1 and 32.
    pub const fn new(numerator: u8, denominator: u8) -> Result<Self, TimeError> {
        let Some(numerator) = NonZeroU8::new(numerator) else {
            return Err(TimeError::InvalidSignatureNumerator { numerator });
        };
        if denominator == 0 || denominator > MAX_DENOMINATOR || !denominator.is_power_of_two() {
            return Err(TimeError::InvalidSignatureDenominator { denominator });
        }
        Ok(Self {
            numerator,
            denominator,
        })
    }

    /// Builds a compile-time constant from a known-good pair.
    const fn new_const(numerator: u8, denominator: u8) -> Self {
        match Self::new(numerator, denominator) {
            Ok(signature) => signature,
            // Unreachable for the literals used above; avoids a panic path in a
            // `const fn`.
            Err(_) => Self {
                numerator: NonZeroU8::MIN,
                denominator: 4,
            },
        }
    }

    /// Returns the numerator: how many notes of the denominator's value fit in
    /// a bar.
    #[must_use]
    pub const fn numerator(self) -> u8 {
        self.numerator.get()
    }

    /// Returns the denominator: the note value that counts as one unit.
    #[must_use]
    pub const fn denominator(self) -> u8 {
        self.denominator
    }

    /// Returns the length of one bar in ticks.
    ///
    /// Always exact: [`TICKS_PER_WHOLE_NOTE`] is divisible by every supported
    /// denominator, so no rounding occurs.
    ///
    /// The result fits comfortably in 32 bits: the widest possible bar is 255
    /// whole notes, or 3 916 800 ticks.
    #[must_use]
    pub const fn ticks_per_bar(self) -> u32 {
        // Both operands are validated: the denominator is a power of two no
        // greater than 32, and the whole-note tick count divides exactly by all
        // of them.
        #[allow(
            clippy::integer_division,
            reason = "exact by construction; see TICKS_PER_WHOLE_NOTE"
        )]
        let ticks_per_unit = TICKS_PER_WHOLE_NOTE / self.denominator as u32;
        ticks_per_unit * self.numerator.get() as u32
    }

    /// Decomposes an absolute tick position into bar, beat and tick.
    ///
    /// Handles negative positions correctly: bar `-1` beat `0` is one bar before
    /// the origin, not a truncation towards zero. This matters because
    /// transitions and count-ins legitimately begin before the arrangement does.
    #[must_use]
    pub fn ticks_to_musical(self, ticks: Ticks) -> MusicalTime {
        // Non-zero by construction: the numerator is non-zero and
        // `ticks_per_unit` is at least 480. Widening from `u32` is lossless.
        let ticks_per_bar = i64::from(self.ticks_per_bar());

        let bar = ticks.get().div_euclid(ticks_per_bar);
        let within_bar = ticks.get().rem_euclid(ticks_per_bar);

        let beat = within_bar.div_euclid(i64::from(TICKS_PER_BEAT));
        let tick = within_bar.rem_euclid(i64::from(TICKS_PER_BEAT));

        // `within_bar` is in `0..ticks_per_bar`, so `beat` fits comfortably in
        // u32 for any supported signature, and `tick` is below TICKS_PER_BEAT.
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "both values are bounded by rem_euclid above"
        )]
        MusicalTime {
            bar,
            beat: beat as u32,
            tick: tick as u32,
        }
    }

    /// Composes a bar, beat and tick position into an absolute tick count.
    ///
    /// Beat and tick values beyond their nominal range carry into the next bar
    /// rather than being rejected, so callers can do arithmetic without
    /// normalising first.
    #[must_use]
    pub fn musical_to_ticks(self, position: MusicalTime) -> Ticks {
        let ticks_per_bar = i64::from(self.ticks_per_bar());
        let from_bars = position.bar.saturating_mul(ticks_per_bar);
        let from_beats = i64::from(position.beat).saturating_mul(i64::from(TICKS_PER_BEAT));
        Ticks::new(
            from_bars
                .saturating_add(from_beats)
                .saturating_add(i64::from(position.tick)),
        )
    }
}

impl fmt::Display for TimeSignature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.numerator.get(), self.denominator)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn four_four_bar_is_four_beats() {
        assert_eq!(TimeSignature::FOUR_FOUR.ticks_per_bar(), TICKS_PER_BEAT * 4);
    }

    #[test]
    fn six_eight_bar_is_three_quarter_notes() {
        assert_eq!(TimeSignature::SIX_EIGHT.ticks_per_bar(), TICKS_PER_BEAT * 3);
    }

    #[test]
    fn seven_eight_bar_is_three_and_a_half_quarter_notes() {
        let signature = TimeSignature::new(7, 8);
        assert!(signature.is_ok());
        if let Ok(signature) = signature {
            // Seven eighth notes: 7 × 1920 ticks. Written as a product rather
            // than as `TICKS_PER_BEAT * 7 / 2` so that the expected value is
            // stated directly instead of being derived by a division that could
            // itself round.
            assert_eq!(
                signature.ticks_per_bar(),
                1_920 * 7,
                "a 7/8 bar is exactly three and a half quarter notes"
            );
        }
    }

    #[test]
    fn rejects_invalid_signatures() {
        assert!(matches!(
            TimeSignature::new(0, 4),
            Err(TimeError::InvalidSignatureNumerator { numerator: 0 })
        ));
        assert!(matches!(
            TimeSignature::new(4, 0),
            Err(TimeError::InvalidSignatureDenominator { denominator: 0 })
        ));
        assert!(matches!(
            TimeSignature::new(4, 6),
            Err(TimeError::InvalidSignatureDenominator { denominator: 6 })
        ));
        assert!(matches!(
            TimeSignature::new(4, 64),
            Err(TimeError::InvalidSignatureDenominator { denominator: 64 })
        ));
    }

    #[test]
    fn decomposition_is_correct_at_bar_boundaries() {
        let signature = TimeSignature::FOUR_FOUR;
        let one_bar = Ticks::new(i64::from(TICKS_PER_BEAT) * 4);

        assert_eq!(
            signature.ticks_to_musical(Ticks::ZERO),
            MusicalTime::new(0, 0, 0)
        );
        assert_eq!(
            signature.ticks_to_musical(one_bar),
            MusicalTime::new(1, 0, 0)
        );
        assert_eq!(
            signature.ticks_to_musical(Ticks::new(one_bar.get() - 1)),
            MusicalTime::new(0, 3, TICKS_PER_BEAT - 1)
        );
    }

    #[test]
    fn negative_positions_floor_rather_than_truncate() {
        let signature = TimeSignature::FOUR_FOUR;
        let one_tick_before_origin = Ticks::new(-1);

        // Truncation towards zero would give bar 0; flooring gives bar -1,
        // which is the musically correct answer.
        assert_eq!(
            signature.ticks_to_musical(one_tick_before_origin),
            MusicalTime::new(-1, 3, TICKS_PER_BEAT - 1)
        );

        let one_bar_before = Ticks::new(-(i64::from(TICKS_PER_BEAT) * 4));
        assert_eq!(
            signature.ticks_to_musical(one_bar_before),
            MusicalTime::new(-1, 0, 0)
        );
    }

    #[test]
    fn composition_and_decomposition_round_trip() {
        let signatures = [
            TimeSignature::FOUR_FOUR,
            TimeSignature::THREE_FOUR,
            TimeSignature::SIX_EIGHT,
        ];
        for signature in signatures {
            for raw in [-9_600_i64, -1, 0, 1, 960, 3_839, 15_360, 1_000_000] {
                let ticks = Ticks::new(raw);
                let musical = signature.ticks_to_musical(ticks);
                assert_eq!(
                    signature.musical_to_ticks(musical),
                    ticks,
                    "round trip failed for {signature} at {raw} ticks"
                );
            }
        }
    }

    #[test]
    fn out_of_range_components_carry_into_the_next_bar() {
        let signature = TimeSignature::FOUR_FOUR;
        // Beat 4 does not exist in 4/4; it is the downbeat of the next bar.
        let overflowing = MusicalTime::new(0, 4, 0);
        let normalised = signature.ticks_to_musical(signature.musical_to_ticks(overflowing));
        assert_eq!(normalised, MusicalTime::new(1, 0, 0));
    }
}
