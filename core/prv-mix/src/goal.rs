//! What the user asked for, in a form the planner can search against.
//!
//! # The seam between language and decision
//!
//! ADR-0006 divides the system: musical decisions are computed, and language is
//! used to translate intent inward and reasoning outward. A [`Goal`] is the
//! object that division is drawn around. A language model — or the on-device
//! parser that replaces it when cloud AI is disabled — produces one of these,
//! and nothing downstream ever sees the prompt.
//!
//! That is what makes the offline path a real path rather than a degraded one.
//! The planner cannot tell whether its goal came from a fluent sentence or from
//! six taps on a form, because by the time it arrives the difference has been
//! erased. The system loses fluency without cloud AI; it loses no capability.
//!
//! It is also what makes the safety rules enforceable. A goal is a value with a
//! validated range, so "a set that gets more and more intense for three hours"
//! becomes a curve and a duration, and a prompt that cannot be turned into one
//! produces a clarifying question rather than a guess.

use prv_time::Frames;

/// The shape of a set's energy over its length.
///
/// Deliberately a small closed set rather than a free-form curve. These are the
/// shapes a DJ actually plans toward, they are the shapes a listener
/// recognises, and each has a name a user can be shown alongside the graph. A
/// free-form curve would be more expressive and would mostly express mistakes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum EnergyShape {
    /// Rising steadily from beginning to end.
    ///
    /// The warm-up set, and the default when a user says "build".
    Rising,
    /// Rising to a peak around two thirds through, then easing.
    ///
    /// The classic arc of a headline set: the peak lands before the end so the
    /// room is brought down deliberately rather than abandoned at full tilt.
    Arc,
    /// Held at a steady high level.
    ///
    /// A peak-time set that begins where another left off.
    Plateau,
    /// Two peaks with a deliberate dip between them.
    ///
    /// A longer set that needs a second wind, and the shape a room recovers
    /// from best.
    Wave,
    /// Falling steadily.
    ///
    /// The close-down set, and the sunset.
    Falling,
}

impl EnergyShape {
    /// The target energy at a fraction through the set, from zero to one.
    ///
    /// Evaluated rather than tabulated so that the same shape serves a
    /// forty-minute set and a six-hour one without interpolation artefacts at
    /// the joins.
    #[must_use]
    pub fn at(self, progress: f32) -> f32 {
        let t = if progress.is_finite() {
            f64::from(progress.clamp(0.0, 1.0))
        } else {
            0.0
        };
        let value = match self {
            // From a third to full, so a warm-up starts somewhere rather than
            // in silence.
            Self::Rising => 0.35 + 0.65 * t,
            // A peak at two thirds, easing to about three quarters at the end.
            Self::Arc => {
                let peak = 2.0 / 3.0;
                if t <= peak {
                    0.35 + 0.65 * (t / peak)
                } else {
                    1.0 - 0.25 * ((t - peak) / (1.0 - peak))
                }
            }
            Self::Plateau => 0.85,
            // Two peaks with a dip between: a raised cosine at twice the rate.
            Self::Wave => {
                let phase = core::f64::consts::TAU * 2.0 * t;
                0.7 + 0.25 * phase.sin()
            }
            Self::Falling => 0.95 - 0.6 * t,
        };
        crate::num::narrow(value.clamp(0.0, 1.0))
    }

    /// A stable identifier, for storage and localisation.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Rising => "energy.rising",
            Self::Arc => "energy.arc",
            Self::Plateau => "energy.plateau",
            Self::Wave => "energy.wave",
            Self::Falling => "energy.falling",
        }
    }
}

/// How far the planner may depart from established practice.
///
/// # This never relaxes a hard constraint
///
/// Master Prompt #3B's safety rules are guarantees, not tendencies. Creativity
/// widens the *soft* constraints — how much of a tempo jump is acceptable, how
/// unusual a harmonic move may be — and has no effect at all on the rules that
/// make a transition unlistenable. A clashing key is never generated at any
/// setting, and that is enforced in [`crate::transition`] rather than promised
/// here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Creativity {
    /// Only moves a working DJ would make without thinking.
    Conservative,
    /// Established practice, with the occasional bolder choice.
    Balanced,
    /// Willing to take a defensible risk when the payoff is large.
    Adventurous,
}

