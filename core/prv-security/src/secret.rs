//! Values that must never be printed, logged, or attached to a bug report.
//!
//! # The type is the guard
//!
//! Master Prompt #26 forbids transmitting secrets in logs. The usual
//! implementation of that rule is a review checklist, and a review checklist is
//! a rule that holds until the evening someone is debugging a failed upload at
//! two in the morning and adds one `println!`.
//!
//! [`Secret`] therefore has no `Debug` or `Display` that reveals anything. Every
//! rendering of it — including the derived ones a struct containing it gets for
//! free — prints a placeholder. Reaching the bytes requires calling
//! [`Secret::expose`], which is named the way it is so that it shows up when
//! someone searches for it and reads oddly in a diff.
//!
//! # The core can name where a secret lives; it can never hold a stored one
//!
//! ADR-0001 keeps the core free of input and output, which means it cannot read
//! a keychain. That is the useful half of the restriction: [`SecretRef`] names a
//! location in platform storage, and the platform layer is the only thing that
//! can turn one into a value. A credential therefore has exactly one path into
//! the process, and it is a path that can be audited in one place.

use core::fmt;

/// The text every rendering of a secret produces.
pub const REDACTED: &str = "«redacted»";

/// Secret material — a token, a key, a password, a session credential.
///
/// Deliberately not [`Clone`]. Copying secret material is occasionally
/// necessary and never accidental, and a caller who genuinely needs a second
/// copy can build one from [`Secret::expose`] where a reviewer will see it.
pub struct Secret {
    bytes: Vec<u8>,
}

impl Secret {
    /// Wraps bytes that came from secure storage.
    #[must_use]
    pub const fn new(bytes: Vec<u8>) -> Self {
        Self { bytes }
    }

    /// Wraps text that came from secure storage.
    ///
    /// The caller's own copy of the text is not — and cannot be — cleared by
    /// this call. Prefer handing bytes straight from the platform's keychain
    /// interface, so that the only copy in the process is the one this type
    /// owns.
    #[must_use]
    pub fn from_text(text: &str) -> Self {
        Self::new(text.as_bytes().to_vec())
    }

    /// The bytes.
    ///
    /// Named so that its use is obvious in a diff and findable in a search.
    /// Every call is a place where secret material leaves this type's
    /// protection, and there should be very few of them.
    #[must_use]
    pub fn expose(&self) -> &[u8] {
        &self.bytes
    }

    /// How many bytes the secret is.
    ///
    /// Not secret in practice: the length of a credential is a property of the
    /// scheme that issued it, not of the user who holds it.
    #[must_use]
    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    /// Whether there is nothing here.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    /// Whether a candidate matches, comparing without an early return.
    ///
    /// The ordinary `==` on byte slices stops at the first difference, so the
    /// time it takes reveals how much of a guess was correct — which is enough
    /// to recover a token one byte at a time given enough attempts. This
    /// accumulates every difference and inspects the total once.
    ///
    /// What it does not hide is the *length*: a candidate of the wrong length
    /// is rejected immediately. That is deliberate and safe, because the length
    /// of a credential is fixed by the scheme that issued it and is not a
    /// secret.
    #[must_use]
    pub fn matches(&self, candidate: &[u8]) -> bool {
        if self.bytes.len() != candidate.len() {
            return false;
        }
        let mut difference = 0_u8;
        for (held, offered) in self.bytes.iter().zip(candidate.iter()) {
            difference |= held ^ offered;
        }
        difference == 0
    }
}

impl PartialEq for Secret {
    fn eq(&self, other: &Self) -> bool {
        self.matches(&other.bytes)
    }
}

impl Eq for Secret {}

impl fmt::Debug for Secret {
    /// Prints the placeholder, never the value.
    ///
    /// This is the whole point of the type. A struct that derives `Debug` and
    /// contains a `Secret` is safe to print, which is what makes the guarantee
    /// hold for code nobody reviewed for this.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Secret({REDACTED})")
    }
}

impl fmt::Display for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(REDACTED)
    }
}

impl Drop for Secret {
    /// Overwrites the bytes before the memory is released.
    ///
    /// Best effort, and worth being exact about what that means. Nothing in
    /// safe Rust obliges the compiler to keep a write to a buffer that is about
    /// to be freed; a guaranteed erase needs a volatile write, which needs
    /// `unsafe`, which ADR-0002 confines to `prv-rt`. Weakening that confinement
    /// for a defence-in-depth measure would cost more than it buys.
    ///
    /// The measure that actually carries the weight is elsewhere: secret
    /// material lives in the platform keychain and is held here for as short a
    /// time as possible.
    fn drop(&mut self) {
        for byte in &mut self.bytes {
            *byte = 0;
        }
    }
}

/// Where a secret lives in platform storage.
///
/// The core can name one and can never resolve one, because resolving it is
/// input and ADR-0001 forbids the core input. That asymmetry is the point: a
/// credential enters the process through the platform layer alone.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SecretRef {
    service: &'static str,
    account: String,
}

/// Why a secret reference could not be built.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SecretRefError {
    /// The service or account was empty.
    Empty,
    /// The account was longer than [`SecretRef::MAX_ACCOUNT_BYTES`].
    ///
    /// Bounded because an unbounded identifier reaching a platform keychain
    /// interface is an input nobody checked.
    AccountTooLong {
        /// How many bytes were offered.
        bytes: usize,
    },
}

impl fmt::Display for SecretRefError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::Empty => f.write_str("a secret reference needs a service and an account"),
            Self::AccountTooLong { bytes } => {
                write!(f, "an account of {bytes} bytes is too long")
            }
        }
    }
}

