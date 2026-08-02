//! What was asked for, and what was decided.
//!
//! # An audit record that cannot contain a person
//!
//! An audit trail is the thing you read after something went wrong, which means
//! it is also the thing most likely to be exported, attached to a ticket, and
//! seen by people who were never meant to see the user's library. So an
//! [`Entry`] holds a sequence number, a subject *kind*, a capability and a
//! decision — four values from closed vocabularies, and nothing else. There is
//! no field for a file path, a project name, or a note.
//!
//! [`Entry::render`] produces a [`Record`](crate::redaction::Record), so the one
//! way an audit entry becomes text is through the module that refuses to print
//! personal data. A future field that needed redaction would have to go through
//! the same door.
//!
//! # Nothing is dropped silently
//!
//! The in-memory log is bounded, because an unbounded one is a leak with a
//! respectable name. When it overflows it discards the *oldest* entry and
//! increments [`AuditLog::discarded`]. A security log that quietly forgets is
//! worse than no log, because it reads as a complete record of a period in
//! which it was not one.
//!
//! This is the live window, not the archive. Durable retention needs a file,
//! which needs input and output, which ADR-0001 puts outside the core.

use core::fmt;
use std::collections::VecDeque;

use crate::authorisation::{Capability, PluginId, Refusal, Role, Subject};
use crate::redaction::{Field, Record};

/// What kind of thing acted, without saying which person.
///
/// A role is not personal data — it is a relationship to a project, and there
/// may be many editors. A plugin identifier names software.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Actor {
    /// A person, in the role they held.
    User(Role),
    /// A plugin.
    Plugin(PluginId),
}

impl Actor {
    /// The actor behind a subject.
    #[must_use]
    pub const fn of(subject: Subject) -> Self {
        match subject {
            Subject::User { role } => Self::User(role),
            Subject::Plugin { id, .. } => Self::Plugin(id),
        }
    }

    /// A stable identifier, for storage and localisation.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::User(_) => "actor.user",
            Self::Plugin(_) => "actor.plugin",
        }
    }
}

/// How a request ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Decision {
    /// It went ahead.
    Allowed,
    /// It did not, for this reason.
    Refused(Refusal),
}

impl Decision {
    /// The result of an authorisation call.
    #[must_use]
    pub const fn of(result: Result<(), Refusal>) -> Self {
        match result {
            Ok(()) => Self::Allowed,
            Err(refusal) => Self::Refused(refusal),
        }
    }

    /// Whether the request went ahead.
    #[must_use]
    pub const fn is_allowed(self) -> bool {
        matches!(self, Self::Allowed)
    }

    /// A stable identifier, for storage and localisation.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Allowed => "decision.allowed",
            Self::Refused(_) => "decision.refused",
        }
    }
}

/// One decision, recorded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Entry {
    ordinal: u64,
    actor: Actor,
    capability: Capability,
    decision: Decision,
}

impl Entry {
    /// Records a decision.
    ///
    /// `ordinal` is caller-supplied, as everywhere else in the core: ADR-0001
    /// keeps the clock outside. An audit trail does need wall-clock times, and
    /// they are stamped by the layer that has a clock, against these ordinals.
    #[must_use]
    pub const fn new(
        ordinal: u64,
        subject: Subject,
        capability: Capability,
        decision: Decision,
    ) -> Self {
        Self {
            ordinal,
            actor: Actor::of(subject),
            capability,
            decision,
        }
    }

    /// Where this sits in the sequence of decisions.
    #[must_use]
    pub const fn ordinal(self) -> u64 {
        self.ordinal
    }

    /// Who acted.
    #[must_use]
    pub const fn actor(self) -> Actor {
        self.actor
    }

    /// What they asked to do.
    #[must_use]
    pub const fn capability(self) -> Capability {
        self.capability
    }

    /// What was decided.
    #[must_use]
    pub const fn decision(self) -> Decision {
        self.decision
    }

