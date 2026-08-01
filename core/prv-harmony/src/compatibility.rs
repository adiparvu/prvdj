use core::fmt;

use crate::camelot::CamelotCode;
use crate::key::Key;

/// How two keys relate to one another.
///
/// This is the evidence, in the sense ADR-0006 uses the word: a fact about the
/// music from which an explanation is derived, rather than a narrative composed
/// after a decision was made. The relation stays true however the planner's
/// weights are tuned, which is what allows the explanation shown to the user to
/// stay true as well.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum KeyRelation {
    /// The same key.
    Identical,

    /// Relative major and minor — the same notes, a different centre.
    ///
    /// A minor and C major. Interchangeable in a mix because they contain
    /// exactly the same pitches.
    Relative,

    /// A perfect fifth up: one step clockwise, same ring.
    ///
    /// The classic lift. The two keys share all but one note, and the direction
    /// is the one listeners hear as rising.
    Dominant,

    /// A perfect fourth up: one step anticlockwise, same ring.
    ///
    /// As smooth as the dominant and heard as settling rather than lifting.
    Subdominant,

    /// One step clockwise with a change of ring.
    ///
    /// A diagonal move. Changes both the centre and the colour at once — more
    /// noticeable than a plain fifth, and useful when the mood should shift.
    DiagonalUp,

    /// One step anticlockwise with a change of ring.
    DiagonalDown,

    /// Two steps clockwise, same ring — a whole tone up.
    ///
    /// The energy lift DJs reach for when a set needs to climb. Audible as a
    /// deliberate move rather than a seamless one, which is often the point.
    EnergyLift,

    /// The same tonic in the other mode.
    ///
    /// A minor and A major. Shares a centre but changes the colour completely;
    /// effective on a breakdown, risky under sustained melodic content.
    Parallel,

    /// Everything else.
    ///
    /// Carries the distance so that the planner and the explanation can say how
    /// far apart the keys are rather than only that they do not fit.
    Distant {
        /// Shortest distance around the wheel, from 2 to 6.
        wheel_distance: u8,
    },
}

impl fmt::Display for KeyRelation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Identical => f.write_str("the same key"),
            Self::Relative => f.write_str("relative major and minor"),
            Self::Dominant => f.write_str("a perfect fifth up"),
            Self::Subdominant => f.write_str("a perfect fourth up"),
            Self::DiagonalUp => f.write_str("a fifth up with a change of mode"),
            Self::DiagonalDown => f.write_str("a fourth up with a change of mode"),
            Self::EnergyLift => f.write_str("a whole tone up"),
            Self::Parallel => f.write_str("the same tonic in the other mode"),
            Self::Distant { wheel_distance } => {
                write!(f, "{wheel_distance} steps apart on the wheel")
            }
        }
    }
}

/// Whether a harmonic move is permitted, discouraged, or forbidden.
///
/// Master Prompt #3B lists "clashing keys" among the things the system must
/// **never** create. ADR-0006 implements that as a hard constraint rather than a
/// low score: a candidate classified [`HarmonicSafety::Clashing`] is removed
/// from the candidate set, not ranked below the others. That is the difference
/// between a guarantee and a tendency.
///
/// [`HarmonicSafety::Risky`] moves remain available and are what the creativity
/// setting of Master Prompt #3B unlocks. Raising creativity relaxes soft
/// constraints; it never relaxes the hard ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum HarmonicSafety {
    /// Established practice. Available at every creativity setting.
    Safe,
    /// Musically defensible but noticeable. Unlocked by higher creativity, or by
    /// a transition that mitigates it — filtering the bass, or moving during a
    /// breakdown where little melodic content is exposed.
    Risky,
    /// A clash. Never generated, at any creativity setting.
    Clashing,
}

/// The result of comparing two keys.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HarmonicCompatibility {
    /// How the keys relate. The stable, explainable part.
    pub relation: KeyRelation,
    /// Whether the move is permitted, discouraged or forbidden.
    pub safety: HarmonicSafety,
    /// A score from 0.0 to 1.0, higher being smoother.
    ///
    /// **Provisional.** These weights encode established harmonic mixing
    /// practice and are a starting point, not a calibrated model. ADR-0006
    /// schedules calibration against listener evaluation in Phase 3. Treat the
    /// ordering as meaningful and the absolute values as subject to change; the
    /// relation, not the number, is what explanations are built from.
    pub score: f32,
}

impl HarmonicCompatibility {
    /// Returns `true` if the move is never to be generated.
    #[must_use]
    pub const fn is_clashing(&self) -> bool {
        matches!(self.safety, HarmonicSafety::Clashing)
    }
}

impl fmt::Display for HarmonicCompatibility {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} (score {:.2})", self.relation, self.score)
    }
}