impl Creativity {
    /// The largest tempo change permitted between two tracks, as a fraction.
    ///
    /// Six per cent is roughly the limit at which a pitch shift stops being
    /// transparent on vocals; twelve is what a DJ will do deliberately with a
    /// filter and an ear on the room.
    #[must_use]
    pub const fn max_tempo_change(self) -> f32 {
        match self {
            Self::Conservative => 0.04,
            Self::Balanced => 0.06,
            Self::Adventurous => 0.12,
        }
    }

    /// Whether harmonically risky moves may be generated.
    ///
    /// Risky is not clashing. `prv-harmony` separates the two precisely so that
    /// this setting has something meaningful to unlock without ever unlocking
    /// something unlistenable.
    #[must_use]
    pub const fn allows_risky_harmony(self) -> bool {
        matches!(self, Self::Balanced | Self::Adventurous)
    }

    /// A stable identifier.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Conservative => "creativity.conservative",
            Self::Balanced => "creativity.balanced",
            Self::Adventurous => "creativity.adventurous",
        }
    }
}

/// What the user asked for.
#[derive(Debug, Clone, PartialEq)]
pub struct Goal {
    duration: Frames,
    shape: EnergyShape,
    creativity: Creativity,
    tempo_floor: Option<f32>,
    tempo_ceiling: Option<f32>,
    weights: crate::transition::Weights,
}

impl Goal {
    /// Creates a goal for a set of a given length and shape.
    #[must_use]
    pub const fn new(duration: Frames, shape: EnergyShape) -> Self {
        Self {
            duration,
            shape,
            creativity: Creativity::Balanced,
            tempo_floor: None,
            tempo_ceiling: None,
            weights: crate::transition::Weights::DEFAULT,
        }
    }

    /// Sets how much each part of a transition counts.
    ///
    /// ADR-0006 puts the weights on the goal because they come from two places
    /// that both belong to the request: the scenario, and the user's learned
    /// profile. A goal built without them uses [`crate::transition::Weights::DEFAULT`],
    /// so a system that has learned nothing behaves exactly as one with no
    /// learning at all — which is what makes it safe to ship learning switched
    /// on from the first day.
    #[must_use]
    pub const fn with_weights(mut self, weights: crate::transition::Weights) -> Self {
        self.weights = weights;
        self
    }

    /// How much each part of a transition counts.
    #[must_use]
    pub const fn weights(&self) -> crate::transition::Weights {
        self.weights
    }

    /// Sets how far the planner may depart from established practice.
    #[must_use]
    pub const fn with_creativity(mut self, creativity: Creativity) -> Self {
        self.creativity = creativity;
        self
    }

    /// Restricts the set to a tempo range, in beats per minute.
    ///
    /// A user asking for "house, nothing over 128" is expressing a hard
    /// constraint, not a preference, so it is one.
    #[must_use]
    pub fn with_tempo_range(mut self, floor: f32, ceiling: f32) -> Self {
        let (low, high) = if floor <= ceiling {
            (floor, ceiling)
        } else {
            (ceiling, floor)
        };
        self.tempo_floor = Some(low);
        self.tempo_ceiling = Some(high);
        self
    }

    /// The target length.
    #[must_use]
    pub const fn duration(&self) -> Frames {
        self.duration
    }

    /// The energy shape.
    #[must_use]
    pub const fn shape(&self) -> EnergyShape {
        self.shape
    }

    /// The creativity setting.
    #[must_use]
    pub const fn creativity(&self) -> Creativity {
        self.creativity
    }

    /// The tempo floor, if one was set.
    #[must_use]
    pub const fn tempo_floor(&self) -> Option<f32> {
        self.tempo_floor
    }

    /// The tempo ceiling, if one was set.
    #[must_use]
    pub const fn tempo_ceiling(&self) -> Option<f32> {
        self.tempo_ceiling
    }

