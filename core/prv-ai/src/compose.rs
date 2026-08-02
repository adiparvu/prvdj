//! Turning a request into a list of things to do.
//!
//! # The situation is a parameter, never a field
//!
//! Composing a plan needs to know things this crate has no business owning: how
//! many tracks are waiting to be analysed, what this machine can do, whether
//! audio is going out to a room. All three are passed in.
//!
//! That is not fastidiousness. A copy of the library's state held here would be
//! stale the moment a track finished importing, and the plans built from it
//! would be confidently wrong in the direction that is hardest to notice — a set
//! planned over music the system thought was analysed and was not.
//!
//! # A plan contains only what the request actually needs
//!
//! Analysis appears in a planning plan only when something is unanalysed. An
//! empty request produces an empty plan rather than a plan of no-ops, because a
//! caller that has to distinguish "nothing to do" from "one step that does
//! nothing" will eventually get it wrong, and the honest answer to "analyse my
//! library" when everything is analysed is *nothing*.

use crate::agent::{Capability, Device};
use crate::intent::Intent;
use crate::task::{Task, TaskError, TaskId, TaskPlan};

/// What the rest of the system is doing right now.
///
/// Supplied by the caller at the moment a plan is composed, and not stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Situation {
    /// How many tracks in the library have no usable analysis.
    pub unanalysed_tracks: u32,
    /// Whether the set being worked on has anything in it yet.
    pub set_has_placements: bool,
    /// What this machine can do for itself.
    pub device: Device,
}

impl Situation {
    /// A machine that can do everything, a library that is fully analysed, and
    /// an empty set.
    #[must_use]
    pub const fn settled() -> Self {
        Self {
            unanalysed_tracks: 0,
            set_has_placements: false,
            device: Device::capable(),
        }
    }

    /// Whether anything needs analysing before a set can be planned over it.
    #[must_use]
    pub const fn needs_analysis(&self) -> bool {
        self.unanalysed_tracks > 0
    }
}

/// Why a request could not be turned into a plan.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ComposeError {
    /// The request refers to a set that has nothing in it.
    ///
    /// Explaining a choice that was never made, or offering a transition out of
    /// a record that is not there, is not a failure of the system — it is a
    /// request about something that does not exist, and saying so is more
    /// useful than producing an empty answer.
    NothingToWorkOn,

    /// The plan could not be assembled.
    ///
    /// Not reachable from any request this module composes: the identifiers are
    /// distinct by construction and the plans are five steps long against a
    /// limit of a thousand. It exists because the alternative is discarding the
    /// result of every `add`, and a discarded result is how a real failure
    /// becomes a plan that is quietly missing a step.
    CouldNotAssemble(TaskError),
}

impl core::fmt::Display for ComposeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NothingToWorkOn => f.write_str("there is nothing in the set to work on"),
            Self::CouldNotAssemble(error) => write!(f, "{error}"),
        }
    }
}

impl core::error::Error for ComposeError {}

impl From<TaskError> for ComposeError {
    fn from(error: TaskError) -> Self {
        Self::CouldNotAssemble(error)
    }
}

/// The identifiers a composed plan uses.
///
/// Fixed rather than allocated, because a plan is short, composed in one place,
/// and easier to read in a log when the same step always has the same number.
mod step {
    /// Analysing whatever is not analysed.
    pub(super) const ANALYSE: u64 = 1;
    /// Finding candidates.
    pub(super) const SEARCH: u64 = 2;
    /// Arranging them.
    pub(super) const PLAN: u64 = 3;
    /// Choosing how each record becomes the next.
    pub(super) const TRANSITIONS: u64 = 4;
    /// Saying why.
    pub(super) const EXPLAIN: u64 = 5;
    /// Working out whether it meets its delivery target.
    pub(super) const DELIVER: u64 = 6;
}

