use core::fmt;

use crate::key::{Key, Mode, PitchClass};

/// Which ring of the Camelot wheel a code sits on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Wheel {
    /// The inner ring, written `A`. Minor keys.
    A,
    /// The outer ring, written `B`. Major keys.
    B,
}

impl Wheel {
    /// The other ring.
    #[must_use]
    pub const fn opposite(self) -> Self {
        match self {
            Self::A => Self::B,
            Self::B => Self::A,
        }
    }

    /// The mode this ring represents.
    #[must_use]
    pub const fn mode(self) -> Mode {
        match self {
            Self::A => Mode::Minor,
            Self::B => Mode::Major,
        }
    }
}

impl fmt::Display for Wheel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::A => f.write_str("A"),
            Self::B => f.write_str("B"),
        }
    }
}

/// A position on the Camelot wheel, such as `8A` or `11B`.
///
/// # Why the wheel exists
///
/// Keys a perfect fifth apart share all but one note, so moving between them is
/// the smoothest harmonic move available. The Camelot wheel numbers the twelve
/// keys so that a fifth becomes a step of one, and places each major key
/// opposite its relative minor. The result is that a rule needing music theory
/// to state — "move by a fifth, or to the relative" — becomes "move one number,
/// or change the letter".
///
/// That is exactly the translation Master Prompt #12 asks for: the power of the
/// underlying idea, without requiring the user to hold the theory.
///
/// Numbers run 1 to 12 and wrap, so 12 and 1 are adjacent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CamelotCode {
    number: u8,
    wheel: Wheel,
}

impl CamelotCode {
    /// Creates a code, returning `None` if `number` is outside 1 to 12.
    #[must_use]
    pub const fn new(number: u8, wheel: Wheel) -> Option<Self> {
        if number == 0 || number > 12 {
            return None;
        }
        Some(Self { number, wheel })
    }

    /// The number, from 1 to 12.
    #[must_use]
    pub const fn number(self) -> u8 {
        self.number
    }

    /// The ring.
    #[must_use]
    pub const fn wheel(self) -> Wheel {
        self.wheel
    }

    /// Converts a key to its position on the wheel.
    ///
    /// # How the arithmetic works
    ///
    /// Adjacent numbers are a perfect fifth apart, and a fifth is seven
    /// semitones, so the number advances by one for every seven semitones the
    /// tonic rises. Anchoring at the two codes everyone knows — A minor is 8A
    /// and C major is 8B — gives the two closed forms below, in modular
    /// arithmetic over the twelve pitch classes.
    #[must_use]
    pub const fn from_key(key: Key) -> Self {
        let semitones = key.tonic.semitones() as u16;
        let offset = match key.mode {
            // A minor (semitone 9) must map to 8: 7×9 + 5 = 68 ≡ 8 (mod 12).
            Mode::Minor => 5,
            // C major (semitone 0) must map to 8.
            Mode::Major => 8,
        };
        let position = (semitones * 7 + offset) % 12;
        // Position 0 is the twelfth code, because the wheel counts from one.
        let number = if position == 0 { 12 } else { position as u8 };
        Self {
            number,
            wheel: match key.mode {
                Mode::Minor => Wheel::A,
                Mode::Major => Wheel::B,
            },
        }
    }

    /// Converts a position on the wheel back to a key.
    ///
    /// The inverse of [`Self::from_key`]. Seven is its own inverse modulo
    /// twelve — 7 × 7 = 49 ≡ 1 — so undoing the multiplication is another
    /// multiplication by seven.
    #[must_use]
    pub const fn to_key(self) -> Key {
        let position = (self.number % 12) as u16;
        let offset = match self.wheel {
            Wheel::A => 5,
            Wheel::B => 8,
        };
        // Add 12 before subtracting so the intermediate stays non-negative.
        let semitones = ((position + 12 - offset) * 7) % 12;
        Key::new(
            PitchClass::from_semitones(semitones as u8),
            self.wheel.mode(),
        )
    }

    /// The shortest distance to another code around the wheel, ignoring rings.
    ///
    /// Ranges from 0 to 6, because the wheel is circular: 12 and 1 are one step
    /// apart, not eleven.
    #[must_use]
    pub const fn wheel_distance(self, other: Self) -> u8 {
        let a = self.number;
        let b = other.number;
        let forward = if a <= b { b - a } else { b + 12 - a };
        if forward <= 6 {
            forward
        } else {
            12 - forward
        }
    }

