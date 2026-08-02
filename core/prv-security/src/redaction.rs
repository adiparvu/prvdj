//! What a diagnostic record is allowed to contain.
//!
//! # Redaction by construction, not by discipline
//!
//! Master Prompt #26 requires structured logging to redact by field type rather
//! than by the author remembering. The strongest reading of that is the one
//! taken here: a log field that holds a secret does not exist. [`Field::secret`]
//! takes a name and *no value*, so a token cannot be placed in a record even by
//! someone who wants to.
//!
//! [`Field::personal`] is the same shape for the same reason, one step weaker.
//! A record notes that a personal value was there and withheld, which keeps the
//! shape of a log line stable and makes the redaction visible rather than
//! leaving a reader unsure whether the field was empty or removed.
//!
//! # Then what does a diagnostic actually say?
//!
//! Derived facts. Not the file path, but its extension and whether it resolved;
//! not the track title, but the identifier the document already uses. This is a
//! constraint that improves logs: "opening a file failed" with a path is a line
//! someone reads once, and "opening a file failed, extension=flac, exists=false,
//! bytes=0" is a line that survives being pasted into a report by a user who
//! would rather not send their music library along with it.
//!
//! # No consent makes a secret loggable
//!
//! [`Sensitivity::may_be_included_with_consent`] answers true for personal data
//! and false for secrets, permanently. A user can agree to send diagnostics
//! containing their own information; nobody can agree on behalf of the service
//! that issued a token.

use core::fmt;

use crate::secret::REDACTED;

/// The text a withheld personal value renders as.
pub const WITHHELD: &str = "«personal»";

/// How careful something has to be handled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum Sensitivity {
    /// Safe to record and to send. Says nothing about a person.
    Public,
    /// Information about a person.
    ///
    /// Withheld by default, and includable in a diagnostic the user chooses to
    /// send. Their information, their decision.
    Personal,
    /// Credential material.
    ///
    /// Never recorded, never sent, at any consent setting.
    Secret,
}

impl Sensitivity {
    /// Every level, least sensitive first.
    pub const ALL: [Self; 3] = [Self::Public, Self::Personal, Self::Secret];

    /// Whether a user's agreement can put this in a diagnostic they send.
    ///
    /// False for secrets, permanently. A user can consent to sharing their own
    /// information; they cannot consent on behalf of the service that issued a
    /// credential, and a leaked token is not undone by having been agreed to.
    #[must_use]
    pub const fn may_be_included_with_consent(self) -> bool {
        matches!(self, Self::Public | Self::Personal)
    }

    /// Whether this may be recorded with no agreement at all.
    #[must_use]
    pub const fn is_public(self) -> bool {
        matches!(self, Self::Public)
    }

    /// A stable identifier, for storage and localisation.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Public => "sensitivity.public",
            Self::Personal => "sensitivity.personal",
            Self::Secret => "sensitivity.secret",
        }
    }
}

/// The value side of a diagnostic field.
///
/// Only the public shapes carry data. The two withheld shapes carry none,
/// because a field that cannot hold a value cannot leak one.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Value {
    /// A stable identifier from the product's own vocabulary.
    Key(&'static str),
    /// A count, a duration in samples, a size.
    Number(i64),
    /// An opaque identifier from the document — a placement, a lane, a track.
    Identifier(u64),
    /// A yes or no.
    Flag(bool),
    /// A personal value, deliberately absent.
    WithheldPersonal,
    /// A secret, deliberately absent and never otherwise.
    WithheldSecret,
}

/// One named thing in a diagnostic record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    name: &'static str,
    value: Value,
}

impl Field {
    /// A value from the product's own vocabulary — a state, a reason, a target.
    #[must_use]
    pub const fn key(name: &'static str, value: &'static str) -> Self {
        Self {
            name,
            value: Value::Key(value),
        }
    }

    /// A count, a size, a position.
    #[must_use]
    pub const fn number(name: &'static str, value: i64) -> Self {
        Self {
            name,
            value: Value::Number(value),
        }
    }