    /// The energy the set should be at, a given distance in.
    #[must_use]
    pub fn target_energy(&self, elapsed: Frames) -> f32 {
        let total = self.duration.get();
        if total <= 0 {
            return self.shape.at(0.0);
        }
        let progress = crate::num::narrow(
            crate::num::signed_to_f64(elapsed.get()) / crate::num::signed_to_f64(total),
        );
        self.shape.at(progress)
    }
}

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
    fn every_shape_stays_inside_the_range_across_its_whole_length() {
        // A shape that left the range would corrupt every energy score derived
        // from it, and the arithmetic that produces `Wave` in particular is
        // easy to get wrong by a factor that only shows at one point.
        for shape in [
            EnergyShape::Rising,
            EnergyShape::Arc,
            EnergyShape::Plateau,
            EnergyShape::Wave,
            EnergyShape::Falling,
        ] {
            let mut step = 0;
            while step <= 1000 {
                let progress = crate::num::narrow(f64::from(step) / 1000.0);
                let value = shape.at(progress);
                assert!(
                    (0.0..=1.0).contains(&value),
                    "{shape:?} at {progress} gave {value}"
                );
                step += 1;
            }
        }
    }

    #[test]
    fn the_shapes_do_what_their_names_say() {
        assert!(EnergyShape::Rising.at(0.0) < EnergyShape::Rising.at(1.0));
        assert!(EnergyShape::Falling.at(0.0) > EnergyShape::Falling.at(1.0));
        // The arc peaks before the end, which is the whole point of it: the
        // room is brought down deliberately rather than abandoned at full tilt.
        let peak = EnergyShape::Arc.at(2.0 / 3.0);
        assert!(peak > EnergyShape::Arc.at(0.0));
        assert!(peak > EnergyShape::Arc.at(1.0));
        // The plateau is flat.
        assert_eq!(EnergyShape::Plateau.at(0.1), EnergyShape::Plateau.at(0.9));
        // The wave dips in the middle of each half.
        assert!(EnergyShape::Wave.at(0.375) < EnergyShape::Wave.at(0.125));
    }

    #[test]
    fn out_of_range_progress_is_clamped_rather_than_extrapolated() {
        for shape in [EnergyShape::Rising, EnergyShape::Arc, EnergyShape::Falling] {
            assert_eq!(shape.at(-1.0), shape.at(0.0));
            assert_eq!(shape.at(2.0), shape.at(1.0));
            assert_eq!(shape.at(f32::NAN), shape.at(0.0));
        }
    }

    #[test]
    fn creativity_widens_soft_limits_monotonically() {
        assert!(
            Creativity::Conservative.max_tempo_change() < Creativity::Balanced.max_tempo_change()
        );
        assert!(
            Creativity::Balanced.max_tempo_change() < Creativity::Adventurous.max_tempo_change()
        );
        assert!(!Creativity::Conservative.allows_risky_harmony());
        assert!(Creativity::Adventurous.allows_risky_harmony());
    }

    #[test]
    fn a_reversed_tempo_range_is_corrected_rather_than_rejected() {
        // A user typing 128 and then 120 means the same thing either way round,
        // and refusing the goal over it would be pedantry the interface has to
        // apologise for.
        let goal = Goal::new(Frames::new(1000), EnergyShape::Arc).with_tempo_range(128.0, 120.0);
        assert_eq!(goal.tempo_floor(), Some(120.0));
        assert_eq!(goal.tempo_ceiling(), Some(128.0));
    }

    #[test]
    fn target_energy_follows_the_shape_across_the_set() {
        let goal = Goal::new(Frames::new(1_000_000), EnergyShape::Rising);
        assert!(goal.target_energy(Frames::ZERO) < goal.target_energy(Frames::new(500_000)));
        assert!(
            goal.target_energy(Frames::new(500_000)) < goal.target_energy(Frames::new(1_000_000))
        );
    }

    #[test]
    fn a_zero_length_goal_does_not_divide_by_zero() {
        let goal = Goal::new(Frames::ZERO, EnergyShape::Arc);
        let value = goal.target_energy(Frames::new(100));
        assert!(value.is_finite());
    }

    #[test]
    fn keys_are_distinct() {
        let shapes = [
            EnergyShape::Rising.key(),
            EnergyShape::Arc.key(),
            EnergyShape::Plateau.key(),
            EnergyShape::Wave.key(),
            EnergyShape::Falling.key(),
        ];
        for (index, key) in shapes.iter().enumerate() {
            for (other, value) in shapes.iter().enumerate() {
                assert!(index == other || key != value, "two shapes share {key}");
            }
        }
    }
}