/// Classifies and scores the harmonic move from `from` to `to`.
///
/// Direction matters: a fifth up and a fourth up are different moves with
/// different effects, so `compatibility(a, b)` and `compatibility(b, a)` may
/// differ in relation while agreeing on distance.
///
/// # Example
///
/// ```
/// use prv_harmony::{compatibility, HarmonicSafety, Key, KeyRelation, PitchClass};
///
/// // A minor to C major: the same notes, a different centre.
/// let result = compatibility(Key::minor(PitchClass::A), Key::major(PitchClass::C));
/// assert_eq!(result.relation, KeyRelation::Relative);
/// assert_eq!(result.safety, HarmonicSafety::Safe);
/// ```
#[must_use]
pub fn compatibility(from: Key, to: Key) -> HarmonicCompatibility {
    let relation = classify(from, to);
    let (safety, score) = weigh(relation);
    HarmonicCompatibility {
        relation,
        safety,
        score,
    }
}

/// Determines the relation between two keys.
fn classify(from: Key, to: Key) -> KeyRelation {
    if from == to {
        return KeyRelation::Identical;
    }
    if from.parallel() == to {
        return KeyRelation::Parallel;
    }

    let from_code = CamelotCode::from_key(from);
    let to_code = CamelotCode::from_key(to);
    let same_ring = from_code.wheel() == to_code.wheel();
    let steps = from_code.signed_steps(to_code);

    match (same_ring, steps) {
        (true, 0) => KeyRelation::Identical,
        (false, 0) => KeyRelation::Relative,
        (true, 1) => KeyRelation::Dominant,
        (true, -1) => KeyRelation::Subdominant,
        (false, 1) => KeyRelation::DiagonalUp,
        (false, -1) => KeyRelation::DiagonalDown,
        (true, 2) => KeyRelation::EnergyLift,
        _ => KeyRelation::Distant {
            wheel_distance: from_code.wheel_distance(to_code),
        },
    }
}

