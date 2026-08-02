//! The shell around a deterministic planner.
//!
//! # What an orchestrator is for, and what it is not for
//!
//! Master Prompt #19 asks for several capabilities coordinated into one
//! coherent answer. ADR-0006 says how: musical decisions are *computed* under
//! hard constraints, and language is used only to translate intent inward and
//! evidence outward. This crate is the coordination, and it is deliberately not
//! the intelligence.
//!
//! Nothing here decides anything musical. [`intent`] turns what a user asked for
//! into the planner's own goal, bounding every value at the boundary because the
//! thing on the other side produces output that is plausible rather than
//! correct. [`agent`] says what the system can do and where each of those things
//! happens. [`task`] works out the order. [`run`] records what happened. The
//! decisions themselves belong to `prv-mix` and `prv-analysis`, and are the same
//! whether or not a sentence was ever involved.
//!
//! # Two properties are worth more than the rest of the crate
//!
//! **No musical decision is made on a server.** A musical decision has to be
//! reproducible, explainable, and the same this evening as it was this
//! afternoon. A request to a model is none of those. The pairing is checked over
//! every agent.
//!
//! **Losing the cloud costs fluency, never capability.** With no agreement of
//! any kind the product still interprets a request, analyses tracks, searches,
//! plans, chooses transitions, explains itself and prepares a delivery. It stops
//! reading sentences and stops writing prose. A test withdraws everything and
//! schedules a whole set-planning plan to prove it, because "graceful
//! degradation" is a phrase that means nothing until something measures it.
//!
//! # What running a plan looks like
//!
//! ```
//! use prv_ai::{Activity, Capability, Consents, Device, Outcome, Run, Task, TaskId, TaskPlan};
//!
//! let mut plan = TaskPlan::new();
//! plan.add(Task::new(TaskId::new(1), Capability::Analysis))?;
//! plan.add(Task::new(TaskId::new(2), Capability::Planning).after(TaskId::new(1)))?;
//!
//! // Nothing has been agreed to, and the plan still runs — on the device.
//! let schedule = plan.schedule(&Consents::none(), Device::capable(), Activity::Idle)?;
//! assert!(!schedule.anything_leaves_the_device());
//!
//! let mut run = Run::new(schedule);
//! while let Some(task) = run.next_step() {
//!     run.record(task, Outcome::Completed);
//! }
//! assert!(run.is_complete());
//! # Ok::<(), Box<dyn core::error::Error>>(())
//! ```

pub mod agent;
pub mod compose;
pub mod intent;
pub mod run;
pub mod task;

pub use agent::{AgentKind, Capability, Device, DeviceFeature};
pub use compose::{plan_for, ComposeError, Situation};
pub use intent::{Intent, IntentError};
pub use run::{Failure, Outcome, Run};
pub use task::{Activity, Schedule, Step, Task, TaskError, TaskId, TaskPlan, Urgency};

/// Re-exported so a caller can build a plan without naming `prv-security`
/// directly for the one type it needs.
pub use prv_security::Consents;

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::panic,
        reason = "a test that cannot build its own fixture should fail loudly"
    )]

    use super::*;
    use prv_mix::{Creativity, EnergyShape};
    use prv_security::{ProcessingLocation, Purpose};

    fn a_request() -> Intent {
        Intent::PlanSet {
            minutes: 120,
            shape: EnergyShape::Arc,
            creativity: Creativity::Balanced,
            tempo_floor: Some(120.0),
            tempo_ceiling: Some(132.0),
        }
    }

    fn a_plan_for(intent: &Intent) -> TaskPlan {
        let mut plan = TaskPlan::new();
        plan.add(Task::new(TaskId::new(1), Capability::Interpretation))
            .expect("add");
        plan.add(Task::new(TaskId::new(2), Capability::Analysis).after(TaskId::new(1)))
            .expect("add");
        plan.add(Task::new(TaskId::new(3), Capability::Search).after(TaskId::new(2)))
            .expect("add");
        if intent.is_planning() {
            plan.add(Task::new(TaskId::new(4), Capability::Planning).after(TaskId::new(3)))
                .expect("add");
            plan.add(Task::new(TaskId::new(5), Capability::TransitionChoice).after(TaskId::new(4)))
                .expect("add");
        }
        plan.add(Task::new(TaskId::new(6), Capability::Explanation).after(TaskId::new(4)))
            .expect("add");
        plan
    }

    #[test]
    fn a_two_hour_set_is_planned_with_nothing_agreed_to_and_nothing_sent() {
        // The whole crate in one test, taking the path a privacy-conscious user
        // actually takes: they have agreed to nothing at all.
        let intent = a_request();
        let goal = intent.to_goal(48_000).expect("a valid request");
        assert_eq!(goal.duration().get(), 120 * 60 * 48_000);
        assert_eq!(goal.shape(), EnergyShape::Arc);

        let plan = a_plan_for(&intent);
        let consents = Consents::none();
        assert!(
            plan.missing_agreements(&consents, Device::capable())
                .is_empty(),
            "a plan that can run locally asked for an agreement"
        );

        let schedule = plan
            .schedule(&consents, Device::capable(), Activity::Idle)
            .expect("a schedule");
        assert!(!schedule.anything_leaves_the_device());
        for step in schedule.steps() {
            assert_eq!(step.agent().location(), ProcessingLocation::OnDevice);
        }

        let mut run = Run::new(schedule);
        while let Some(task) = run.next_step() {
            run.record(task, Outcome::Completed);
        }
        assert!(run.is_complete());
    }

    #[test]
    fn agreeing_to_the_cloud_changes_how_it_reads_and_writes_and_not_what_it_decides() {
        // ADR-0006's division, end to end. Granting every agreement must not
        // move a musical decision off the device.
        let mut everything = Consents::none();
        let mut ordinal = 0;
        for purpose in Purpose::ALL {
            ordinal += 1;
            everything.grant(purpose, ordinal);
        }

        let schedule = a_plan_for(&a_request())
            .schedule(&everything, Device::capable(), Activity::Idle)
            .expect("a schedule");

        for step in schedule.steps() {
            if step.agent().decides_musically() {
                assert_eq!(
                    step.agent().location(),
                    ProcessingLocation::OnDevice,
                    "{} decided musically on a server",
                    step.agent()
                );
            }
        }
    }

    #[test]
    fn a_failure_early_on_never_produces_a_confident_answer_late_on() {
        // The failure mode worth engineering against: not a visible error, but
        // a plausible wrong result built on a measurement that never arrived.
        let schedule = a_plan_for(&a_request())
            .schedule(&Consents::none(), Device::capable(), Activity::Idle)
            .expect("a schedule");
        let mut run = Run::new(schedule);

        run.record(TaskId::new(1), Outcome::Completed);
        run.record(
            TaskId::new(2),
            Outcome::Failed {
                failure: Failure::NoAnswer,
            },
        );

        assert_eq!(
            run.outcome(TaskId::new(4)),
            Some(Outcome::Skipped {
                because: TaskId::new(2)
            }),
            "a set was planned on an analysis that failed"
        );
        assert_eq!(
            run.outcome(TaskId::new(6)),
            Some(Outcome::Skipped {
                because: TaskId::new(2)
            }),
            "an explanation was offered for a set that was never planned"
        );
        assert!(run.is_finished());
        assert!(!run.is_complete());
    }
}
