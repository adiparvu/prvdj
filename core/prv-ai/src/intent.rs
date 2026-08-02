//! What the user asked for, once language has been left behind.
//!
//! # This is the inward half of ADR-0006's seam
//!
//! A language model — or the on-device parser that replaces it when cloud AI is
//! switched off — produces one of these, and nothing downstream ever sees the
//! prompt. By the time an intent reaches the planner, the difference between a
//! fluent sentence and six taps on a form has been erased.
//!
//! # A model cannot emit a value the system will act on unchecked
//!
//! Every field is bounded here, at the boundary, once. The reason is not
//! defensive programming in general: it is that the thing on the other side of
//! this boundary is a model whose output is *plausible* rather than *correct*,
//! and a plausible three-hundred-hour set or a tempo floor above its ceiling
//! would otherwise become a real object inside a system that assumes its values
//! are sane.
//!
//! The test that matters walks extreme and adversarial values through
//! [`Intent::to_goal`] and asserts that every one of them either produces a goal
//! inside the planner's own limits or produces an error naming the field. There
//! is no third outcome.

use core::fmt;

use prv_mix::{Creativity, EnergyShape, Goal};
use prv_time::Frames;

/// What a user can ask the system to do.
///
/// Closed, and short. Master Prompt #19 asks for an orchestrator over several
/// capabilities, not for an interpreter that will attempt anything phrased
/// confidently; a request that does not fit one of these is a clarifying
/// question, which is a better outcome than a confident guess.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Intent {
    /// Plan a set.
    PlanSet {
        /// How long, in minutes.
        minutes: u32,
        /// The shape of its energy.
        shape: EnergyShape,
        /// How far from the safe answer the planner may go.
        creativity: Creativity,
        /// The slowest tempo to consider, in beats per minute.
        tempo_floor: Option<f32>,
        /// The fastest tempo to consider, in beats per minute.
        tempo_ceiling: Option<f32>,
    },

    /// Analyse whatever in the library has not been analysed.
    AnalyseLibrary,

    /// Say why something was chosen.
    ///
    /// Carries no free text: an explanation is generated *from the evidence the
    /// planner retained*, not from a question. ADR-0006's outward half.
    ExplainChoice {
        /// Which placement in the set.
        placement: u64,
    },

    /// Offer different ways to get from one record to the next.
    SuggestTransition {
        /// Which placement the transition leaves.
        from_placement: u64,
    },

    /// Prepare a delivery report for the current project.
    PrepareExport,
}

/// Why an intent could not be honoured.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum IntentError {
    /// A set of that length is not something the planner will attempt.
    DurationOutOfRange {
        /// What was asked for.
        minutes: u32,
        /// The shortest accepted.
        low: u32,
        /// The longest accepted.
        high: u32,
    },
    /// A tempo bound was not a tempo.
    TempoOutOfRange {
        /// The lowest accepted, in beats per minute.
        low: f32,
        /// The highest accepted.
        high: f32,
    },
    /// The floor was above the ceiling.
    TempoRangeInverted,
    /// The intent does not produce a goal at all.
    ///
    /// Not every intent is a planning request, and asking one for a goal is a
    /// programming error rather than a user error — which is why it is a
    /// distinct variant rather than a generic failure.
    NotAPlanningIntent,
}

impl fmt::Display for IntentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::DurationOutOfRange { minutes, low, high } => {
                write!(f, "a set of {minutes} minutes is outside {low} to {high}")
            }
            Self::TempoOutOfRange { low, high } => {
                write!(f, "a tempo bound must be between {low} and {high}")
            }
            Self::TempoRangeInverted => f.write_str("the tempo floor is above the ceiling"),
            Self::NotAPlanningIntent => f.write_str("this intent does not describe a set"),
        }
    }
}

impl core::error::Error for IntentError {}

impl Intent {
    /// The shortest set the planner will attempt, in minutes.
    ///
    /// Ten. Below that there is no shape to plan — two records and a transition
    /// is not a set, and pretending to arrange one would be theatre.
    pub const MIN_MINUTES: u32 = 10;

    /// The longest, in minutes.
    ///
    /// Twelve hours. Longer than any real set and short enough that the beam
    /// search's cost stays bounded; a request beyond it is a mistake or a model
    /// hallucinating a number, and both deserve the same answer.
    pub const MAX_MINUTES: u32 = 720;