/// Builds the plan a request needs, given what is true right now.
///
/// The intent is assumed to exist already: interpretation is what *produced*
/// it, so a plan never contains a step to work out what the user meant.
///
/// # Errors
///
/// Returns [`ComposeError::NothingToWorkOn`] when the request is about a set
/// with nothing in it, and [`ComposeError::CouldNotAssemble`] if the plan could
/// not be built — which nothing here can currently cause, and which is carried
/// rather than discarded so that a change that could would be caught.
pub fn plan_for(intent: &Intent, situation: &Situation) -> Result<TaskPlan, ComposeError> {
    let mut plan = TaskPlan::new();

    match *intent {
        Intent::PlanSet { .. } => {
            let mut previous = None;
            if situation.needs_analysis() {
                let analyse = TaskId::new(step::ANALYSE);
                plan.add(Task::new(analyse, Capability::Analysis))?;
                previous = Some(analyse);
            }

            let search = TaskId::new(step::SEARCH);
            let mut task = Task::new(search, Capability::Search);
            if let Some(earlier) = previous {
                task = task.after(earlier);
            }
            plan.add(task)?;

            let planning = TaskId::new(step::PLAN);
            plan.add(Task::new(planning, Capability::Planning).after(search))?;

            let transitions = TaskId::new(step::TRANSITIONS);
            plan.add(Task::new(transitions, Capability::TransitionChoice).after(planning))?;

            plan.add(
                Task::new(TaskId::new(step::EXPLAIN), Capability::Explanation).after(transitions),
            )?;
        }

        Intent::AnalyseLibrary => {
            // Housekeeping. Nobody is looking at the screen waiting for it, so
            // it yields to a performance — and an already-analysed library
            // produces an empty plan rather than a step that does nothing.
            if situation.needs_analysis() {
                plan.add(
                    Task::new(TaskId::new(step::ANALYSE), Capability::Analysis).in_the_background(),
                )?;
            }
        }

        Intent::ExplainChoice { .. } => {
            if !situation.set_has_placements {
                return Err(ComposeError::NothingToWorkOn);
            }
            plan.add(Task::new(
                TaskId::new(step::EXPLAIN),
                Capability::Explanation,
            ))?;
        }

        Intent::SuggestTransition { .. } => {
            if !situation.set_has_placements {
                return Err(ComposeError::NothingToWorkOn);
            }
            let transitions = TaskId::new(step::TRANSITIONS);
            plan.add(Task::new(transitions, Capability::TransitionChoice))?;
            plan.add(
                Task::new(TaskId::new(step::EXPLAIN), Capability::Explanation).after(transitions),
            )?;
        }

        Intent::PrepareExport => {
            if !situation.set_has_placements {
                return Err(ComposeError::NothingToWorkOn);
            }
            plan.add(Task::new(TaskId::new(step::DELIVER), Capability::Delivery))?;
        }
    }

    Ok(plan)
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::panic,
        reason = "a test that cannot build its own fixture should fail loudly"
    )]

    use super::*;
    use crate::task::Activity;
    use prv_mix::{Creativity, EnergyShape};
    use prv_security::Consents;

    fn a_set() -> Intent {
        Intent::PlanSet {
            minutes: 90,
            shape: EnergyShape::Arc,
            creativity: Creativity::Balanced,
            tempo_floor: None,
            tempo_ceiling: None,
        }
    }

    fn capabilities(plan: &TaskPlan) -> Vec<Capability> {
        plan.tasks().map(Task::capability).collect()
    }

    #[test]
    fn analysis_appears_only_when_something_is_unanalysed() {
        // A plan containing a step that does nothing teaches a caller to ignore
        // steps.
        let settled = Situation::settled();
        assert!(
            !capabilities(&plan_for(&a_set(), &settled).expect("a plan"))
                .contains(&Capability::Analysis)
        );

        let behind = Situation {
            unanalysed_tracks: 40,
            ..Situation::settled()
        };
        assert!(capabilities(&plan_for(&a_set(), &behind).expect("a plan"))
            .contains(&Capability::Analysis));
    }

    #[test]
    fn a_request_never_contains_a_step_to_work_out_what_it_meant() {
        // Interpretation is what produced the intent. A plan that interpreted
        // it again would be asking the question after it had been answered.
        for intent in [
            a_set(),
            Intent::AnalyseLibrary,
            Intent::ExplainChoice { placement: 1 },
            Intent::SuggestTransition { from_placement: 1 },
            Intent::PrepareExport,
        ] {
            let situation = Situation {
                unanalysed_tracks: 5,
                set_has_placements: true,
                device: Device::capable(),
            };
            let plan = plan_for(&intent, &situation).expect("a plan");
            assert!(
                !capabilities(&plan).contains(&Capability::Interpretation),
                "{} planned to interpret itself",
                intent.key()
            );
        }
    }

    #[test]
    fn an_analysed_library_asked_to_analyse_itself_produces_nothing_to_do() {
        // The honest answer to "analyse my library" when everything is analysed
        // is nothing — and a caller that had to distinguish that from a step
        // that does nothing would eventually get it wrong.
        let plan = plan_for(&Intent::AnalyseLibrary, &Situation::settled()).expect("a plan");
        assert!(plan.is_empty());

        let schedule = plan
            .schedule(&Consents::none(), Device::capable(), Activity::Idle)
            .expect("an empty schedule");
        assert!(schedule.is_empty());
        assert!(!schedule.has_deferred_work());
    }

    #[test]
    fn housekeeping_yields_to_a_performance_and_a_request_does_not() {
        // The distinction Master Prompt #19 turns on: it is not the capability
        // that is background work, it is why the task is in the plan.
        let behind = Situation {
            unanalysed_tracks: 200,
            ..Situation::settled()
        };

        let housekeeping = plan_for(&Intent::AnalyseLibrary, &behind).expect("a plan");
        let during_a_set = housekeeping
            .schedule(&Consents::none(), Device::capable(), Activity::Performing)
            .expect("schedule");
        assert!(during_a_set.is_empty(), "housekeeping ran during a set");
        assert_eq!(during_a_set.deferred().len(), 1);

        // The same analysis, asked for as part of a set the user wants now,
        // runs — because they asked, and refusing what somebody just requested
        // is worse than doing it.
        let requested = plan_for(&a_set(), &behind).expect("a plan");
        let also_during_a_set = requested
            .schedule(&Consents::none(), Device::capable(), Activity::Performing)
            .expect("schedule");
        assert!(also_during_a_set.deferred().is_empty());
        assert!(also_during_a_set
            .steps()
            .iter()
            .any(|step| step.agent().capability() == Capability::Analysis));
    }

    #[test]
    fn a_request_about_a_set_that_is_empty_says_so() {
        // More useful than producing an empty answer, which reads as "the
        // system had nothing to say" rather than "there was nothing there".
        let empty = Situation::settled();
        for intent in [
            Intent::ExplainChoice { placement: 1 },
            Intent::SuggestTransition { from_placement: 1 },
            Intent::PrepareExport,
        ] {
            assert_eq!(
                plan_for(&intent, &empty).err(),
                Some(ComposeError::NothingToWorkOn),
                "{}",
                intent.key()
            );
        }

        let with_music = Situation {
            set_has_placements: true,
            ..Situation::settled()
        };
        for intent in [
            Intent::ExplainChoice { placement: 1 },
            Intent::SuggestTransition { from_placement: 1 },
            Intent::PrepareExport,
        ] {
            assert!(plan_for(&intent, &with_music).is_ok(), "{}", intent.key());
        }
    }

    #[test]
    fn a_composed_plan_always_schedules() {
        // Whatever the situation, on any machine, with no agreements. If this
        // ever failed, the composer would be building plans the scheduler
        // refuses — a defect a user would meet as "it just does nothing".
        for unanalysed in [0_u32, 1, 5000] {
            for has_music in [false, true] {
                for device in [Device::capable(), Device::modest()] {
                    let situation = Situation {
                        unanalysed_tracks: unanalysed,
                        set_has_placements: has_music,
                        device,
                    };
                    for intent in [
                        a_set(),
                        Intent::AnalyseLibrary,
                        Intent::ExplainChoice { placement: 1 },
                        Intent::SuggestTransition { from_placement: 1 },
                        Intent::PrepareExport,
                    ] {
                        let Ok(plan) = plan_for(&intent, &situation) else {
                            continue;
                        };
                        for activity in [Activity::Idle, Activity::Performing] {
                            assert!(
                                plan.schedule(&Consents::none(), device, activity).is_ok(),
                                "{} would not schedule",
                                intent.key()
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn the_order_of_a_planning_plan_is_the_order_the_work_has_to_happen_in() {
        let behind = Situation {
            unanalysed_tracks: 3,
            ..Situation::settled()
        };
        let schedule = plan_for(&a_set(), &behind)
            .expect("a plan")
            .schedule(&Consents::none(), Device::capable(), Activity::Idle)
            .expect("schedule");

        let order: Vec<Capability> = schedule
            .steps()
            .iter()
            .map(|step| step.agent().capability())
            .collect();
        assert_eq!(
            order,
            vec![
                Capability::Analysis,
                Capability::Search,
                Capability::Planning,
                Capability::TransitionChoice,
                Capability::Explanation,
            ]
        );
    }

    #[test]
    fn a_situation_is_read_and_never_kept() {
        // Not a behaviour test — a shape test. `Situation` is Copy and carries
        // no identity, so there is nothing here that could go stale.
        let situation = Situation::settled();
        let copy = situation;
        assert_eq!(situation, copy);
        assert!(!situation.needs_analysis());
    }
}
