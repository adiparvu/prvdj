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

/// Whether anybody is waiting for a task.
///
/// Master Prompt #19 requires non-critical work to be suspended during live
/// playback. "Non-critical" is not a property of a capability — analysing a
/// track is background work on a Tuesday afternoon and the most urgent thing in
/// the building when a DJ has just asked for the next hour. It is a property of
/// *why the task is in the plan*, which is what this records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum Urgency {
    /// Nobody is waiting. Housekeeping, pre-computation, catching up.
    Background,
    /// Somebody asked for this and is looking at the screen.
    Requested,
}

impl Urgency {
    /// A stable identifier, for storage and localisation.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Background => "urgency.background",
            Self::Requested => "urgency.requested",
        }
    }
}

/// What the machine is doing while a plan is scheduled.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Activity {
    /// Nothing is playing to a room.
    #[default]
    Idle,
    /// Audio is going out to an audience.
    ///
    /// Everything else yields. Master Prompt #19 puts audio performance first,
    /// always, and this is the value that says so.
    Performing,
}

impl Activity {
    /// A stable identifier, for storage and localisation.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Idle => "activity.idle",
            Self::Performing => "activity.performing",
        }
    }
}

/// One thing to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    id: TaskId,
    capability: Capability,
    depends_on: Vec<TaskId>,
    urgency: Urgency,
}

impl Task {
    /// A task somebody asked for.
    #[must_use]
    pub const fn new(id: TaskId, capability: Capability) -> Self {
        Self {
            id,
            capability,
            depends_on: Vec::new(),
            urgency: Urgency::Requested,
        }
    }