    /// The slowest tempo accepted as a bound, in beats per minute.
    ///
    /// Held as a float because that is what a tempo is compared against; the
    /// alternative is a conversion at every comparison, which is where a
    /// rounding difference between two call sites comes from.
    pub const MIN_TEMPO: f32 = 40.0;

    /// The fastest.
    pub const MAX_TEMPO: f32 = 220.0;

    /// Turns a planning intent into the planner's own goal.
    ///
    /// # Errors
    ///
    /// Returns [`IntentError`] naming the field that was wrong, so that a
    /// clarifying question can be about the thing that needs clarifying.
    pub fn to_goal(&self, sample_rate: u32) -> Result<Goal, IntentError> {
        let Self::PlanSet {
            minutes,
            shape,
            creativity,
            tempo_floor,
            tempo_ceiling,
        } = *self
        else {
            return Err(IntentError::NotAPlanningIntent);
        };

        if !(Self::MIN_MINUTES..=Self::MAX_MINUTES).contains(&minutes) {
            return Err(IntentError::DurationOutOfRange {
                minutes,
                low: Self::MIN_MINUTES,
                high: Self::MAX_MINUTES,
            });
        }

        for bound in [tempo_floor, tempo_ceiling].into_iter().flatten() {
            if !is_a_tempo(bound) {
                return Err(IntentError::TempoOutOfRange {
                    low: Self::MIN_TEMPO,
                    high: Self::MAX_TEMPO,
                });
            }
        }
        if let (Some(floor), Some(ceiling)) = (tempo_floor, tempo_ceiling) {
            if floor > ceiling {
                return Err(IntentError::TempoRangeInverted);
            }
        }

        let frames = i64::from(minutes)
            .saturating_mul(60)
            .saturating_mul(i64::from(sample_rate));
        let mut goal = Goal::new(Frames::new(frames), shape).with_creativity(creativity);
        if let (Some(floor), Some(ceiling)) = (tempo_floor, tempo_ceiling) {
            goal = goal.with_tempo_range(floor, ceiling);
        }
        Ok(goal)
    }

    /// Whether this intent describes a set to plan.
    #[must_use]
    pub const fn is_planning(&self) -> bool {
        matches!(self, Self::PlanSet { .. })
    }

    /// A stable identifier, for storage and localisation.
    #[must_use]
    pub const fn key(&self) -> &'static str {
        match self {
            Self::PlanSet { .. } => "intent.plan_set",
            Self::AnalyseLibrary => "intent.analyse_library",
            Self::ExplainChoice { .. } => "intent.explain_choice",
            Self::SuggestTransition { .. } => "intent.suggest_transition",
            Self::PrepareExport => "intent.prepare_export",
        }
    }
}