    /// The signed number of steps from this code to another, taking the shorter
    /// direction.
    ///
    /// Positive means clockwise — the direction that raises the key by a fifth,
    /// which is the direction that lifts energy. Ranges from -6 to 6.
    #[must_use]
    #[allow(
        clippy::cast_possible_wrap,
        reason = "both numbers are 1 to 12, so `forward` is 0 to 11 and cannot reach i8::MAX"
    )]
    pub const fn signed_steps(self, other: Self) -> i8 {
        let a = self.number;
        let b = other.number;
        let forward = if a <= b { b - a } else { b + 12 - a };
        if forward <= 6 {
            forward as i8
        } else {
            (forward as i8) - 12
        }
    }

    /// The code `steps` positions clockwise around the wheel, on the same ring.
    #[must_use]
    #[allow(
        clippy::cast_sign_loss,
        reason = "`steps % 12` is -11 to 11, so adding 12 yields 1 to 23: always positive"
    )]
    pub const fn step(self, steps: i8) -> Self {
        // Reduce into 0..12 while staying non-negative.
        let offset = ((steps % 12) + 12) as u8 % 12;
        let raw = (self.number + offset) % 12;
        Self {
            number: if raw == 0 { 12 } else { raw },
            wheel: self.wheel,
        }
    }
}

impl fmt::Display for CamelotCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", self.number, self.wheel)
    }
}

impl From<Key> for CamelotCode {
    fn from(key: Key) -> Self {
        Self::from_key(key)
    }
}