    /// An identifier the document already uses.
    ///
    /// Public because it names a row in the user's own project, not a person.
    /// An identifier that *did* name a person would be a pseudonym, which is
    /// personal data however opaque it looks, and belongs in
    /// [`Field::personal`].
    #[must_use]
    pub const fn identifier(name: &'static str, value: u64) -> Self {
        Self {
            name,
            value: Value::Identifier(value),
        }
    }

    /// A yes or no.
    #[must_use]
    pub const fn flag(name: &'static str, value: bool) -> Self {
        Self {
            name,
            value: Value::Flag(value),
        }
    }

    /// Records that a personal value was here, without recording it.
    ///
    /// Takes no value on purpose. What the reader of a log needs is to know the
    /// field existed; what they do not need is the user's file path.
    #[must_use]
    pub const fn personal(name: &'static str) -> Self {
        Self {
            name,
            value: Value::WithheldPersonal,
        }
    }

    /// Records that a secret was here, without recording it.
    ///
    /// Takes no value on purpose, and this is the strongest guarantee in the
    /// module: there is no expression in this crate's vocabulary that puts
    /// credential material into a diagnostic.
    #[must_use]
    pub const fn secret(name: &'static str) -> Self {
        Self {
            name,
            value: Value::WithheldSecret,
        }
    }

    /// What this field is called.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        self.name
    }

    /// How carefully it must be handled.
    #[must_use]
    pub const fn sensitivity(&self) -> Sensitivity {
        match self.value {
            Value::Key(_) | Value::Number(_) | Value::Identifier(_) | Value::Flag(_) => {
                Sensitivity::Public
            }
            Value::WithheldPersonal => Sensitivity::Personal,
            Value::WithheldSecret => Sensitivity::Secret,
        }
    }
}

impl fmt::Display for Field {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}=", self.name)?;
        match self.value {
            Value::Key(value) => f.write_str(value),
            Value::Number(value) => write!(f, "{value}"),
            Value::Identifier(value) => write!(f, "{value}"),
            Value::Flag(value) => f.write_str(if value { "true" } else { "false" }),
            Value::WithheldPersonal => f.write_str(WITHHELD),
            Value::WithheldSecret => f.write_str(REDACTED),
        }
    }
}

/// A diagnostic line.
///
/// The message is a stable key rather than a sentence, for the same reason
/// every other user-visible string in the core is: the core knows no language,
/// and a log that is grepped by key survives translation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    event: &'static str,
    fields: Vec<Field>,
}

impl Record {
    /// The most fields one record may carry.
    ///
    /// Bounded so that a loop appending in an error path produces a long log
    /// rather than an unbounded allocation.
    pub const MAX_FIELDS: usize = 32;

    /// Starts a record.
    #[must_use]
    pub const fn new(event: &'static str) -> Self {
        Self {
            event,
            fields: Vec::new(),
        }
    }

    /// Adds a field, up to [`Self::MAX_FIELDS`].
    ///
    /// Fields beyond the limit are dropped rather than allocated, and
    /// [`Self::is_full`] says so.
    #[must_use]
    pub fn with(mut self, field: Field) -> Self {
        if self.fields.len() < Self::MAX_FIELDS {
            self.fields.push(field);
        }
        self
    }

    /// What happened.
    #[must_use]
    pub const fn event(&self) -> &'static str {
        self.event
    }

    /// The fields, in the order they were added.
    #[must_use]
    pub fn fields(&self) -> &[Field] {
        &self.fields
    }

    /// Whether further fields would be dropped.
    #[must_use]
    pub fn is_full(&self) -> bool {
        self.fields.len() >= Self::MAX_FIELDS
    }

    /// The highest sensitivity any field carries.
    ///
    /// What a transport decides on. A record that is entirely public may be
    /// sent without asking; anything above that is the user's call.
    #[must_use]
    pub fn sensitivity(&self) -> Sensitivity {
        self.fields
            .iter()
            .map(Field::sensitivity)
            .max()
            .unwrap_or(Sensitivity::Public)
    }
}