/// Whether a number offered as a tempo is one.
///
/// Rejects the values a model reaches for when it is guessing — nothing, an
/// infinity, a negative — as well as the ones that are merely wrong.
fn is_a_tempo(value: f32) -> bool {
    value.is_finite() && (Intent::MIN_TEMPO..=Intent::MAX_TEMPO).contains(&value)
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::panic,
        clippy::float_cmp,
        reason = "fixtures are exact; a test that cannot build one should fail loudly"
    )]

    use super::*;

    const RATE: u32 = 48_000;

    fn plan(minutes: u32, floor: Option<f32>, ceiling: Option<f32>) -> Intent {
        Intent::PlanSet {
            minutes,
            shape: EnergyShape::Arc,
            creativity: Creativity::Balanced,
            tempo_floor: floor,
            tempo_ceiling: ceiling,
        }
    }

    #[test]
    fn nothing_a_model_can_emit_becomes_a_goal_the_planner_would_not_accept() {
        // The property this module exists for. The thing on the other side of
        // this boundary produces output that is plausible rather than correct,
        // and there is no third outcome here: a goal inside the limits, or an
        // error naming the field.
        let durations = [
            0,
            1,
            Intent::MIN_MINUTES - 1,
            Intent::MIN_MINUTES,
            60,
            Intent::MAX_MINUTES,
            Intent::MAX_MINUTES + 1,
            100_000,
            u32::MAX,
        ];
        let tempos = [
            None,
            Some(f32::NAN),
            Some(f32::INFINITY),
            Some(f32::NEG_INFINITY),
            Some(-120.0),
            Some(0.0),
            Some(1.0),
            Some(128.0),
            Some(1_000_000.0),
        ];

        for minutes in durations {
            for floor in tempos {
                for ceiling in tempos {
                    match plan(minutes, floor, ceiling).to_goal(RATE) {
                        Ok(goal) => {
                            let frames = goal.duration().get();
                            assert!(
                                frames > 0,
                                "{minutes} minutes produced a set of {frames} frames"
                            );
                            assert!(
                                (Intent::MIN_MINUTES..=Intent::MAX_MINUTES).contains(&minutes),
                                "{minutes} minutes should not have produced a goal"
                            );
                            if let Some(bound) = goal.tempo_floor() {
                                assert!(is_a_tempo(bound), "a goal carries a tempo of {bound}");
                            }
                            if let Some(bound) = goal.tempo_ceiling() {
                                assert!(is_a_tempo(bound), "a goal carries a tempo of {bound}");
                            }
                            if let (Some(low), Some(high)) =
                                (goal.tempo_floor(), goal.tempo_ceiling())
                            {
                                assert!(low <= high, "a goal carries an inverted tempo range");
                            }
                        }
                        Err(error) => {
                            // Every refusal names something. A bare "invalid"
                            // would make the clarifying question unanswerable.
                            assert!(!error.to_string().is_empty());
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn a_set_too_short_to_have_a_shape_is_refused() {
        // Two records and a transition is not a set, and pretending to arrange
        // one would be theatre.
        assert_eq!(
            plan(Intent::MIN_MINUTES - 1, None, None)
                .to_goal(RATE)
                .err(),
            Some(IntentError::DurationOutOfRange {
                minutes: Intent::MIN_MINUTES - 1,
                low: Intent::MIN_MINUTES,
                high: Intent::MAX_MINUTES,
            })
        );
        assert!(plan(Intent::MIN_MINUTES, None, None).to_goal(RATE).is_ok());
    }

    #[test]
    fn an_inverted_tempo_range_is_its_own_error() {
        // Distinct from an out-of-range bound because the remedy is different:
        // one value is wrong, or two values are the wrong way round.
        assert_eq!(
            plan(60, Some(140.0), Some(120.0)).to_goal(RATE).err(),
            Some(IntentError::TempoRangeInverted)
        );
        assert!(plan(60, Some(120.0), Some(140.0)).to_goal(RATE).is_ok());
        assert!(
            plan(60, Some(128.0), Some(128.0)).to_goal(RATE).is_ok(),
            "a single-tempo set is a real request"
        );
    }

    #[test]
    fn one_tempo_bound_on_its_own_is_kept_out_of_the_goal() {
        // The planner takes a range or nothing. Half a range would have to be
        // completed with a number nobody chose, which is the kind of invented
        // value this whole module exists to prevent.
        let goal = plan(60, Some(120.0), None)
            .to_goal(RATE)
            .expect("a valid duration");
        assert_eq!(goal.tempo_floor(), None);
        assert_eq!(goal.tempo_ceiling(), None);
    }

    #[test]
    fn the_duration_survives_the_conversion_exactly() {
        let goal = plan(90, None, None).to_goal(RATE).expect("valid");
        assert_eq!(goal.duration().get(), 90 * 60 * i64::from(RATE));

        let at_other_rate = plan(90, None, None).to_goal(44_100).expect("valid");
        assert_eq!(at_other_rate.duration().get(), 90 * 60 * 44_100);
    }

    #[test]
    fn asking_a_non_planning_intent_for_a_goal_says_what_went_wrong() {
        for intent in [
            Intent::AnalyseLibrary,
            Intent::ExplainChoice { placement: 1 },
            Intent::SuggestTransition { from_placement: 1 },
            Intent::PrepareExport,
        ] {
            assert!(!intent.is_planning());
            assert_eq!(
                intent.to_goal(RATE).err(),
                Some(IntentError::NotAPlanningIntent),
                "{}",
                intent.key()
            );
        }
        assert!(plan(60, None, None).is_planning());
    }

    #[test]
    fn intent_keys_are_distinct() {
        let keys = [
            plan(60, None, None).key(),
            Intent::AnalyseLibrary.key(),
            Intent::ExplainChoice { placement: 1 }.key(),
            Intent::SuggestTransition { from_placement: 1 }.key(),
            Intent::PrepareExport.key(),
        ];
        for (index, key) in keys.iter().enumerate() {
            for (other, value) in keys.iter().enumerate() {
                assert!(index == other || key != value, "two intents share {key}");
            }
        }
    }
}