/// Assigns safety and score to a relation.
///
/// Kept separate from [`classify`] so that recalibrating the weights cannot
/// change what the system claims about the music — only how strongly it prefers
/// one move over another.
const fn weigh(relation: KeyRelation) -> (HarmonicSafety, f32) {
    match relation {
        KeyRelation::Identical => (HarmonicSafety::Safe, 1.00),
        KeyRelation::Relative => (HarmonicSafety::Safe, 0.95),
        KeyRelation::Dominant => (HarmonicSafety::Safe, 0.90),
        KeyRelation::Subdominant => (HarmonicSafety::Safe, 0.88),
        KeyRelation::DiagonalUp => (HarmonicSafety::Safe, 0.74),
        KeyRelation::DiagonalDown => (HarmonicSafety::Safe, 0.70),
        KeyRelation::EnergyLift => (HarmonicSafety::Risky, 0.62),
        KeyRelation::Parallel => (HarmonicSafety::Risky, 0.58),
        // Two steps is audibly a different key but still usable under a
        // filtered or percussive transition. Beyond that the tonal centres
        // conflict outright, which is the clash Master Prompt #3B forbids.
        KeyRelation::Distant { wheel_distance: 2 } => (HarmonicSafety::Risky, 0.40),
        KeyRelation::Distant { wheel_distance: 3 } => (HarmonicSafety::Clashing, 0.22),
        KeyRelation::Distant { wheel_distance: 4 } => (HarmonicSafety::Clashing, 0.14),
        KeyRelation::Distant { wheel_distance: 5 } => (HarmonicSafety::Clashing, 0.08),
        KeyRelation::Distant { .. } => (HarmonicSafety::Clashing, 0.05),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::key::{Mode, PitchClass};

    #[test]
    fn a_key_is_perfectly_compatible_with_itself() {
        for tonic in PitchClass::ALL {
            for mode in [Mode::Major, Mode::Minor] {
                let key = Key::new(tonic, mode);
                let result = compatibility(key, key);
                assert_eq!(result.relation, KeyRelation::Identical);
                assert_eq!(result.safety, HarmonicSafety::Safe);
                assert!((result.score - 1.0).abs() < f32::EPSILON);
            }
        }
    }

    #[test]
    fn relative_keys_are_recognised_in_both_directions() {
        let a_minor = Key::minor(PitchClass::A);
        let c_major = Key::major(PitchClass::C);
        assert_eq!(
            compatibility(a_minor, c_major).relation,
            KeyRelation::Relative
        );
        assert_eq!(
            compatibility(c_major, a_minor).relation,
            KeyRelation::Relative
        );
    }

    #[test]
    fn a_fifth_up_is_the_dominant_and_a_fourth_up_is_the_subdominant() {
        let a_minor = Key::minor(PitchClass::A);
        let e_minor = Key::minor(PitchClass::E); // a fifth above A
        let d_minor = Key::minor(PitchClass::D); // a fourth above A

        assert_eq!(
            compatibility(a_minor, e_minor).relation,
            KeyRelation::Dominant
        );
        assert_eq!(
            compatibility(a_minor, d_minor).relation,
            KeyRelation::Subdominant
        );
    }

    #[test]
    fn direction_is_preserved_rather_than_collapsed() {
        // A fifth up and a fourth up are different musical moves. A model that
        // reported only distance would lose the distinction, and with it the
        // ability to explain why one lifts and the other settles.
        let a_minor = Key::minor(PitchClass::A);
        let e_minor = Key::minor(PitchClass::E);
        assert_ne!(
            compatibility(a_minor, e_minor).relation,
            compatibility(e_minor, a_minor).relation
        );
    }

    #[test]
    fn parallel_keys_are_recognised() {
        let a_minor = Key::minor(PitchClass::A);
        let a_major = Key::major(PitchClass::A);
        let result = compatibility(a_minor, a_major);
        assert_eq!(result.relation, KeyRelation::Parallel);
        assert_eq!(result.safety, HarmonicSafety::Risky);
    }

    #[test]
    fn a_whole_tone_up_is_an_energy_lift() {
        // 8A to 10A: A minor to B minor.
        let a_minor = Key::minor(PitchClass::A);
        let b_minor = Key::minor(PitchClass::B);
        assert_eq!(
            compatibility(a_minor, b_minor).relation,
            KeyRelation::EnergyLift
        );
    }

    #[test]
    fn opposite_sides_of_the_wheel_clash() {
        // 8A to 2A is the maximum distance: six steps.
        let a_minor = Key::minor(PitchClass::A);
        let d_sharp_minor = Key::minor(PitchClass::DSharp);
        let result = compatibility(a_minor, d_sharp_minor);
        assert_eq!(result.relation, KeyRelation::Distant { wheel_distance: 6 });
        assert!(result.is_clashing());
    }

    #[test]
    fn every_safe_relation_outscores_every_risky_one() {
        // The ordering must be internally consistent, otherwise the planner
        // could prefer a discouraged move to a permitted one.
        let mut worst_safe = 1.0_f32;
        let mut best_risky = 0.0_f32;
        let mut best_clashing = 0.0_f32;

        for from_tonic in PitchClass::ALL {
            for to_tonic in PitchClass::ALL {
                for from_mode in [Mode::Major, Mode::Minor] {
                    for to_mode in [Mode::Major, Mode::Minor] {
                        let result = compatibility(
                            Key::new(from_tonic, from_mode),
                            Key::new(to_tonic, to_mode),
                        );
                        match result.safety {
                            HarmonicSafety::Safe => worst_safe = worst_safe.min(result.score),
                            HarmonicSafety::Risky => best_risky = best_risky.max(result.score),
                            HarmonicSafety::Clashing => {
                                best_clashing = best_clashing.max(result.score);
                            }
                        }
                    }
                }
            }
        }

        assert!(
            worst_safe > best_risky,
            "the worst safe move ({worst_safe}) must outscore the best risky one ({best_risky})"
        );
        assert!(
            best_risky > best_clashing,
            "the best risky move ({best_risky}) must outscore the best clashing one ({best_clashing})"
        );
    }

    #[test]
    fn scores_stay_within_range_for_every_pair() {
        for from_tonic in PitchClass::ALL {
            for to_tonic in PitchClass::ALL {
                for from_mode in [Mode::Major, Mode::Minor] {
                    for to_mode in [Mode::Major, Mode::Minor] {
                        let result = compatibility(
                            Key::new(from_tonic, from_mode),
                            Key::new(to_tonic, to_mode),
                        );
                        assert!(
                            (0.0..=1.0).contains(&result.score),
                            "score {} out of range",
                            result.score
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn every_key_has_at_least_six_safe_destinations() {
        // A guarantee the planner depends on: the hard constraint must never
        // leave a track with nowhere to go. Each key has itself, its relative,
        // its dominant and subdominant, and two diagonals.
        for tonic in PitchClass::ALL {
            for mode in [Mode::Major, Mode::Minor] {
                let from = Key::new(tonic, mode);
                let mut safe = 0;
                for to_tonic in PitchClass::ALL {
                    for to_mode in [Mode::Major, Mode::Minor] {
                        let result = compatibility(from, Key::new(to_tonic, to_mode));
                        if result.safety == HarmonicSafety::Safe {
                            safe += 1;
                        }
                    }
                }
                assert!(
                    safe >= 6,
                    "{from} has only {safe} safe destinations; the hard constraint \
                     must never strand a track"
                );
            }
        }
    }

    #[test]
    fn relations_read_as_plain_language() {
        // Master Prompt #12 requires jargon to be translated rather than
        // displayed. These strings are the source for that translation.
        assert_eq!(
            KeyRelation::Relative.to_string(),
            "relative major and minor"
        );
        assert_eq!(KeyRelation::Dominant.to_string(), "a perfect fifth up");
        assert_eq!(
            KeyRelation::Distant { wheel_distance: 4 }.to_string(),
            "4 steps apart on the wheel"
        );
    }
}
