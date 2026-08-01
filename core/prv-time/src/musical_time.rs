use core::fmt;
use core::ops::{Add, Sub};

/// Ticks per beat, where a beat is a quarter note.
///
/// The value is chosen so that every musically meaningful subdivision lands on
/// a whole tick. 3840 factors as 2^8 × 3 × 5, which means it divides exactly by
/// 2, 3, 4, 5, 6, 8, 10, 12, 16, 24, 32, 64 and 128 — covering every duple,
/// triplet and quintuplet subdivision a DJ or producer will use, plus the
/// 1/128-note resolution needed for tight edit points.
///
/// Exactness matters more than magnitude here. A resolution that cannot
/// represent a triplet forces rounding at every triplet boundary, and rounding
/// at edit points is what makes a beat grid drift away from the music.
pub const TICKS_PER_BEAT: u32 = 3840;

/// Ticks in a whole note, derived from [`TICKS_PER_BEAT`].
///
/// Used to convert time-signature denominators into tick counts. Because this
/// is 15360 = 2^10 × 3 × 5, every power-of-two denominator up to 32 divides it
/// exactly, so every bar length is a whole number of ticks.
pub(crate) const TICKS_PER_WHOLE_NOTE: u32 = TICKS_PER_BEAT * 4;

/// A signed position or duration in musical ticks.
///
/// Ticks are tempo-relative: the same tick position corresponds to different
/// frame positions at different tempi. Frames are the authoritative unit for
/// playback; ticks are the authoritative unit for musical structure. The
/// transport clock converts between them exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Ticks(i64);

impl Ticks {
    /// Zero ticks.
    pub const ZERO: Self = Self(0);

    /// One beat.
    pub const BEAT: Self = Self(TICKS_PER_BEAT as i64);

    /// Creates a tick count.
    #[must_use]
    pub const fn new(ticks: i64) -> Self {
        Self(ticks)
    }

    /// Returns the raw count.
    #[must_use]
    pub const fn get(self) -> i64 {
        self.0
    }

    /// Creates a tick count from a whole number of beats.
    #[must_use]
    pub const fn from_beats(beats: i64) -> Self {
        Self(beats.saturating_mul(TICKS_PER_BEAT as i64))
    }
}

impl Add for Ticks {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self(self.0.saturating_add(other.0))
    }
}

impl Sub for Ticks {
    type Output = Self;

    fn sub(self, other: Self) -> Self {
        Self(self.0.saturating_sub(other.0))
    }
}

impl fmt::Display for Ticks {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ticks", self.0)
    }
}

/// A musical position decomposed into bar, beat and tick.
///
/// All three components are zero-based. The first beat of the first bar is
/// `MusicalTime { bar: 0, beat: 0, tick: 0 }`. Presentation layers that display
/// counting numbers add one; keeping the domain zero-based means arithmetic
/// never needs an off-by-one correction, which is where bar-position bugs
/// normally come from.
///
/// `bar` is signed because positions before the start of a project are
/// meaningful: a transition may begin before the arrangement does, and a
/// count-in is naturally expressed as negative bars.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct MusicalTime {
    /// Bar index, zero-based, may be negative.
    pub bar: i64,
    /// Quarter-note beat within the bar, zero-based.
    pub beat: u32,
    /// Tick within the beat, zero-based, always less than [`TICKS_PER_BEAT`].
    pub tick: u32,
}

impl MusicalTime {
    /// The origin: bar zero, beat zero, tick zero.
    pub const ZERO: Self = Self {
        bar: 0,
        beat: 0,
        tick: 0,
    };

    /// Creates a musical position.
    #[must_use]
    pub const fn new(bar: i64, beat: u32, tick: u32) -> Self {
        Self { bar, beat, tick }
    }

    /// Returns `true` if the position falls exactly on a beat.
    #[must_use]
    pub const fn is_on_beat(self) -> bool {
        self.tick == 0
    }

    /// Returns `true` if the position falls exactly on a bar line.
    #[must_use]
    pub const fn is_on_bar(self) -> bool {
        self.beat == 0 && self.tick == 0
    }
}

impl fmt::Display for MusicalTime {
    /// Formats as `bar.beat.tick` using counting numbers for bar and beat.
    ///
    /// This is the form DJs and producers read, where the downbeat of the first
    /// bar is `1.1.0`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.bar + 1, self.beat + 1, self.tick)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tick_resolution_divides_every_common_subdivision() {
        for divisor in [2_u32, 3, 4, 5, 6, 8, 10, 12, 16, 24, 32, 64, 128] {
            assert_eq!(
                TICKS_PER_BEAT % divisor,
                0,
                "TICKS_PER_BEAT must divide exactly by {divisor}"
            );
        }
    }

    #[test]
    fn whole_note_divides_by_every_supported_denominator() {
        for denominator in [1_u32, 2, 4, 8, 16, 32] {
            assert_eq!(
                TICKS_PER_WHOLE_NOTE % denominator,
                0,
                "whole note must divide exactly by {denominator}"
            );
        }
    }

    #[test]
    fn ticks_arithmetic_saturates() {
        assert_eq!(Ticks::from_beats(2).get(), i64::from(TICKS_PER_BEAT) * 2);
        assert_eq!(
            (Ticks::BEAT + Ticks::BEAT).get(),
            i64::from(TICKS_PER_BEAT) * 2
        );
        assert_eq!((Ticks::BEAT - Ticks::BEAT).get(), 0);
        assert_eq!(Ticks::new(i64::MAX) + Ticks::new(1), Ticks::new(i64::MAX));
    }

    #[test]
    fn display_uses_counting_numbers() {
        assert_eq!(MusicalTime::ZERO.to_string(), "1.1.0");
        assert_eq!(MusicalTime::new(3, 2, 960).to_string(), "4.3.960");
    }

    #[test]
    fn beat_and_bar_predicates() {
        assert!(MusicalTime::ZERO.is_on_beat());
        assert!(MusicalTime::ZERO.is_on_bar());
        assert!(MusicalTime::new(1, 2, 0).is_on_beat());
        assert!(!MusicalTime::new(1, 2, 0).is_on_bar());
        assert!(!MusicalTime::new(1, 0, 5).is_on_beat());
    }
}
