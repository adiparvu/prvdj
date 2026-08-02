//! Counts and diagnostics.
//!
//! # One decision shapes this whole crate
//!
//! **Nothing is recorded without the agreement.** Not recorded-and-not-sent —
//! not recorded. The usual shape is to collect everything and gate the upload,
//! which is defensible in a diagram and indefensible in practice: keeping a
//! person's behaviour and deciding later not to send it is still having kept it,
//! and a bug in the gate becomes a disclosure instead of a missed opportunity.
//!
//! Everything else follows from that. There is no buffer, so there is nothing
//! for a mistake to release; withdrawing an agreement discards what was held,
//! because otherwise "withdraw" is a word for something else; and what a user is
//! shown is the same structure that would be sent, because a separately written
//! summary of "what we collect" is a document that drifts.
//!
//! # Counts, not a trace
//!
//! One number per event kind. No order, no times, no session identifier. A
//! sequence of timestamped events is a record of one person's evening; the same
//! events as totals answer "is anybody using this" and answer nothing else — and
//! the second is the question the product actually has.
//!
//! ADR-0001 keeps the core away from the clock, so the wrong version is not
//! merely discouraged here but unwritable. That is a coincidence rather than the
//! reason, and it is a good one.
//!
//! # Two agreements, not one
//!
//! A crash report and a usage count are different bargains: one is offered to
//! get something fixed, the other to help decide what to build. `prv-security`
//! keeps them as separate purposes and this crate never lets one stand in for
//! the other.

pub mod counts;
pub mod diagnostic;
pub mod event;

pub use counts::Counts;
pub use diagnostic::{Diagnostic, Withheld};
pub use event::Event;

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::panic,
        reason = "a test that cannot build its own fixture should fail loudly"
    )]

    use super::*;
    use prv_security::redaction::{Field, Record, Sensitivity};
    use prv_security::{Consents, Purpose};

    #[test]
    fn a_user_who_agreed_to_nothing_leaves_no_trace_anywhere() {
        // The whole crate, from the position most users are in.
        let nothing = Consents::none();
        let mut counts = Counts::new();

        for event in Event::ALL {
            assert!(!counts.record(event, &nothing));
        }
        assert!(counts.is_empty());

        let report = Diagnostic::about(
            Event::PluginCrashed,
            Record::new("plugin.crashed").with(Field::identifier("plugin", 1)),
        );
        assert!(report.may_be_sent(&nothing).is_err());
    }

    #[test]
    fn agreeing_to_one_thing_is_never_agreeing_to_the_other() {
        let mut counts = Counts::new();
        let mut usage_only = Consents::none();
        usage_only.grant(Purpose::UsageAnalytics, 1);

        assert!(counts.record(Event::PlanKept, &usage_only));
        assert!(!counts.record(Event::PluginCrashed, &usage_only));

        let crash = Diagnostic::about(Event::PluginCrashed, Record::new("plugin.crashed"));
        assert!(crash.may_be_sent(&usage_only).is_err());
    }

    #[test]
    fn everything_this_crate_would_send_is_public_by_construction() {
        // The counts are always public; a diagnostic may not be, and the one
        // that is not is refused rather than trimmed. Trimming would mean
        // sending something the author did not write and nobody reviewed.
        let mut everything = Consents::none();
        let mut ordinal = 0;
        for purpose in Purpose::ALL {
            ordinal += 1;
            everything.grant(purpose, ordinal);
        }

        let mut counts = Counts::new();
        for event in Event::ALL {
            counts.record(event, &everything);
        }
        assert_eq!(counts.report().sensitivity(), Sensitivity::Public);

        let personal = Diagnostic::about(
            Event::AnalysisFailed,
            Record::new("analysis.failed").with(Field::personal("path")),
        );
        assert!(
            personal.may_be_sent(&everything).is_err(),
            "a report naming the user's files was cleared to send"
        );
    }
}
