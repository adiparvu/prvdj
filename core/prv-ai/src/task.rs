//! What has to happen, in what order, and by what.
//!
//! # A task names a capability, not an agent
//!
//! A plan says "analyse this track"; *which* agent does the analysing is
//! resolved at scheduling time from what the user has agreed to. That ordering
//! is the whole reason the offline path works: the same plan runs with every
//! consent granted and with none, and the only difference is which agent each
//! step resolves to.
//!
//! # Scheduling is deterministic, and that is a requirement rather than a
//! convenience
//!
//! Master Prompt #19 asks for an orchestrator whose behaviour can be explained.
//! An order that varied between runs would make every support conversation
//! start with "and did it do them in this order that time" — so among tasks
//! whose dependencies are all met, the lowest identifier goes first, always.
//!
//! # A cycle is refused, not run
//!
//! It would be possible to break a cycle by dropping an edge and to carry on.
//! That produces a plan that runs and is wrong, which is worse than one that
//! does not run: the wrongness surfaces later, somewhere else, as a task that
//! read a value before it was written.

use std::collections::{BTreeMap, BTreeSet};

use core::fmt;

use prv_security::{Consents, Purpose};

use crate::agent::{AgentKind, Capability, Device};

/// Which task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TaskId(u64);

impl TaskId {
    /// Names a task.
    #[must_use]
    pub const fn new(id: u64) -> Self {
        Self(id)
    }

    /// The underlying value.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl fmt::Display for TaskId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// One thing to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    id: TaskId,
    capability: Capability,
    depends_on: Vec<TaskId>,
}

impl Task {
    /// A task with no dependencies.
    #[must_use]
    pub const fn new(id: TaskId, capability: Capability) -> Self {
        Self {
            id,
            capability,
            depends_on: Vec::new(),
        }
    }

    /// Adds a dependency.
    #[must_use]
    pub fn after(mut self, other: TaskId) -> Self {
        if !self.depends_on.contains(&other) {
            self.depends_on.push(other);
            self.depends_on.sort_unstable();
        }
        self
    }

    /// Which task.
    #[must_use]
    pub const fn id(&self) -> TaskId {
        self.id
    }

    /// What it needs done.
    #[must_use]
    pub const fn capability(&self) -> Capability {
        self.capability
    }

    /// What must finish first.
    #[must_use]
    pub fn depends_on(&self) -> &[TaskId] {
        &self.depends_on
    }
}

/// Why a plan could not be built or scheduled.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum TaskError {
    /// Two tasks share an identifier.
    DuplicateTask {
        /// Which.
        id: TaskId,
    },
    /// A task depends on something that is not in the plan.
    UnknownDependency {
        /// Which task.
        task: TaskId,
        /// What it wanted.
        missing: TaskId,
    },
    /// A task depends on itself, directly or through others.
    Cyclic {
        /// The tasks caught in it, in identifier order.
        tasks: Vec<TaskId>,
    },
    /// The plan holds as many tasks as it will.
    Full {
        /// How many that is.
        limit: usize,
    },
    /// Nothing available can serve a task's capability.
    ///
    /// Carries the agreement that would make one available, if there is one, so
    /// an interface asks the right question instead of reporting a failure.
    NoAgent {
        /// Which task.
        task: TaskId,
        /// What it needed done.
        capability: Capability,
        /// What the user would have to agree to.
        needs: Option<Purpose>,
    },
}

impl fmt::Display for TaskError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateTask { id } => write!(f, "task {id} is already in the plan"),
            Self::UnknownDependency { task, missing } => {
                write!(
                    f,
                    "task {task} depends on {missing}, which is not in the plan"
                )
            }
            Self::Cyclic { tasks } => write!(f, "{} tasks depend on each other", tasks.len()),
            Self::Full { limit } => write!(f, "a plan holds no more than {limit} tasks"),
            Self::NoAgent {
                task, capability, ..
            } => write!(f, "nothing available can do {capability} for task {task}"),
        }
    }
}

impl core::error::Error for TaskError {}

/// One step of a schedule: a task and the agent that will do it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Step {
    task: TaskId,
    agent: AgentKind,
}

impl Step {
    /// Which task.
    #[must_use]
    pub const fn task(self) -> TaskId {
        self.task
    }

    /// What will do it.
    #[must_use]
    pub const fn agent(self) -> AgentKind {
        self.agent
    }
}

/// Everything that has to happen.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TaskPlan {
    tasks: BTreeMap<u64, Task>,
}

