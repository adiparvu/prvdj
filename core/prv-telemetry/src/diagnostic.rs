//! A report about something that went wrong.
//!
//! # A diagnostic is checked before it can be sent, not trusted
//!
//! `prv-security` makes it impossible to put a secret in a
//! [`Record`](prv_security::redaction::Record) — the field constructor takes no
//! value. What it cannot do is stop somebody attaching a *personal* field and
//! sending it anyway, because personal information is legitimately includable
//! when the user has agreed.
//!
//! [`Diagnostic::may_be_sent`] is that decision, in one place. It answers three
//! questions in a fixed order, and the order is the point: is this the kind of
//! report they agreed to, does it contain anything personal, and if so did they
//! agree to *that*. A report containing credential material is refused whatever
//! the answers, permanently — `prv-security`'s rule that no agreement makes a
//! secret sendable, enforced at the one place it could be violated.

use prv_security::redaction::{Record, Sensitivity};
use prv_security::{Consents, Purpose};

use crate::event::Event;

/// Why a report may not leave the device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Withheld {
    /// The user has not agreed to reports of this kind.
    NotAgreedTo {
        /// What they would have to agree to.
        purpose: Purpose,
    },
    /// It contains personal information and the user has not agreed to that.
    ///
    /// Distinct from [`Self::NotAgreedTo`] because the remedy is different and
    /// so is the honest question: not "may we send reports" but "this one
    /// mentions your files — may we send it".
    PersonalWithoutAgreement,
    /// It contains credential material.
    ///
    /// Never sendable. No agreement reaches this, at any setting, because a user
    /// cannot consent on behalf of the service that issued the credential.
    ContainsASecret,
}

impl Withheld {
    /// Whether asking the user could change the answer.
    #[must_use]
    pub const fn is_addressable_by_the_user(self) -> bool {
        !matches!(self, Self::ContainsASecret)
    }

    /// A stable identifier, for storage and localisation.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::NotAgreedTo { .. } => "withheld.not_agreed_to",
            Self::PersonalWithoutAgreement => "withheld.personal_without_agreement",
            Self::ContainsASecret => "withheld.contains_a_secret",
        }
    }
}

/// A report about one fault.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    event: Event,
    record: Record,
}

impl Diagnostic {
    /// Builds a report about a fault.
    #[must_use]
    pub const fn about(event: Event, record: Record) -> Self {
        Self { event, record }
    }

    /// What went wrong.
    #[must_use]
    pub const fn event(&self) -> Event {
        self.event
    }

    /// The detail.
    #[must_use]
    pub const fn record(&self) -> &Record {
        &self.record
    }

    /// The most careful handling anything in it needs.
    #[must_use]
    pub fn sensitivity(&self) -> Sensitivity {
        self.record.sensitivity()
    }

