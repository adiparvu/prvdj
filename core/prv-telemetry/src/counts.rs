//! What is kept, and the reason it is so little.
//!
//! # Withholding agreement means nothing is recorded
//!
//! This is the decision the module exists for, and it is the one most products
//! get wrong. The common shape is to collect everything and gate the *upload*,
//! which is defensible in a diagram and indefensible in practice: recording a
//! person's behaviour and deciding later not to send it is still having recorded
//! it, and a bug in the gate is a disclosure rather than a missed opportunity.
//!
//! [`Counts::record`] therefore does nothing at all without the agreement. There
//! is no buffer filling up in the meantime, so there is nothing for a mistake to
//! release.
//!
//! # Counts, not a trace
//!
//! What is kept is one number per event kind. No order, no times, no session,
//! nothing that says *when*. A sequence of timestamped events is a behavioural
//! record of one person's evening; the same events as totals answer "is anyone
//! using this" and answer nothing else. The second is what the product needs.
//!
//! ADR-0001's rule that the core cannot read a clock happens to make the wrong
//! version impossible to write here, which is a pleasant coincidence rather than
//! the reason.
//!
//! # The user can see exactly what is held
//!
//! [`Counts::report`] renders through `prv-security`'s redaction module, so what
//! a user is shown is the same structure that would be sent — not a description
//! of it written separately and liable to drift.

use std::collections::BTreeMap;

use prv_security::redaction::{Field, Record};
use prv_security::{Consents, Purpose};

use crate::event::Event;

/// Everything this device has counted.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Counts {
    totals: BTreeMap<Event, u64>,
}

impl Counts {
    /// Nothing counted.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The agreement an event needs before it may be counted.
    ///
    /// Faults belong to crash diagnostics, everything else to usage counting.
    /// A user who wants to help get crashes fixed has not thereby agreed to
    /// having their habits counted.
    #[must_use]
    pub const fn purpose_for(event: Event) -> Purpose {
        if event.is_a_fault() {
            Purpose::CrashDiagnostics
        } else {
            Purpose::UsageAnalytics
        }
    }

    /// Counts an event, if the user has agreed to that kind being counted.
    ///
    /// Returns whether anything was recorded. Without the agreement this is a
    /// no-op and *not* a deferred write: there is no buffer, so there is nothing
    /// a later mistake could release.
    pub fn record(&mut self, event: Event, consents: &Consents) -> bool {
        if !consents.allows(Self::purpose_for(event)) {
            return false;
        }
        let total = self.totals.entry(event).or_insert(0);
        *total = total.saturating_add(1);
        true
    }

    /// How many times an event has been counted.
    #[must_use]
    pub fn total(&self, event: Event) -> u64 {
        self.totals.get(&event).copied().unwrap_or(0)
    }

    /// Every event with a non-zero count, in event order.
    #[must_use]
    pub fn totals(&self) -> Vec<(Event, u64)> {
        self.totals
            .iter()
            .map(|(event, total)| (*event, *total))
            .collect()
    }

