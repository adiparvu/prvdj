//! Security policy: who may act, what the user agreed to, what may be written
//! down, and what was decided.
//!
//! # Policy, not enforcement
//!
//! This crate decides. It does not check a signature, open a keychain, write a
//! log file or open a socket — ADR-0001 keeps all of that outside the core, and
//! the separation is what makes the rules testable. Every decision here is a
//! pure function of values, so the question "what can a viewer do" has an answer
//! that can be printed rather than a behaviour that has to be observed.
//!
//! The four modules answer four different questions, and they are deliberately
//! kept apart:
//!
//! | Module | Question |
//! |---|---|
//! | [`authorisation`] | May this subject do this? |
//! | [`consent`] | Has the user agreed to this happening at all? |
//! | [`redaction`] | May this be written down or sent? |
//! | [`audit`] | What was decided, and when? |
//!
//! Authorisation and consent are not the same check and must not be collapsed
//! into one. A user is *permitted* to have a track analysed on their own
//! project; whether it may be sent to a server is a separate question with a
//! separate answer, and merging them is how a product ends up treating "you own
//! this" as "we may do anything with it".
//!
//! # What this crate is not
//!
//! It is not entitlement. `prv-entitlements` answers what somebody paid for;
//! this answers whether they are allowed. The two fail differently and are
//! remedied differently — an upgrade against the owner of a project — and
//! offering the wrong remedy is worse than offering none. Neither crate depends
//! on the other, and the architecture check keeps entitlements out of the
//! engines entirely.
//!
//! # Jurisdictions are not modelled here
//!
//! Master Prompt #26 requires that jurisdiction-specific assumptions stay out of
//! the core, so that regulatory change is a policy edit rather than an
//! architectural one. Nothing in this crate names a territory, a statute or a
//! retention period. What it provides is the machinery those rules need:
//! purposes that are separately granted and separately withdrawable, a
//! sensitivity scale, and a record of what was decided. A rule that says
//! "diagnostics may not leave this region" is expressed by a transport that
//! consults [`consent`], not by a variant added here.

pub mod audit;
pub mod authorisation;
pub mod consent;
pub mod redaction;
pub mod secret;

pub use audit::{Actor, AuditLog, Decision, Entry};
pub use authorisation::{
    allows, authorise, Capability, PermissionSet, PluginId, Refusal, Role, Subject,
};
pub use consent::{ConsentRequired, Consents, Grant, ProcessingLocation, Purpose};
pub use redaction::{Field, Record, Sensitivity};
pub use secret::{Secret, SecretRef, SecretRefError};

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::panic,
        reason = "a test that cannot build its own fixture should fail loudly"
    )]

    use super::*;

    #[test]
    fn permission_and_agreement_are_two_different_questions() {
        // The property the crate's shape exists to protect. Owning a project
        // permits editing it; it does not agree to anything being sent
        // anywhere. A product that merged these would be treating "you own
        // this" as "we may do anything with it".
        let owner = Subject::user(Role::Owner);
        assert!(allows(owner, Capability::EditProject));

        let consents = Consents::none();
        assert!(
            consents.check(Purpose::CloudAnalysis).is_err(),
            "ownership agreed to cloud processing on the user's behalf"
        );
        assert!(consents.check(Purpose::ModelTraining).is_err());
    }

    #[test]
    fn a_full_decision_is_recorded_without_recording_anybody() {
        // The end-to-end shape: decide, record, render — and what comes out is
        // safe to attach to a support ticket.
        let mut log = AuditLog::new();
        let plugin = Subject::plugin(PluginId::new(4), PermissionSet::NONE);

        let refused = log.authorise(1, plugin, Capability::UseNetwork);
        assert!(refused.is_err());

        let rendered: Vec<String> = log.entries().map(Entry::to_string).collect();
        assert_eq!(rendered.len(), 1);
        for line in &rendered {
            assert!(line.starts_with("audit.decision"), "{line}");
        }
        for entry in log.entries() {
            assert_eq!(entry.render().sensitivity(), Sensitivity::Public);
        }
    }

    #[test]
    fn a_diagnostic_about_a_credential_failure_carries_no_credential() {
        // What this crate is for, in one line of code: everything a support
        // engineer needs, nothing an attacker could use.
        let reference = SecretRef::new("prv.sync", "someone@example.com").expect("a reference");
        let token = Secret::from_text("st-live-9f3c11aa");

        let record = Record::new("sync.authentication_failed")
            .with(Field::key("service", reference.service()))
            .with(Field::personal("account"))
            .with(Field::secret("token"))
            .with(Field::number("attempts", 3));

        let line = record.to_string();
        assert!(line.contains("prv.sync"));
        assert!(line.contains("attempts=3"));
        assert!(!line.contains("example.com"), "{line}");
        assert!(!line.contains("9f3c11aa"), "{line}");
        assert!(!format!("{token:?}").contains("9f3c11aa"));
        assert_eq!(record.sensitivity(), Sensitivity::Secret);
        assert!(!record.sensitivity().may_be_included_with_consent());
    }
}