impl fmt::Display for Record {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.event)?;
        for field in &self.fields {
            write!(f, " {field}")?;
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

    #[test]
    fn a_record_renders_public_values_and_withholds_the_rest() {
        let record = Record::new("library.open_failed")
            .with(Field::key("reason", "reason.not_found"))
            .with(Field::identifier("track", 91))
            .with(Field::number("bytes", 0))
            .with(Field::flag("retried", true))
            .with(Field::personal("path"))
            .with(Field::secret("token"));

        let rendered = record.to_string();
        assert!(rendered.starts_with("library.open_failed"));
        assert!(rendered.contains("reason=reason.not_found"));
        assert!(rendered.contains("track=91"));
        assert!(rendered.contains("bytes=0"));
        assert!(rendered.contains("retried=true"));
        assert!(rendered.contains(&format!("path={WITHHELD}")));
        assert!(rendered.contains(&format!("token={REDACTED}")));
    }

    #[test]
    fn the_shape_of_a_line_is_stable_whether_or_not_a_value_was_withheld() {
        // What makes redaction visible rather than leaving a reader unsure
        // whether a field was empty or removed.
        let record = Record::new("sync.failed").with(Field::personal("account"));
        assert_eq!(record.fields().len(), 1);
        assert!(record.to_string().contains("account="));
    }

    #[test]
    fn sensitivity_is_taken_from_the_worst_field() {
        // What a transport decides on. A record that is entirely public may be
        // sent without asking; anything above that is the user's call.
        let public = Record::new("e").with(Field::number("n", 1));
        assert_eq!(public.sensitivity(), Sensitivity::Public);

        let personal = public.clone().with(Field::personal("path"));
        assert_eq!(personal.sensitivity(), Sensitivity::Personal);

        let secret = personal.clone().with(Field::secret("token"));
        assert_eq!(secret.sensitivity(), Sensitivity::Secret);

        assert_eq!(
            Record::new("empty").sensitivity(),
            Sensitivity::Public,
            "a record with no fields discloses nothing"
        );
    }

    #[test]
    fn no_agreement_can_put_a_secret_in_a_diagnostic() {
        // A user can consent to sharing their own information. They cannot
        // consent on behalf of the service that issued a credential, and a
        // leaked token is not undone by having been agreed to.
        assert!(!Sensitivity::Secret.may_be_included_with_consent());
        assert!(Sensitivity::Personal.may_be_included_with_consent());
        assert!(Sensitivity::Public.may_be_included_with_consent());
        assert!(Sensitivity::Public.is_public());
        assert!(!Sensitivity::Personal.is_public());
    }

    #[test]
    fn sensitivity_orders_from_least_to_most_careful() {
        assert!(Sensitivity::Public < Sensitivity::Personal);
        assert!(Sensitivity::Personal < Sensitivity::Secret);
        assert_eq!(Sensitivity::ALL.len(), 3);
    }

    #[test]
    fn a_record_stops_growing_rather_than_growing_without_limit() {
        let mut record = Record::new("noisy");
        for _ in 0..(Record::MAX_FIELDS * 2) {
            record = record.with(Field::number("n", 1));
        }
        assert_eq!(record.fields().len(), Record::MAX_FIELDS);
        assert!(record.is_full());
    }

    #[test]
    fn every_field_reports_the_sensitivity_its_constructor_implies() {
        assert_eq!(Field::key("a", "b").sensitivity(), Sensitivity::Public);
        assert_eq!(Field::number("a", 1).sensitivity(), Sensitivity::Public);
        assert_eq!(Field::identifier("a", 1).sensitivity(), Sensitivity::Public);
        assert_eq!(Field::flag("a", true).sensitivity(), Sensitivity::Public);
        assert_eq!(Field::personal("a").sensitivity(), Sensitivity::Personal);
        assert_eq!(Field::secret("a").sensitivity(), Sensitivity::Secret);
        assert_eq!(Field::personal("path").name(), "path");
    }

    #[test]
    fn sensitivity_keys_are_distinct() {
        let keys: Vec<&str> = Sensitivity::ALL.iter().map(|s| s.key()).collect();
        for (index, key) in keys.iter().enumerate() {
            for (other, value) in keys.iter().enumerate() {
                assert!(index == other || key != value, "two levels share {key}");
            }
        }
    }
}