impl TaskPlan {
    /// How many tasks one plan may hold.
    ///
    /// Bounded because the plan is built from an intent, and an intent comes
    /// from a model. A request that expands into ten thousand tasks is not a
    /// request.
    pub const MAX_TASKS: usize = 1024;

    /// An empty plan.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a task.
    ///
    /// # Errors
    ///
    /// Returns [`TaskError`] if the identifier is already used or the plan is
    /// full. Dependencies are checked when the plan is scheduled rather than
    /// here, so that tasks may be added in any order.
    pub fn add(&mut self, task: Task) -> Result<(), TaskError> {
        if self.tasks.contains_key(&task.id.get()) {
            return Err(TaskError::DuplicateTask { id: task.id });
        }
        if self.tasks.len() >= Self::MAX_TASKS {
            return Err(TaskError::Full {
                limit: Self::MAX_TASKS,
            });
        }
        self.tasks.insert(task.id.get(), task);
        Ok(())
    }

    /// Every task, in identifier order.
    pub fn tasks(&self) -> impl Iterator<Item = &Task> {
        self.tasks.values()
    }

    /// How many tasks.
    #[must_use]
    pub fn len(&self) -> usize {
        self.tasks.len()
    }

    /// Whether there is nothing to do.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tasks.is_empty()
    }

    /// The agreements this plan would need that the user has not given.
    ///
    /// Computed *before* anything runs, so an interface can ask once, up front,
    /// for exactly what is needed — rather than interrupting a user four steps
    /// in with a question they could have answered at the start.
    ///
    /// Empty when everything can be done on the device, which is the usual case.
    #[must_use]
    pub fn missing_agreements(&self, consents: &Consents, device: Device) -> Vec<Purpose> {
        let mut needed = BTreeSet::new();
        for task in self.tasks.values() {
            if task.capability.available_agent(consents, device).is_some() {
                continue;
            }
            for agent in task.capability.agents() {
                if let Some(purpose) = agent.purpose() {
                    needed.insert(purpose);
                }
            }
        }
        needed.into_iter().collect()
    }

    /// Works out the order and who does what.
    ///
    /// # Errors
    ///
    /// Returns [`TaskError`] for an unknown dependency, a cycle, or a capability
    /// nothing available can serve.
    pub fn schedule(&self, consents: &Consents, device: Device) -> Result<Schedule, TaskError> {
        for task in self.tasks.values() {
            for dependency in &task.depends_on {
                if !self.tasks.contains_key(&dependency.get()) {
                    return Err(TaskError::UnknownDependency {
                        task: task.id,
                        missing: *dependency,
                    });
                }
            }
        }

        let mut done: BTreeSet<u64> = BTreeSet::new();
        let mut steps: Vec<Step> = Vec::with_capacity(self.tasks.len());

        while done.len() < self.tasks.len() {
            // The lowest identifier whose dependencies are all met. Taking the
            // lowest rather than the first found is what makes the order the
            // same on every run and on every machine.
            let ready = self.tasks.values().find(|task| {
                !done.contains(&task.id.get())
                    && task
                        .depends_on
                        .iter()
                        .all(|dependency| done.contains(&dependency.get()))
            });

            let Some(task) = ready else {
                let caught: Vec<TaskId> = self
                    .tasks
                    .values()
                    .filter(|task| !done.contains(&task.id.get()))
                    .map(|task| task.id)
                    .collect();
                return Err(TaskError::Cyclic { tasks: caught });
            };

            let agent = task
                .capability
                .available_agent(consents, device)
                .ok_or_else(|| TaskError::NoAgent {
                    task: task.id,
                    capability: task.capability,
                    needs: task
                        .capability
                        .agents()
                        .into_iter()
                        .find_map(AgentKind::purpose),
                })?;

            done.insert(task.id.get());
            steps.push(Step {
                task: task.id,
                agent,
            });
        }

        Ok(Schedule {
            steps,
            dependencies: self
                .tasks
                .values()
                .map(|task| (task.id.get(), task.depends_on.clone()))
                .collect(),
        })
    }
}

/// A plan with an order and an agent for each step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Schedule {
    steps: Vec<Step>,
    dependencies: BTreeMap<u64, Vec<TaskId>>,
}

impl Schedule {
    /// The steps, in the order they run.
    #[must_use]
    pub fn steps(&self) -> &[Step] {
        &self.steps
    }

    /// How many steps.
    #[must_use]
    pub fn len(&self) -> usize {
        self.steps.len()
    }

    /// Whether there is nothing to do.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    /// What a step waits for.
    #[must_use]
    pub fn dependencies_of(&self, task: TaskId) -> &[TaskId] {
        self.dependencies
            .get(&task.get())
            .map_or(&[], Vec::as_slice)
    }