    /// Whether this may leave the device.
    ///
    /// # Errors
    ///
    /// Returns [`Withheld`] saying why not, and whether asking the user could
    /// change the answer — so an interface never offers a prompt that cannot be
    /// honoured.
    pub fn may_be_sent(&self, consents: &Consents) -> Result<(), Withheld> {
        // A secret first, because nothing below it can override this and a
        // reader should not have to check that it does not.
        if self.sensitivity() == Sensitivity::Secret {
            return Err(Withheld::ContainsASecret);
        }

        let purpose = if self.event.is_a_fault() {
            Purpose::CrashDiagnostics
        } else {
            Purpose::UsageAnalytics
        };
        if !consents.allows(purpose) {
            return Err(Withheld::NotAgreedTo { purpose });
        }

        // Personal information rides on the same agreement only when the user
        // was told it would. There is no separate purpose for it in
        // `prv-security`; until there is, the safe reading is that agreeing to
        // send crash reports is agreeing to send *reports*, and anything that
        // names the user's own files needs its own answer.
        if self.sensitivity() == Sensitivity::Personal {
            return Err(Withheld::PersonalWithoutAgreement);
        }

        Ok(())
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
    use prv_security::redaction::Field;

    fn agreeing_to(purpose: Purpose) -> Consents {
        let mut consents = Consents::none();
        consents.grant(purpose, 1);
        consents
    }

    fn a_crash_report() -> Diagnostic {
        Diagnostic::about(
            Event::PluginCrashed,
            Record::new("plugin.crashed")
                .with(Field::identifier("plugin", 4))
                .with(Field::number("bypasses", 2))
                .with(Field::key("tier", "tier.wasm")),
        )
    }

    #[test]
    fn a_public_report_goes_when_the_user_agreed_to_reports() {
        let report = a_crash_report();
        assert_eq!(report.sensitivity(), Sensitivity::Public);
        assert!(report
            .may_be_sent(&agreeing_to(Purpose::CrashDiagnostics))
            .is_ok());
        assert_eq!(report.event(), Event::PluginCrashed);
        assert!(!report.record().fields().is_empty());
    }

    #[test]
    fn nothing_goes_without_the_agreement_that_covers_it() {
        let report = a_crash_report();
        assert_eq!(
            report.may_be_sent(&Consents::none()).err(),
            Some(Withheld::NotAgreedTo {
                purpose: Purpose::CrashDiagnostics
            })
        );
        assert_eq!(
            report
                .may_be_sent(&agreeing_to(Purpose::UsageAnalytics))
                .err(),
            Some(Withheld::NotAgreedTo {
                purpose: Purpose::CrashDiagnostics
            }),
            "agreeing to usage counting sent a crash report"
        );
    }

    #[test]
    fn a_report_naming_the_users_own_files_needs_its_own_answer() {
        // Agreeing to send crash reports is agreeing to send reports. It is not
        // agreeing to send a list of what is on the machine.
        let report = Diagnostic::about(
            Event::AnalysisFailed,
            Record::new("analysis.failed").with(Field::personal("path")),
        );
        assert_eq!(report.sensitivity(), Sensitivity::Personal);
        assert_eq!(
            report
                .may_be_sent(&agreeing_to(Purpose::CrashDiagnostics))
                .err(),
            Some(Withheld::PersonalWithoutAgreement)
        );
    }

    #[test]
    fn a_report_containing_a_credential_never_goes_at_any_setting() {
        // prv-security's rule that no agreement makes a secret sendable,
        // enforced at the one place it could be violated.
        let report = Diagnostic::about(
            Event::CloudUnreachable,
            Record::new("sync.failed")
                .with(Field::key("service", "prv.sync"))
                .with(Field::secret("token")),
        );

        let mut everything = Consents::none();
        let mut ordinal = 0;
        for purpose in Purpose::ALL {
            ordinal += 1;
            everything.grant(purpose, ordinal);
        }

        assert_eq!(
            report.may_be_sent(&everything).err(),
            Some(Withheld::ContainsASecret)
        );
        assert!(!Withheld::ContainsASecret.is_addressable_by_the_user());
        assert!(Withheld::PersonalWithoutAgreement.is_addressable_by_the_user());
        assert!(Withheld::NotAgreedTo {
            purpose: Purpose::CrashDiagnostics
        }
        .is_addressable_by_the_user());
    }

    #[test]
    fn a_secret_is_refused_before_anything_else_is_considered() {
        // The order matters: a reader should not have to check that a later
        // rule does not override this one.
        let report = Diagnostic::about(
            Event::CloudUnreachable,
            Record::new("sync.failed")
                .with(Field::personal("account"))
                .with(Field::secret("token")),
        );
        assert_eq!(
            report.may_be_sent(&Consents::none()).err(),
            Some(Withheld::ContainsASecret),
            "the report was refused for the lesser reason"
        );
    }

    #[test]
    fn withheld_keys_are_distinct() {
        let keys = [
            Withheld::NotAgreedTo {
                purpose: Purpose::CrashDiagnostics,
            }
            .key(),
            Withheld::PersonalWithoutAgreement.key(),
            Withheld::ContainsASecret.key(),
        ];
        for (index, key) in keys.iter().enumerate() {
            for (other, value) in keys.iter().enumerate() {
                assert!(index == other || key != value, "two reasons share {key}");
            }
        }
    }
}