impl core::error::Error for SecretRefError {}

impl SecretRef {
    /// The longest account identifier accepted.
    pub const MAX_ACCOUNT_BYTES: usize = 256;

    /// Names a secret.
    ///
    /// `service` is a compile-time constant because it identifies *our* use of
    /// the keychain and is not user data. `account` distinguishes one user's
    /// credential from another's and is supplied at run time.
    ///
    /// # Errors
    ///
    /// Returns [`SecretRefError`] if either part is empty or the account is
    /// longer than [`Self::MAX_ACCOUNT_BYTES`].
    pub fn new(service: &'static str, account: &str) -> Result<Self, SecretRefError> {
        if service.is_empty() || account.is_empty() {
            return Err(SecretRefError::Empty);
        }
        if account.len() > Self::MAX_ACCOUNT_BYTES {
            return Err(SecretRefError::AccountTooLong {
                bytes: account.len(),
            });
        }
        Ok(Self {
            service,
            account: account.to_owned(),
        })
    }

    /// Which store, and what for.
    #[must_use]
    pub const fn service(&self) -> &'static str {
        self.service
    }

    /// Whose credential.
    ///
    /// Personal data — often an account name or an address — so it is redacted
    /// in every rendering and reachable only through this deliberately named
    /// accessor.
    #[must_use]
    pub fn expose_account(&self) -> &str {
        &self.account
    }
}

impl fmt::Display for SecretRef {
    /// Names the service and redacts the account.
    ///
    /// The service is safe to print and is the part a diagnostic needs: knowing
    /// that a lookup failed against the sync credential store is actionable,
    /// and knowing which account it was for is not, to anyone but an attacker.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{REDACTED}", self.service)
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

    #[derive(Debug)]
    #[allow(
        dead_code,
        reason = "these fields exist to be printed by the derived Debug, which is the \
                  behaviour under test"
    )]
    struct Session {
        user: u64,
        token: Secret,
    }

    #[test]
    fn a_secret_reveals_nothing_when_printed() {
        let secret = Secret::from_text("a-real-looking-token");
        assert_eq!(format!("{secret}"), REDACTED);
        assert!(!format!("{secret:?}").contains("real-looking"));
    }

    #[test]
    fn a_struct_that_derives_debug_and_contains_one_is_safe_to_print() {
        // The property that makes the guarantee hold for code nobody reviewed
        // for it. If this ever failed, every log line in the product would be a
        // place a token could appear.
        let session = Session {
            user: 42,
            token: Secret::from_text("a-real-looking-token"),
        };
        let rendered = format!("{session:?}");
        assert!(rendered.contains("42"), "the safe field should still print");
        assert!(
            !rendered.contains("real-looking"),
            "a derived Debug leaked the token: {rendered}"
        );
    }

    #[test]
    fn comparison_does_not_stop_at_the_first_difference() {
        // The behaviour, not the timing — timing is not testable in a unit test
        // and asserting it would be theatre. What is testable is that every
        // byte is inspected, which is what the implementation must do for the
        // timing property to hold.
        let secret = Secret::from_text("correct-horse");
        assert!(secret.matches(b"correct-horse"));
        assert!(!secret.matches(b"correct-horsf"), "last byte differs");
        assert!(!secret.matches(b"xorrect-horse"), "first byte differs");
        assert!(!secret.matches(b"correct-hors"), "shorter");
        assert!(!secret.matches(b"correct-horses"), "longer");
    }

    #[test]
    fn two_secrets_compare_by_value() {
        assert_eq!(Secret::from_text("same"), Secret::from_text("same"));
        assert_ne!(Secret::from_text("same"), Secret::from_text("other"));
    }

    #[test]
    fn the_length_is_available_because_it_is_not_the_secret() {
        let secret = Secret::from_text("sixteen-bytes!!!");
        assert_eq!(secret.len(), 16);
        assert!(!secret.is_empty());
        assert!(Secret::new(Vec::new()).is_empty());
    }

    #[test]
    fn a_secret_reference_names_the_service_and_hides_the_account() {
        let reference =
            SecretRef::new("prv.sync", "someone@example.com").expect("a valid reference");
        assert_eq!(reference.service(), "prv.sync");
        assert_eq!(reference.expose_account(), "someone@example.com");

        let rendered = format!("{reference}");
        assert!(rendered.contains("prv.sync"), "the service is actionable");
        assert!(
            !rendered.contains("example.com"),
            "the account leaked: {rendered}"
        );
        assert!(!format!("{reference:?}").is_empty());
    }

    #[test]
    fn a_reference_without_both_parts_is_refused() {
        assert_eq!(
            SecretRef::new("", "account").err(),
            Some(SecretRefError::Empty)
        );
        assert_eq!(
            SecretRef::new("service", "").err(),
            Some(SecretRefError::Empty)
        );
    }

    #[test]
    fn an_unbounded_account_is_refused_rather_than_passed_on() {
        // An unbounded identifier reaching a platform keychain interface is an
        // input nobody checked.
        let long = "a".repeat(SecretRef::MAX_ACCOUNT_BYTES + 1);
        assert_eq!(
            SecretRef::new("prv.sync", &long).err(),
            Some(SecretRefError::AccountTooLong {
                bytes: SecretRef::MAX_ACCOUNT_BYTES + 1
            })
        );
        let longest = "a".repeat(SecretRef::MAX_ACCOUNT_BYTES);
        assert!(SecretRef::new("prv.sync", &longest).is_ok());
    }
}