    /// The entry as a diagnostic record.
    ///
    /// The only route from an entry to text, and it goes through the module
    /// that will not print personal data. Every field here is a stable key or a
    /// number by construction.
    #[must_use]
    pub fn render(self) -> Record {
        let record = Record::new("audit.decision")
            .with(Field::number(
                "ordinal",
                i64::try_from(self.ordinal).unwrap_or(i64::MAX),
            ))
            .with(Field::key("actor", self.actor.key()))
            .with(Field::key("capability", self.capability.key()))
            .with(Field::key("decision", self.decision.key()));

        let record = match self.actor {
            Actor::User(role) => record.with(Field::key("role", role.key())),
            Actor::Plugin(id) => record.with(Field::identifier("plugin", id.get())),
        };

        match self.decision {
            Decision::Allowed => record,
            Decision::Refused(refusal) => record.with(Field::key("reason", refusal.key())),
        }
    }
}

impl fmt::Display for Entry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.render())
    }
}

/// The live window of decisions.
#[derive(Debug, Clone)]
pub struct AuditLog {
    entries: VecDeque<Entry>,
    capacity: usize,
    discarded: u64,
}

impl Default for AuditLog {
    fn default() -> Self {
        Self::new()
    }
}

impl AuditLog {
    /// How many decisions the log keeps by default.
    ///
    /// Enough to cover a long session's worth of interesting decisions at a few
    /// hundred bytes in total, and small enough that its cost is never a reason
    /// to switch auditing off.
    pub const DEFAULT_CAPACITY: usize = 1024;

    /// A log of the default size.
    #[must_use]
    pub fn new() -> Self {
        Self::with_capacity(Self::DEFAULT_CAPACITY)
    }

