//! What actually happened.
//!
//! # A task never runs on an input that was not produced
//!
//! When a step fails, everything downstream of it is *skipped and said to be
//! skipped*, transitively. The alternative — carry on and let the dependent
//! cope — is how a system produces a confident answer built on a missing
//! measurement, which is the worst failure available to it: not a visible error
//! but a plausible wrong result.
//!
//! [`Run::record`] does the cascade itself rather than trusting a caller to
//! remember, because the caller who forgets is the one running the interesting
//! failure.
//!
//! # A skip names the step that caused it
//!
//! Master Prompt #10 requires an error to explain. "Four steps were skipped" is
//! not an explanation; "four steps were skipped because analysing the track
//! failed" is one, and it points at the thing worth retrying.

use std::collections::{BTreeMap, BTreeSet};

use core::fmt;

use crate::agent::AgentKind;
use crate::task::{Schedule, TaskId};

/// Why a step failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Failure {
    /// The agent could not be reached.
    ///
    /// A server that did not answer, a child process that did not start. The
    /// distinguishing feature is that trying again may work.
    Unreachable,
    /// The agent ran and could not produce an answer.
    ///
    /// A track it could not decode, a library with nothing that fits. Trying
    /// again will not help; something has to change.
    NoAnswer,
    /// The agent produced something the system would not accept.
    ///
    /// The boundary in [`crate::intent`] doing its job at run time: a model
    /// returned a value outside what the system will act on.
    Rejected,
    /// It was still running when the user stopped waiting.
    Cancelled,
}

impl Failure {
    /// Whether running the same step again might succeed.
    ///
    /// What decides whether an interface offers "try again" — offering it for a
    /// failure that cannot be retried is worse than not offering it, because it
    /// spends the user's attention on a button that does nothing.
    #[must_use]
    pub const fn is_worth_retrying(self) -> bool {
        matches!(self, Self::Unreachable)
    }

    /// A stable identifier, for storage and localisation.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Unreachable => "failure.unreachable",
            Self::NoAnswer => "failure.no_answer",
            Self::Rejected => "failure.rejected",
            Self::Cancelled => "failure.cancelled",
        }
    }
}

/// How a step ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Outcome {
    /// It produced what it was for.
    Completed,
    /// It did not.
    Failed {
        /// Why.
        failure: Failure,
    },
    /// It was never attempted, because something it needed did not finish.
    Skipped {
        /// The step that failed.
        because: TaskId,
    },
}

impl Outcome {
    /// Whether the step produced an answer.
    #[must_use]
    pub const fn is_completed(self) -> bool {
        matches!(self, Self::Completed)
    }

    /// A stable identifier, for storage and localisation.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Completed => "outcome.completed",
            Self::Failed { .. } => "outcome.failed",
            Self::Skipped { .. } => "outcome.skipped",
        }
    }
}

/// A schedule being worked through.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    schedule: Schedule,
    outcomes: BTreeMap<u64, Outcome>,
}

impl Run {
    /// Starts a run.
    #[must_use]
    pub const fn new(schedule: Schedule) -> Self {
        Self {
            schedule,
            outcomes: BTreeMap::new(),
        }
    }

    /// What is being worked through.
    #[must_use]
    pub const fn schedule(&self) -> &Schedule {
        &self.schedule
    }

    /// The next step to attempt, if there is one.
    ///
    /// Skips over anything already decided — including steps this run has
    /// already marked as skipped, which is what stops a failure from being
    /// discovered once per dependent.
    #[must_use]
    pub fn next_step(&self) -> Option<TaskId> {
        self.schedule
            .steps()
            .iter()
            .map(|step| step.task())
            .find(|task| !self.outcomes.contains_key(&task.get()))
    }

    /// What will do a step.
    #[must_use]
    pub fn agent_for(&self, task: TaskId) -> Option<AgentKind> {
        self.schedule
            .steps()
            .iter()
            .find(|step| step.task() == task)
            .map(|step| step.agent())
    }

    /// Records how a step ended, skipping everything that depended on it if it
    /// failed.
    ///
    /// The cascade happens here rather than in the caller because the caller who
    /// forgets is the one running the interesting failure.
    pub fn record(&mut self, task: TaskId, outcome: Outcome) {
        self.outcomes.insert(task.get(), outcome);
        if outcome.is_completed() {
            return;
        }
        self.skip_everything_after(task);
    }