    /// Marks this as work nobody is waiting for.
    ///
    /// Background work yields to a performance. It does not yield to a
    /// *requested* task that depends on it — see [`TaskPlan::schedule`], where
    /// urgency travels backwards along dependencies, because a step somebody is
    /// waiting for cannot be waiting on something that was postponed.
    #[must_use]
    pub const fn in_the_background(mut self) -> Self {
        self.urgency = Urgency::Background;
        self
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

    /// Whether anybody is waiting for it, as declared.
    ///
    /// The *effective* urgency may be higher: a background task that a
    /// requested one depends on is promoted when the plan is scheduled.
    #[must_use]
    pub const fn urgency(&self) -> Urgency {
        self.urgency
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

    /// Whether each task is really background work, after urgency has travelled
    /// backwards along dependencies.
    ///
    /// A step somebody is waiting for cannot be waiting on something that was
    /// postponed, so anything a requested task needs is itself requested. The
    /// consequence is the invariant the deferral rule rests on: the set of
    /// deferred tasks is closed under dependency, and no step in a schedule ever
    /// waits for one.
    fn effective_urgency(&self) -> BTreeMap<u64, Urgency> {
        let mut urgency: BTreeMap<u64, Urgency> = self
            .tasks
            .values()
            .map(|task| (task.id.get(), task.urgency))
            .collect();

        // Repeat until nothing changes. Bounded by the number of tasks, because
        // each pass promotes at least one or stops.
        loop {
            let mut promoted = false;
            for task in self.tasks.values() {
                if urgency.get(&task.id.get()) != Some(&Urgency::Requested) {
                    continue;
                }
                for dependency in &task.depends_on {
                    if let Some(entry) = urgency.get_mut(&dependency.get()) {
                        if *entry == Urgency::Background {
                            *entry = Urgency::Requested;
                            promoted = true;
                        }
                    }
                }
            }
            if !promoted {
                break;
            }
        }
        urgency
    }

    /// Works out the order, who does what, and what waits.
    ///
    /// During a performance, background work is *deferred* rather than dropped:
    /// it stays in the schedule's [`Schedule::deferred`] list so the caller can
    /// run it when the room empties. Master Prompt #19 puts audio performance
    /// first; it does not say the work disappears.
    ///
    /// # Errors
    ///
    /// Returns [`TaskError`] for an unknown dependency, a cycle, or a capability
    /// nothing available can serve. A cycle among deferred tasks is still an
    /// error — a plan that would only be wrong later is wrong now.
    pub fn schedule(
        &self,
        consents: &Consents,
        device: Device,
        activity: Activity,
    ) -> Result<Schedule, TaskError> {
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

        let urgency = self.effective_urgency();
        let deferred: BTreeSet<u64> = if activity == Activity::Performing {
            urgency
                .iter()
                .filter(|(_, level)| **level == Urgency::Background)
                .map(|(id, _)| *id)
                .collect()
        } else {
            BTreeSet::new()
        };

        let mut done: BTreeSet<u64> = deferred.clone();
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
            deferred: deferred.into_iter().map(TaskId::new).collect(),
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
    deferred: Vec<TaskId>,
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

    /// The tasks that are waiting for the performance to end.
    ///
    /// Never silently dropped. A plan that quietly did less during a set would
    /// leave a user wondering why their library never finishes analysing.
    #[must_use]
    pub fn deferred(&self) -> &[TaskId] {
        &self.deferred
    }

    /// Whether anything is waiting for the room to empty.
    #[must_use]
    pub fn has_deferred_work(&self) -> bool {
        !self.deferred.is_empty()
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
            .schedule(&Consents::none(), Device::capable(), Activity::Idle)
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
            .schedule(&Consents::none(), Device::capable(), Activity::Idle)
            .expect("schedule");
        for _ in 0..8 {
            assert_eq!(
                plan.schedule(&Consents::none(), Device::capable(), Activity::Idle)
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
            .schedule(&Consents::none(), Device::capable(), Activity::Idle)
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
            .schedule(&Consents::none(), Device::capable(), Activity::Idle)
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
            plan.schedule(&Consents::none(), Device::capable(), Activity::Idle)
                .err(),
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
            plan.schedule(&Consents::none(), Device::capable(), Activity::Idle),
            Err(TaskError::Cyclic { .. })
        ));
    }

    #[test]
    fn a_dependency_that_is_not_in_the_plan_is_named() {
        let mut plan = TaskPlan::new();
        plan.add(Task::new(id(1), Capability::Analysis).after(id(99)))
            .expect("add");
        assert_eq!(
            plan.schedule(&Consents::none(), Device::capable(), Activity::Idle)
                .err(),
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

        let refused = plan.schedule(&Consents::none(), Device::modest(), Activity::Idle);
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
        assert!(plan
            .schedule(&consents, Device::modest(), Activity::Idle)
            .is_ok());
    }

    #[test]
    fn a_schedule_says_in_advance_whether_anything_leaves_the_device() {
        let mut consents = Consents::none();
        consents.grant(Purpose::CloudStemSeparation, 1);

        let mut plan = TaskPlan::new();
        plan.add(Task::new(id(1), Capability::Stems)).expect("add");
        let schedule = plan
            .schedule(&consents, Device::modest(), Activity::Idle)
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
            .schedule(&Consents::none(), Device::capable(), Activity::Idle)
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

    #[test]
    fn during_a_performance_background_work_waits_rather_than_being_dropped() {
        // Master Prompt #19 puts audio performance first. It does not say the
        // work disappears — a plan that quietly did less during a set would
        // leave a user wondering why their library never finishes analysing.
        let mut plan = TaskPlan::new();
        plan.add(Task::new(id(1), Capability::Analysis).in_the_background())
            .expect("add");
        plan.add(Task::new(id(2), Capability::Search)).expect("add");

        let performing = plan
            .schedule(&Consents::none(), Device::capable(), Activity::Performing)
            .expect("schedule");
        let running: Vec<u64> = performing.steps().iter().map(|s| s.task().get()).collect();
        assert_eq!(running, vec![2], "background work ran during a performance");
        assert_eq!(performing.deferred(), &[id(1)]);
        assert!(performing.has_deferred_work());

        let idle = plan
            .schedule(&Consents::none(), Device::capable(), Activity::Idle)
            .expect("schedule");
        assert_eq!(idle.len(), 2, "the deferred task never ran");
        assert!(!idle.has_deferred_work());
    }

    #[test]
    fn nothing_a_requested_task_needs_is_ever_deferred() {
        // Urgency travels backwards along dependencies. A step somebody is
        // waiting for cannot be waiting on something that was postponed.
        let mut plan = TaskPlan::new();
        plan.add(Task::new(id(1), Capability::Analysis).in_the_background())
            .expect("add");
        plan.add(
            Task::new(id(2), Capability::Search)
                .after(id(1))
                .in_the_background(),
        )
        .expect("add");
        plan.add(Task::new(id(3), Capability::Planning).after(id(2)))
            .expect("add");

        let schedule = plan
            .schedule(&Consents::none(), Device::capable(), Activity::Performing)
            .expect("schedule");
        assert!(
            schedule.deferred().is_empty(),
            "a step the user is waiting for was left waiting on postponed work"
        );
        assert_eq!(schedule.len(), 3);
    }

    #[test]
    fn the_deferred_set_is_closed_under_dependency() {
        // The invariant the whole rule rests on: no step in a schedule ever
        // waits for something that is not in it.
        let mut plan = TaskPlan::new();
        plan.add(Task::new(id(1), Capability::Analysis).in_the_background())
            .expect("add");
        plan.add(
            Task::new(id(2), Capability::Search)
                .after(id(1))
                .in_the_background(),
        )
        .expect("add");
        plan.add(Task::new(id(3), Capability::Delivery))
            .expect("add");

        let schedule = plan
            .schedule(&Consents::none(), Device::capable(), Activity::Performing)
            .expect("schedule");
        let deferred: BTreeSet<u64> = schedule.deferred().iter().map(|t| t.get()).collect();
        assert_eq!(deferred, [1, 2].into_iter().collect::<BTreeSet<u64>>());

        for step in schedule.steps() {
            for dependency in schedule.dependencies_of(step.task()) {
                assert!(
                    !deferred.contains(&dependency.get()),
                    "{} waits for the deferred task {dependency}",
                    step.task()
                );
            }
        }
    }

    #[test]
    fn a_task_is_requested_unless_it_says_otherwise() {
        // The safe default: work whose urgency nobody thought about is work
        // somebody is waiting for, so forgetting to mark it never makes the
        // product feel like it stopped.
        assert_eq!(
            Task::new(id(1), Capability::Analysis).urgency(),
            Urgency::Requested
        );
        assert_eq!(
            Task::new(id(1), Capability::Analysis)
                .in_the_background()
                .urgency(),
            Urgency::Background
        );
        assert_eq!(Activity::default(), Activity::Idle);
        assert_ne!(Urgency::Background.key(), Urgency::Requested.key());
        assert_ne!(Activity::Idle.key(), Activity::Performing.key());
    }

    #[test]
    fn a_cycle_among_deferred_tasks_is_still_an_error() {
        // A plan that would only be wrong later is wrong now.
        let mut plan = TaskPlan::new();
        plan.add(
            Task::new(id(1), Capability::Analysis)
                .after(id(2))
                .in_the_background(),
        )
        .expect("add");
        plan.add(Task::new(id(2), Capability::Search).after(id(1)))
            .expect("add");
        assert!(matches!(
            plan.schedule(&Consents::none(), Device::capable(), Activity::Performing),
            Err(TaskError::Cyclic { .. })
        ));
    }
}