    /// A log that keeps at most `capacity` entries, and at least one.
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        let capacity = capacity.max(1);
        Self {
            entries: VecDeque::with_capacity(capacity),
            capacity,
            discarded: 0,
        }
    }

    /// Records a decision, discarding the oldest entry if the log is full.
    pub fn record(&mut self, entry: Entry) {
        if self.entries.len() >= self.capacity {
            let _ = self.entries.pop_front();
            self.discarded = self.discarded.saturating_add(1);
        }
        self.entries.push_back(entry);
    }

    /// Authorises a request and records what was decided.
    ///
    /// The pairing exists so that deciding and recording cannot come apart. A
    /// caller that used [`authorise`](crate::authorise) directly and forgot to
    /// record would leave a hole in the trail exactly where something
    /// interesting happened.
    ///
    /// # Errors
    ///
    /// Returns the same [`Refusal`] the decision produced, after recording it.
    pub fn authorise(
        &mut self,
        ordinal: u64,
        subject: Subject,
        capability: Capability,
    ) -> Result<(), Refusal> {
        let result = crate::authorise(subject, capability);
        self.record(Entry::new(
            ordinal,
            subject,
            capability,
            Decision::of(result),
        ));
        result
    }

    /// Every entry held, oldest first.
    pub fn entries(&self) -> impl Iterator<Item = &Entry> {
        self.entries.iter()
    }

    /// Only the refusals — what an investigation starts from.
    pub fn refusals(&self) -> impl Iterator<Item = &Entry> {
        self.entries
            .iter()
            .filter(|entry| !entry.decision.is_allowed())
    }

    /// How many entries are held.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether nothing has been recorded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// How many entries fell out of the window.
    ///
    /// Never nonzero without something having been lost, and always available,
    /// so a reader can tell a complete record from a partial one.
    #[must_use]
    pub const fn discarded(&self) -> u64 {
        self.discarded
    }

    /// Whether the log holds everything it was ever given.
    #[must_use]
    pub const fn is_complete(&self) -> bool {
        self.discarded == 0
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
    use crate::authorisation::{PermissionSet, Role};
    use crate::redaction::Sensitivity;

    fn owner() -> Subject {
        Subject::user(Role::Owner)
    }

    fn viewer() -> Subject {
        Subject::user(Role::Viewer)
    }

    fn plugin() -> Subject {
        Subject::plugin(PluginId::new(11), PermissionSet::NONE)
    }

    #[test]
    fn deciding_and_recording_cannot_come_apart() {
        let mut log = AuditLog::new();
        assert!(log.is_empty());

        assert!(log.authorise(1, owner(), Capability::EditProject).is_ok());
        assert!(log.authorise(2, viewer(), Capability::EditProject).is_err());

        assert_eq!(log.len(), 2);
        let decisions: Vec<bool> = log.entries().map(|e| e.decision().is_allowed()).collect();
        assert_eq!(decisions, vec![true, false]);
        assert_eq!(log.refusals().count(), 1);
    }

    #[test]
    fn an_entry_cannot_be_rendered_into_anything_but_stable_keys() {
        // The property that makes an audit trail safe to attach to a ticket.
        let entry = Entry::new(
            7,
            plugin(),
            Capability::ReadSecrets,
            Decision::of(crate::authorise(plugin(), Capability::ReadSecrets)),
        );
        let record = entry.render();

        assert_eq!(
            record.sensitivity(),
            Sensitivity::Public,
            "an audit entry rendered something that was not public"
        );
        for field in record.fields() {
            assert!(
                field.sensitivity().is_public(),
                "the field {} is not public",
                field.name()
            );
        }

        let rendered = entry.to_string();
        assert!(rendered.contains("audit.decision"));
        assert!(rendered.contains("capability.read_secrets"));
        assert!(rendered.contains("decision.refused"));
        assert!(rendered.contains("refusal.never_delegated"));
        assert!(rendered.contains("plugin=11"));
    }

    #[test]
    fn an_allowed_entry_records_no_reason() {
        let entry = Entry::new(1, owner(), Capability::ExportProject, Decision::Allowed);
        let rendered = entry.to_string();
        assert!(rendered.contains("decision.allowed"));
        assert!(!rendered.contains("reason="));
        assert!(rendered.contains("role.owner"));
        assert_eq!(entry.ordinal(), 1);
        assert_eq!(entry.capability(), Capability::ExportProject);
        assert_eq!(entry.actor(), Actor::User(Role::Owner));
    }

    #[test]
    fn an_overflowing_log_says_how_many_it_dropped() {
        // A security log that quietly forgets is worse than no log, because it
        // reads as a complete record of a period in which it was not one.
        let mut log = AuditLog::with_capacity(4);
        for ordinal in 0..10 {
            log.record(Entry::new(
                ordinal,
                owner(),
                Capability::ViewProject,
                Decision::Allowed,
            ));
        }

        assert_eq!(log.len(), 4);
        assert_eq!(log.discarded(), 6);
        assert!(!log.is_complete());

        let ordinals: Vec<u64> = log.entries().map(|entry| entry.ordinal()).collect();
        assert_eq!(ordinals, vec![6, 7, 8, 9], "the oldest should go first");
    }

    #[test]
    fn a_log_that_lost_nothing_says_so() {
        let mut log = AuditLog::new();
        log.record(Entry::new(
            1,
            owner(),
            Capability::ViewProject,
            Decision::Allowed,
        ));
        assert!(log.is_complete());
        assert_eq!(log.discarded(), 0);
    }

    #[test]
    fn a_log_always_holds_at_least_one_entry() {
        // A zero-capacity log would discard what it was just given, which is a
        // way of being switched off while appearing to be on.
        let mut log = AuditLog::with_capacity(0);
        log.record(Entry::new(
            1,
            owner(),
            Capability::ViewProject,
            Decision::Allowed,
        ));
        assert_eq!(log.len(), 1);
    }

    #[test]
    fn the_actor_is_the_kind_and_never_the_person() {
        assert_eq!(Actor::of(owner()), Actor::User(Role::Owner));
        assert_eq!(Actor::of(plugin()), Actor::Plugin(PluginId::new(11)));
        assert_eq!(Actor::User(Role::Owner).key(), "actor.user");
        assert_ne!(
            Actor::User(Role::Owner).key(),
            Actor::Plugin(PluginId::new(1)).key()
        );
        assert_ne!(
            Decision::Allowed.key(),
            Decision::Refused(Refusal::NeverDelegated {
                id: PluginId::new(1),
                capability: Capability::ReadSecrets
            })
            .key()
        );
    }
}