    /// Marks every step that depends on a failed one, directly or through
    /// others.
    fn skip_everything_after(&mut self, failed: TaskId) {
        let mut unreachable: BTreeSet<u64> = BTreeSet::new();
        unreachable.insert(failed.get());

        // The schedule is already in dependency order, so one pass forward is
        // enough: anything that depends on an unreachable step is itself
        // unreachable, and its own dependents come later in the list.
        for step in self.schedule.steps() {
            let task = step.task();
            if unreachable.contains(&task.get()) {
                continue;
            }
            let blocked = self
                .schedule
                .dependencies_of(task)
                .iter()
                .any(|dependency| unreachable.contains(&dependency.get()));
            if blocked {
                unreachable.insert(task.get());
                self.outcomes
                    .entry(task.get())
                    .or_insert(Outcome::Skipped { because: failed });
            }
        }
    }

    /// How a step ended, if it has.
    #[must_use]
    pub fn outcome(&self, task: TaskId) -> Option<Outcome> {
        self.outcomes.get(&task.get()).copied()
    }

    /// Whether every step has an outcome.
    #[must_use]
    pub fn is_finished(&self) -> bool {
        self.outcomes.len() >= self.schedule.len()
    }

    /// Whether every step produced an answer.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.is_finished() && self.outcomes.values().all(|outcome| outcome.is_completed())
    }

    /// How many steps produced an answer.
    #[must_use]
    pub fn completed(&self) -> usize {
        self.outcomes
            .values()
            .filter(|outcome| outcome.is_completed())
            .count()
    }

    /// The steps that failed, in order.
    pub fn failures(&self) -> impl Iterator<Item = (TaskId, Failure)> + '_ {
        self.outcomes
            .iter()
            .filter_map(|(task, outcome)| match *outcome {
                Outcome::Failed { failure } => Some((TaskId::new(*task), failure)),
                Outcome::Completed | Outcome::Skipped { .. } => None,
            })
    }

    /// The steps that were never attempted, in order.
    pub fn skipped(&self) -> impl Iterator<Item = (TaskId, TaskId)> + '_ {
        self.outcomes
            .iter()
            .filter_map(|(task, outcome)| match *outcome {
                Outcome::Skipped { because } => Some((TaskId::new(*task), because)),
                Outcome::Completed | Outcome::Failed { .. } => None,
            })
    }

    /// Whether anything that failed is worth attempting again.
    ///
    /// False when every failure is one that trying again cannot fix — which is
    /// what stops an interface offering a button that does nothing.
    #[must_use]
    pub fn is_worth_retrying(&self) -> bool {
        self.failures()
            .any(|(_, failure)| failure.is_worth_retrying())
    }
}