    /// Whether nothing has been counted.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.totals.is_empty()
    }

    /// Discards everything counted under a purpose.
    ///
    /// Called when an agreement is withdrawn. Withdrawing consent has to mean
    /// the data goes, not that the next upload is skipped — otherwise
    /// "withdraw" is a word for something else.
    pub fn forget(&mut self, purpose: Purpose) {
        self.totals
            .retain(|event, _| Self::purpose_for(*event) != purpose);
    }

    /// Discards everything.
    pub fn forget_all(&mut self) {
        self.totals.clear();
    }

    /// Brings the counts into line with what is currently agreed to.
    ///
    /// Called after any change to the agreements. Anything no longer agreed to
    /// is discarded, so a withdrawal takes effect on what is *held* and not only
    /// on what is collected next.
    pub fn apply(&mut self, consents: &Consents) {
        self.totals
            .retain(|event, _| consents.allows(Self::purpose_for(*event)));
    }

    /// What would be sent, in the form it would be sent.
    ///
    /// The same structure the user is shown. A separately written summary of
    /// "what we collect" is a document that drifts from the code; this cannot.
    #[must_use]
    pub fn report(&self) -> Record {
        let mut record = Record::new("telemetry.counts");
        for (event, total) in &self.totals {
            record = record.with(Field::number(
                event.key(),
                i64::try_from(*total).unwrap_or(i64::MAX),
            ));
        }
        record
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
    use prv_security::redaction::Sensitivity;

    fn agreeing_to(purposes: &[Purpose]) -> Consents {
        let mut consents = Consents::none();
        let mut ordinal = 0;
        for purpose in purposes {
            ordinal += 1;
            consents.grant(*purpose, ordinal);
        }
        consents
    }

    #[test]
    fn nothing_is_recorded_without_agreement() {
        // Not "recorded and not sent". Recording a person's behaviour and
        // deciding later not to send it is still having recorded it, and a bug
        // in the gate would be a disclosure rather than a missed opportunity.
        let mut counts = Counts::new();
        let nothing = Consents::none();

        for event in Event::ALL {
            assert!(!counts.record(event, &nothing), "{event} was recorded");
        }
        assert!(counts.is_empty());
        assert!(counts.report().fields().is_empty());
    }

    #[test]
    fn agreeing_to_crash_reports_is_not_agreeing_to_being_counted() {
        // A user who wants to help get crashes fixed has not agreed to having
        // their habits counted.
        let mut counts = Counts::new();
        let faults_only = agreeing_to(&[Purpose::CrashDiagnostics]);

        assert!(counts.record(Event::PluginCrashed, &faults_only));
        assert!(!counts.record(Event::PlanKept, &faults_only));

        assert_eq!(counts.total(Event::PluginCrashed), 1);
        assert_eq!(counts.total(Event::PlanKept), 0);
    }

    #[test]
    fn withdrawing_an_agreement_discards_what_was_already_held() {
        // Otherwise "withdraw" is a word for something else.
        let mut counts = Counts::new();
        let everything = agreeing_to(&[Purpose::CrashDiagnostics, Purpose::UsageAnalytics]);
        counts.record(Event::PluginCrashed, &everything);
        counts.record(Event::PlanKept, &everything);
        assert_eq!(counts.totals().len(), 2);

        let mut narrowed = everything.clone();
        narrowed.withdraw(Purpose::UsageAnalytics);
        counts.apply(&narrowed);

        assert_eq!(counts.total(Event::PluginCrashed), 1);
        assert_eq!(counts.total(Event::PlanKept), 0, "usage counts survived");

        counts.forget(Purpose::CrashDiagnostics);
        assert!(counts.is_empty());
    }

    #[test]
    fn a_cleared_record_is_indistinguishable_from_one_that_never_counted() {
        let mut counts = Counts::new();
        let everything = agreeing_to(&[Purpose::CrashDiagnostics, Purpose::UsageAnalytics]);
        for event in Event::ALL {
            counts.record(event, &everything);
        }
        counts.forget_all();
        assert_eq!(counts, Counts::new());
    }

    #[test]
    fn what_is_kept_is_a_total_and_never_an_order() {
        // A sequence of events is a behavioural record of one person's evening.
        // The same events as totals answer "is anyone using this" and nothing
        // else, which is what the product needs.
        let everything = agreeing_to(&[Purpose::UsageAnalytics]);

        let mut first = Counts::new();
        for event in [Event::PlanKept, Event::PlanRequested, Event::PlanKept] {
            first.record(event, &everything);
        }

        let mut reordered = Counts::new();
        for event in [Event::PlanKept, Event::PlanKept, Event::PlanRequested] {
            reordered.record(event, &everything);
        }

        assert_eq!(
            first, reordered,
            "the order events arrived in survived into what is held"
        );
        assert_eq!(first.total(Event::PlanKept), 2);
    }

    #[test]
    fn what_a_user_is_shown_is_what_would_be_sent() {
        // A separately written summary of "what we collect" is a document that
        // drifts. This one cannot.
        let everything = agreeing_to(&[Purpose::UsageAnalytics]);
        let mut counts = Counts::new();
        counts.record(Event::ExportCompleted, &everything);
        counts.record(Event::ExportCompleted, &everything);

        let report = counts.report();
        assert_eq!(
            report.sensitivity(),
            Sensitivity::Public,
            "a telemetry report carried something that is not public"
        );
        let rendered = report.to_string();
        assert!(rendered.contains("event.export_completed=2"), "{rendered}");
    }

    #[test]
    fn a_report_can_hold_nothing_a_person_could_be_recognised_by() {
        // Checked as a property over every field rather than by reading the
        // constructor, because the constructor is what a later change edits.
        let everything = agreeing_to(&[Purpose::CrashDiagnostics, Purpose::UsageAnalytics]);
        let mut counts = Counts::new();
        for event in Event::ALL {
            counts.record(event, &everything);
        }
        for field in counts.report().fields() {
            assert!(
                field.sensitivity().is_public(),
                "{} is not public",
                field.name()
            );
        }
    }

    #[test]
    fn counting_is_bounded_by_the_number_of_kinds_not_by_use() {
        // A busy month must not cost more memory than a quiet one.
        let everything = agreeing_to(&[Purpose::CrashDiagnostics, Purpose::UsageAnalytics]);
        let mut counts = Counts::new();
        for _ in 0..10_000 {
            counts.record(Event::TrackAnalysed, &everything);
        }
        assert_eq!(counts.totals().len(), 1);
        assert_eq!(counts.total(Event::TrackAnalysed), 10_000);
    }

    #[test]
    fn every_event_belongs_to_exactly_one_agreement() {
        for event in Event::ALL {
            let purpose = Counts::purpose_for(event);
            assert_eq!(
                purpose == Purpose::CrashDiagnostics,
                event.is_a_fault(),
                "{event} is filed under the wrong agreement"
            );
        }
    }
}