    /// Whether anything in this schedule leaves the device.
    ///
    /// What an interface shows before it starts, so that "this will be sent to
    /// a server" is said once, in advance, rather than discovered afterwards.
    #[must_use]
    pub fn anything_leaves_the_device(&self) -> bool {
        self.steps
            .iter()
            .any(|step| step.agent.location().leaves_the_device())
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
    use prv_security::ProcessingLocation;

    fn id(value: u64) -> TaskId {
        TaskId::new(value)
    }

    fn a_set_planning_plan() -> TaskPlan {
        // Interpret what was asked, analyse what is not analysed, search for
        // candidates, plan the set, then choose the transitions. The shape of
        // an actual request.
        let mut plan = TaskPlan::new();
        plan.add(Task::new(id(1), Capability::Interpretation))
            .expect("add");
        plan.add(Task::new(id(2), Capability::Analysis).after(id(1)))
            .expect("add");
        plan.add(Task::new(id(3), Capability::Search).after(id(2)))
            .expect("add");
        plan.add(Task::new(id(4), Capability::Planning).after(id(3)))
            .expect("add");
        plan.add(Task::new(id(5), Capability::TransitionChoice).after(id(4)))
            .expect("add");
        plan
    }

    #[test]
    fn a_whole_plan_runs_with_no_agreements_at_all() {
        // The offline path is a path, not an absence. Every step resolves to a
        // local agent and nothing leaves the device.
        let schedule = a_set_planning_plan()
            .schedule(&Consents::none(), Device::capable())
            .expect("a schedule with no agreements");

        assert_eq!(schedule.len(), 5);
        assert!(!schedule.anything_leaves_the_device());
        for step in schedule.steps() {
            assert_eq!(step.agent().location(), ProcessingLocation::OnDevice);
        }
    }

    #[test]
    fn the_order_is_the_same_on_every_run() {
        // An order that varied would make every support conversation start with
        // "and did it do them in this order that time".
        let plan = a_set_planning_plan();
        let first = plan
            .schedule(&Consents::none(), Device::capable())
            .expect("schedule");
        for _ in 0..8 {
            assert_eq!(
                plan.schedule(&Consents::none(), Device::capable())
                    .expect("schedule"),
                first,
                "the schedule changed between runs"
            );
        }

        let order: Vec<u64> = first.steps().iter().map(|s| s.task().get()).collect();
        assert_eq!(order, vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn independent_tasks_run_in_identifier_order() {
        let mut plan = TaskPlan::new();
        for (value, capability) in [
            (7, Capability::Analysis),
            (2, Capability::Search),
            (5, Capability::Delivery),
        ] {
            plan.add(Task::new(id(value), capability)).expect("add");
        }
        let schedule = plan
            .schedule(&Consents::none(), Device::capable())
            .expect("schedule");
        let order: Vec<u64> = schedule.steps().iter().map(|s| s.task().get()).collect();
        assert_eq!(order, vec![2, 5, 7]);
    }

    #[test]
    fn a_dependency_always_comes_first() {
        // The property the ordering exists for, checked rather than assumed
        // from the shape of the fixture.
        let mut plan = TaskPlan::new();
        plan.add(Task::new(id(10), Capability::Planning).after(id(20)))
            .expect("add");
        plan.add(Task::new(id(20), Capability::Analysis).after(id(30)))
            .expect("add");
        plan.add(Task::new(id(30), Capability::Interpretation))
            .expect("add");

        let schedule = plan
            .schedule(&Consents::none(), Device::capable())
            .expect("schedule");
        let order: Vec<u64> = schedule.steps().iter().map(|s| s.task().get()).collect();
        assert_eq!(order, vec![30, 20, 10]);

        let mut seen = BTreeSet::new();
        for step in schedule.steps() {
            for dependency in schedule.dependencies_of(step.task()) {
                assert!(
                    seen.contains(&dependency.get()),
                    "{} ran before {dependency}",
                    step.task()
                );
            }
            seen.insert(step.task().get());
        }
    }

    #[test]
    fn a_cycle_is_refused_rather_than_broken() {
        // Breaking it by dropping an edge produces a plan that runs and is
        // wrong, which is worse than one that does not run.
        let mut plan = TaskPlan::new();
        plan.add(Task::new(id(1), Capability::Analysis).after(id(2)))
            .expect("add");
        plan.add(Task::new(id(2), Capability::Search).after(id(3)))
            .expect("add");
        plan.add(Task::new(id(3), Capability::Planning).after(id(1)))
            .expect("add");

        assert_eq!(
            plan.schedule(&Consents::none(), Device::capable()).err(),
            Some(TaskError::Cyclic {
                tasks: vec![id(1), id(2), id(3)]
            })
        );
    }

    #[test]
    fn a_task_that_depends_on_itself_is_a_cycle_like_any_other() {
        let mut plan = TaskPlan::new();
        plan.add(Task::new(id(1), Capability::Analysis).after(id(1)))
            .expect("add");
        assert!(matches!(
            plan.schedule(&Consents::none(), Device::capable()),
            Err(TaskError::Cyclic { .. })
        ));
    }

    #[test]
    fn a_dependency_that_is_not_in_the_plan_is_named() {
        let mut plan = TaskPlan::new();
        plan.add(Task::new(id(1), Capability::Analysis).after(id(99)))
            .expect("add");
        assert_eq!(
            plan.schedule(&Consents::none(), Device::capable()).err(),
            Some(TaskError::UnknownDependency {
                task: id(1),
                missing: id(99),
            })
        );
    }

    #[test]
    fn a_capability_nothing_can_serve_names_the_agreement_that_would_help() {
        // So an interface asks the right question instead of reporting a
        // failure.
        let mut plan = TaskPlan::new();
        plan.add(Task::new(id(1), Capability::Stems)).expect("add");

        let refused = plan.schedule(&Consents::none(), Device::modest());
        let Some(TaskError::NoAgent {
            capability, needs, ..
        }) = refused.err()
        else {
            panic!("a plan needing stems with no agreement should not schedule");
        };
        assert_eq!(capability, Capability::Stems);
        assert!(needs.is_some(), "the remedy was not named");
    }

    #[test]
    fn what_a_plan_needs_is_known_before_anything_runs() {
        // So a user is asked once, up front, rather than interrupted four steps
        // in with a question they could have answered at the start.
        let mut plan = TaskPlan::new();
        plan.add(Task::new(id(1), Capability::Analysis))
            .expect("add");
        plan.add(Task::new(id(2), Capability::Stems)).expect("add");

        let missing = plan.missing_agreements(&Consents::none(), Device::modest());
        assert!(missing.contains(&Purpose::CloudStemSeparation));
        assert!(
            !missing.contains(&Purpose::CloudAnalysis),
            "an agreement was asked for where a local agent exists"
        );

        let mut consents = Consents::none();
        consents.grant(Purpose::CloudStemSeparation, 1);
        assert!(plan
            .missing_agreements(&consents, Device::modest())
            .is_empty());
        assert!(plan.schedule(&consents, Device::modest()).is_ok());
    }

    #[test]
    fn a_schedule_says_in_advance_whether_anything_leaves_the_device() {
        let mut consents = Consents::none();
        consents.grant(Purpose::CloudStemSeparation, 1);

        let mut plan = TaskPlan::new();
        plan.add(Task::new(id(1), Capability::Stems)).expect("add");
        let schedule = plan
            .schedule(&consents, Device::modest())
            .expect("schedule");
        assert!(schedule.anything_leaves_the_device());
    }

    #[test]
    fn a_plan_refuses_a_duplicate_and_is_bounded() {
        let mut plan = TaskPlan::new();
        plan.add(Task::new(id(1), Capability::Analysis))
            .expect("add");
        assert_eq!(
            plan.add(Task::new(id(1), Capability::Search)).err(),
            Some(TaskError::DuplicateTask { id: id(1) })
        );

        let mut big = TaskPlan::new();
        for value in 0..TaskPlan::MAX_TASKS as u64 {
            big.add(Task::new(id(value), Capability::Analysis))
                .expect("within the limit");
        }
        assert_eq!(
            big.add(Task::new(id(99_999), Capability::Analysis)).err(),
            Some(TaskError::Full {
                limit: TaskPlan::MAX_TASKS
            })
        );
    }

    #[test]
    fn an_empty_plan_schedules_to_nothing_rather_than_failing() {
        let plan = TaskPlan::new();
        assert!(plan.is_empty());
        let schedule = plan
            .schedule(&Consents::none(), Device::capable())
            .expect("an empty schedule");
        assert!(schedule.is_empty());
        assert!(!schedule.anything_leaves_the_device());
        assert!(schedule.dependencies_of(id(1)).is_empty());
        assert_eq!(plan.tasks().count(), 0);
    }

    #[test]
    fn a_dependency_added_twice_is_held_once() {
        let task = Task::new(id(1), Capability::Analysis)
            .after(id(2))
            .after(id(2));
        assert_eq!(task.depends_on(), &[id(2)]);
        assert_eq!(task.capability(), Capability::Analysis);
        assert_eq!(task.id(), id(1));
    }
}