impl fmt::Display for Run {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}/{} completed, {} failed, {} skipped",
            self.completed(),
            self.schedule.len(),
            self.failures().count(),
            self.skipped().count()
        )
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::panic,
        reason = "a test that cannot build its own fixture should fail loudly"
    )]

    use super::*;
    use crate::agent::{Capability, Device};
    use crate::task::{Activity, Task, TaskPlan};
    use prv_security::Consents;

    fn id(value: u64) -> TaskId {
        TaskId::new(value)
    }

    fn a_run() -> Run {
        // 1 → 2 → 3, with 4 also after 2, and 5 independent of everything.
        let mut plan = TaskPlan::new();
        plan.add(Task::new(id(1), Capability::Interpretation))
            .expect("add");
        plan.add(Task::new(id(2), Capability::Analysis).after(id(1)))
            .expect("add");
        plan.add(Task::new(id(3), Capability::Planning).after(id(2)))
            .expect("add");
        plan.add(Task::new(id(4), Capability::TransitionChoice).after(id(2)))
            .expect("add");
        plan.add(Task::new(id(5), Capability::Delivery))
            .expect("add");
        Run::new(
            plan.schedule(&Consents::none(), Device::capable(), Activity::Idle)
                .expect("schedule"),
        )
    }

    #[test]
    fn a_failure_skips_everything_downstream_of_it_transitively() {
        // The worst failure available to a system is not a visible error but a
        // plausible wrong result built on a missing measurement.
        let mut run = a_run();
        run.record(id(1), Outcome::Completed);
        run.record(
            id(2),
            Outcome::Failed {
                failure: Failure::NoAnswer,
            },
        );

        assert_eq!(
            run.outcome(id(3)),
            Some(Outcome::Skipped { because: id(2) }),
            "a direct dependent ran anyway"
        );
        assert_eq!(
            run.outcome(id(4)),
            Some(Outcome::Skipped { because: id(2) }),
            "a second dependent was missed"
        );
    }

    #[test]
    fn a_skip_names_the_step_that_caused_it() {
        // "Four steps were skipped" is not an explanation. "…because analysing
        // the track failed" is one, and it points at the thing worth retrying.
        let mut run = a_run();
        run.record(
            id(1),
            Outcome::Failed {
                failure: Failure::Unreachable,
            },
        );

        let skipped: Vec<(u64, u64)> = run
            .skipped()
            .map(|(task, because)| (task.get(), because.get()))
            .collect();
        assert_eq!(skipped, vec![(2, 1), (3, 1), (4, 1)]);
    }

    #[test]
    fn a_step_that_did_not_depend_on_the_failure_is_untouched() {
        // A failure removes what it makes impossible and nothing else.
        let mut run = a_run();
        run.record(
            id(1),
            Outcome::Failed {
                failure: Failure::NoAnswer,
            },
        );
        assert_eq!(run.outcome(id(5)), None, "an independent step was skipped");
        assert_eq!(run.next_step(), Some(id(5)));

        run.record(id(5), Outcome::Completed);
        assert!(run.is_finished());
        assert!(!run.is_complete());
        assert_eq!(run.completed(), 1);
    }

    #[test]
    fn the_next_step_never_returns_something_already_decided() {
        // What stops a failure being discovered once per dependent.
        let mut run = a_run();
        assert_eq!(run.next_step(), Some(id(1)));
        run.record(id(1), Outcome::Completed);
        assert_eq!(run.next_step(), Some(id(2)));

        run.record(
            id(2),
            Outcome::Failed {
                failure: Failure::NoAnswer,
            },
        );
        assert_eq!(
            run.next_step(),
            Some(id(5)),
            "the run offered a step whose input never arrived"
        );
    }

    #[test]
    fn a_cascade_keeps_the_first_reason_rather_than_the_last() {
        // When two independent branches fail, a step that was already skipped
        // should not be relabelled by a later failure — the first explanation
        // is the one that answers "why did this not run".
        let mut plan = TaskPlan::new();
        plan.add(Task::new(id(1), Capability::Analysis))
            .expect("add");
        plan.add(Task::new(id(2), Capability::Search)).expect("add");
        plan.add(
            Task::new(id(3), Capability::Planning)
                .after(id(1))
                .after(id(2)),
        )
        .expect("add");
        let mut run = Run::new(
            plan.schedule(&Consents::none(), Device::capable(), Activity::Idle)
                .expect("schedule"),
        );

        run.record(
            id(1),
            Outcome::Failed {
                failure: Failure::NoAnswer,
            },
        );
        run.record(
            id(2),
            Outcome::Failed {
                failure: Failure::Unreachable,
            },
        );
        assert_eq!(
            run.outcome(id(3)),
            Some(Outcome::Skipped { because: id(1) })
        );
    }

    #[test]
    fn a_run_offers_another_attempt_only_when_one_could_help() {
        // Offering it for a failure that cannot be retried spends the user's
        // attention on a button that does nothing.
        let mut retryable = a_run();
        retryable.record(
            id(1),
            Outcome::Failed {
                failure: Failure::Unreachable,
            },
        );
        assert!(retryable.is_worth_retrying());

        let mut hopeless = a_run();
        hopeless.record(
            id(1),
            Outcome::Failed {
                failure: Failure::NoAnswer,
            },
        );
        assert!(!hopeless.is_worth_retrying());

        assert!(Failure::Unreachable.is_worth_retrying());
        for failure in [Failure::NoAnswer, Failure::Rejected, Failure::Cancelled] {
            assert!(!failure.is_worth_retrying(), "{}", failure.key());
        }
    }

    #[test]
    fn a_run_that_finished_cleanly_says_so() {
        let mut run = a_run();
        for value in [1, 2, 3, 4, 5] {
            run.record(id(value), Outcome::Completed);
        }
        assert!(run.is_finished());
        assert!(run.is_complete());
        assert_eq!(run.completed(), 5);
        assert_eq!(run.failures().count(), 0);
        assert_eq!(run.skipped().count(), 0);
        assert_eq!(run.next_step(), None);
        assert!(run.to_string().contains("5/5"));
    }

    #[test]
    fn a_run_knows_what_will_do_each_step() {
        let run = a_run();
        assert!(run.agent_for(id(1)).is_some());
        assert_eq!(run.agent_for(id(99)), None);
        assert_eq!(run.schedule().len(), 5);
    }

    #[test]
    fn outcome_and_failure_keys_are_distinct() {
        let outcomes = [
            Outcome::Completed.key(),
            Outcome::Failed {
                failure: Failure::NoAnswer,
            }
            .key(),
            Outcome::Skipped { because: id(1) }.key(),
        ];
        for (index, key) in outcomes.iter().enumerate() {
            for (other, value) in outcomes.iter().enumerate() {
                assert!(index == other || key != value, "two outcomes share {key}");
            }
        }

        let failures = [
            Failure::Unreachable.key(),
            Failure::NoAnswer.key(),
            Failure::Rejected.key(),
            Failure::Cancelled.key(),
        ];
        for (index, key) in failures.iter().enumerate() {
            for (other, value) in failures.iter().enumerate() {
                assert!(index == other || key != value, "two failures share {key}");
            }
        }
    }
}