impl From<CamelotCode> for Key {
    fn from(code: CamelotCode) -> Self {
        code.to_key()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The full published wheel, used as ground truth.
    ///
    /// Every one of these pairings is checkable against any Camelot chart, which
    /// is the point: the arithmetic in this module is only trustworthy if it
    /// reproduces the whole table, not just the two anchors it was derived from.
    const WHEEL: [(u8, Wheel, PitchClass, Mode); 24] = [
        (1, Wheel::A, PitchClass::GSharp, Mode::Minor),
        (1, Wheel::B, PitchClass::B, Mode::Major),
        (2, Wheel::A, PitchClass::DSharp, Mode::Minor),
        (2, Wheel::B, PitchClass::FSharp, Mode::Major),
        (3, Wheel::A, PitchClass::ASharp, Mode::Minor),
        (3, Wheel::B, PitchClass::CSharp, Mode::Major),
        (4, Wheel::A, PitchClass::F, Mode::Minor),
        (4, Wheel::B, PitchClass::GSharp, Mode::Major),
        (5, Wheel::A, PitchClass::C, Mode::Minor),
        (5, Wheel::B, PitchClass::DSharp, Mode::Major),
        (6, Wheel::A, PitchClass::G, Mode::Minor),
        (6, Wheel::B, PitchClass::ASharp, Mode::Major),
        (7, Wheel::A, PitchClass::D, Mode::Minor),
        (7, Wheel::B, PitchClass::F, Mode::Major),
        (8, Wheel::A, PitchClass::A, Mode::Minor),
        (8, Wheel::B, PitchClass::C, Mode::Major),
        (9, Wheel::A, PitchClass::E, Mode::Minor),
        (9, Wheel::B, PitchClass::G, Mode::Major),
        (10, Wheel::A, PitchClass::B, Mode::Minor),
        (10, Wheel::B, PitchClass::D, Mode::Major),
        (11, Wheel::A, PitchClass::FSharp, Mode::Minor),
        (11, Wheel::B, PitchClass::A, Mode::Major),
        (12, Wheel::A, PitchClass::CSharp, Mode::Minor),
        (12, Wheel::B, PitchClass::E, Mode::Major),
    ];

    #[test]
    fn every_published_wheel_position_is_reproduced() {
        for (number, wheel, tonic, mode) in WHEEL {
            let key = Key::new(tonic, mode);
            let code = CamelotCode::from_key(key);
            assert_eq!(
                (code.number(), code.wheel()),
                (number, wheel),
                "{key} should be {number}{wheel}, got {code}"
            );
        }
    }

    #[test]
    fn every_published_wheel_position_converts_back() {
        for (number, wheel, tonic, mode) in WHEEL {
            let code = CamelotCode::new(number, wheel);
            assert!(code.is_some());
            if let Some(code) = code {
                assert_eq!(
                    code.to_key(),
                    Key::new(tonic, mode),
                    "{code} should be {tonic} {mode}"
                );
            }
        }
    }

    #[test]
    fn conversion_round_trips_for_all_twenty_four_keys() {
        for tonic in PitchClass::ALL {
            for mode in [Mode::Major, Mode::Minor] {
                let key = Key::new(tonic, mode);
                assert_eq!(CamelotCode::from_key(key).to_key(), key);
            }
        }
    }

    #[test]
    fn relative_keys_share_a_number_and_differ_by_ring() {
        // The defining property of the wheel's layout.
        for tonic in PitchClass::ALL {
            for mode in [Mode::Major, Mode::Minor] {
                let key = Key::new(tonic, mode);
                let code = CamelotCode::from_key(key);
                let relative = CamelotCode::from_key(key.relative());
                assert_eq!(code.number(), relative.number(), "{key} and its relative");
                assert_eq!(relative.wheel(), code.wheel().opposite());
            }
        }
    }

    #[test]
    fn one_step_clockwise_is_a_perfect_fifth() {
        // The other defining property: adjacent numbers are seven semitones
        // apart.
        for tonic in PitchClass::ALL {
            for mode in [Mode::Major, Mode::Minor] {
                let key = Key::new(tonic, mode);
                let next = CamelotCode::from_key(key).step(1).to_key();
                assert_eq!(
                    next.tonic.semitones(),
                    (key.tonic.semitones() + 7) % 12,
                    "one step from {key} should rise a fifth"
                );
                assert_eq!(next.mode, key.mode);
            }
        }
    }

    #[test]
    fn distance_takes_the_short_way_round() {
        let one_a = CamelotCode::new(1, Wheel::A);
        let twelve_a = CamelotCode::new(12, Wheel::A);
        let seven_a = CamelotCode::new(7, Wheel::A);
        assert!(one_a.is_some() && twelve_a.is_some() && seven_a.is_some());
        let (Some(one_a), Some(twelve_a), Some(seven_a)) = (one_a, twelve_a, seven_a) else {
            return;
        };

        assert_eq!(one_a.wheel_distance(twelve_a), 1, "12 and 1 are adjacent");
        assert_eq!(twelve_a.wheel_distance(one_a), 1);
        assert_eq!(one_a.wheel_distance(one_a), 0);
        assert_eq!(
            one_a.wheel_distance(seven_a),
            6,
            "the far side of the wheel"
        );
    }

    #[test]
    fn signed_steps_report_direction() {
        let eight = CamelotCode::new(8, Wheel::A);
        let nine = CamelotCode::new(9, Wheel::A);
        let seven = CamelotCode::new(7, Wheel::A);
        let (Some(eight), Some(nine), Some(seven)) = (eight, nine, seven) else {
            return;
        };

        assert_eq!(eight.signed_steps(nine), 1);
        assert_eq!(eight.signed_steps(seven), -1);
        assert_eq!(eight.signed_steps(eight), 0);
    }

    #[test]
    fn stepping_wraps_in_both_directions() {
        let twelve = CamelotCode::new(12, Wheel::B);
        let Some(twelve) = twelve else { return };
        assert_eq!(twelve.step(1).number(), 1);
        assert_eq!(twelve.step(2).number(), 2);

        let one = CamelotCode::new(1, Wheel::B);
        let Some(one) = one else { return };
        assert_eq!(one.step(-1).number(), 12);
        assert_eq!(one.step(-13).number(), 12);
        assert_eq!(one.step(0).number(), 1);
    }

    #[test]
    fn stepping_stays_on_the_same_ring() {
        let code = CamelotCode::new(5, Wheel::A);
        let Some(code) = code else { return };
        assert_eq!(code.step(3).wheel(), Wheel::A);
        assert_eq!(code.step(-4).wheel(), Wheel::A);
    }

    #[test]
    fn invalid_numbers_are_rejected() {
        assert!(CamelotCode::new(0, Wheel::A).is_none());
        assert!(CamelotCode::new(13, Wheel::A).is_none());
        assert!(CamelotCode::new(255, Wheel::B).is_none());
        assert!(CamelotCode::new(1, Wheel::A).is_some());
        assert!(CamelotCode::new(12, Wheel::B).is_some());
    }

    #[test]
    fn codes_display_in_the_familiar_form() {
        let code = CamelotCode::from_key(Key::minor(PitchClass::A));
        assert_eq!(code.to_string(), "8A");
        let code = CamelotCode::from_key(Key::major(PitchClass::C));
        assert_eq!(code.to_string(), "8B");
    }
}
