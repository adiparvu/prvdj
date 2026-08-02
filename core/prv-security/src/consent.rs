//! What the user has agreed to, and where their work is processed.
//!
//! # Nothing is agreed to by default
//!
//! Every purpose starts withheld. Not "withheld for the sensitive ones" — every
//! one, including the harmless-sounding ones, because the moment a default is
//! "on for the harmless ones" somebody has to decide which those are, and that
//! decision is made by whoever is shipping the feature rather than by the person
//! whose data it is.
//!
//! There is no opt-out anywhere in this module. [`Consents::default`] grants
//! nothing, so a bug that fails to load a stored answer fails *closed*.
//!
//! # Training is not implied by anything
//!
//! Master Prompt #26 forbids using a user's projects to train models without
//! explicit permission. [`Purpose::ModelTraining`] is therefore its own purpose,
//! granted by its own act. Agreeing to cloud analysis is agreeing to have a
//! track analysed, and nothing else; there is deliberately no bundle, no
//! "improve the product" umbrella, and no way to reach training by granting
//! something adjacent. A test asserts exactly that.
//!
//! # The user is told where their work is processed
//!
//! Every purpose says whether it runs on the device or leaves it
//! ([`Purpose::location`]), and whether what leaves is the user's own content
//! rather than a fact about it ([`Purpose::sends_content`]). Master Prompt #26
//! requires external processing to be clearly indicated; that indication has to
//! be computed from something, and this is the something.

use core::fmt;

/// Where work happens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ProcessingLocation {
    /// On the user's own machine.
    OnDevice,
    /// On a server.
    Cloud,
}

impl ProcessingLocation {
    /// Whether anything at all leaves the machine.
    #[must_use]
    pub const fn leaves_the_device(self) -> bool {
        matches!(self, Self::Cloud)
    }

    /// A stable identifier, for storage and localisation.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::OnDevice => "processing.on_device",
            Self::Cloud => "processing.cloud",
        }
    }
}

impl fmt::Display for ProcessingLocation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.key())
    }
}

/// Something the product might do that needs the user's agreement first.
///
/// A purpose is a *reason*, not a feature. The distinction matters: a user
/// agrees to their audio being analysed on a server, not to "the cloud", and an
/// agreement phrased as a capability rather than a reason cannot be withdrawn
/// meaningfully.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum Purpose {
    /// Send audio to a server to be analysed for tempo, key and structure.
    CloudAnalysis,

    /// Send a written instruction to a language model to be interpreted.
    ///
    /// Separate from [`Self::CloudAnalysis`] because the material is different
    /// in kind: one is a recording the user owns, the other is something they
    /// wrote, which may mention anything at all.
    CloudLanguage,

    /// Separate a track into stems on a server rather than on the device.
    CloudStemSeparation,

    /// Keep projects in step across the user's own devices.
    ProjectSync,

    /// Let named people open and edit a project.
    Collaboration,

    /// Send a report when the application stops unexpectedly.
    CrashDiagnostics,

    /// Send counts of which features are used.
    UsageAnalytics,

    /// Use the user's own projects to improve the models the product ships.
    ///
    /// Its own purpose, granted by its own act, reachable from nothing else.
    /// Master Prompt #26 requires explicit permission, and permission that
    /// arrives as a side effect of agreeing to something else is not explicit.
    ModelTraining,

    /// Learn from the user's decisions to shape suggestions, on the device.
    ///
    /// On-device and still a purpose. It is inference about a person from their
    /// behaviour, and the fact that it never leaves the machine makes it
    /// low-risk rather than exempt.
    PersonalisedSuggestions,
}

impl Purpose {
    /// Every purpose, so a consent screen cannot omit one.
    pub const ALL: [Self; 9] = [
        Self::CloudAnalysis,
        Self::CloudLanguage,
        Self::CloudStemSeparation,
        Self::ProjectSync,
        Self::Collaboration,
        Self::CrashDiagnostics,
        Self::UsageAnalytics,
        Self::ModelTraining,
        Self::PersonalisedSuggestions,
    ];

    /// Where the work happens.
    #[must_use]
    pub const fn location(self) -> ProcessingLocation {
        match self {
            Self::CloudAnalysis
            | Self::CloudLanguage
            | Self::CloudStemSeparation
            | Self::ProjectSync
            | Self::Collaboration
            | Self::CrashDiagnostics
            | Self::UsageAnalytics
            | Self::ModelTraining => ProcessingLocation::Cloud,
            Self::PersonalisedSuggestions => ProcessingLocation::OnDevice,
        }
    }

    /// Whether the user's own material leaves the device, rather than a fact
    /// about it.
    ///
    /// The distinction a consent screen has to make honestly. "We send the
    /// tempo we measured" and "we send the recording" are both cloud
    /// processing, and a user who is told only the first is being misled.
    #[must_use]
    pub const fn sends_content(self) -> bool {
        matches!(
            self,
            Self::CloudAnalysis
                | Self::CloudLanguage
                | Self::CloudStemSeparation
                | Self::ProjectSync
                | Self::Collaboration
                | Self::ModelTraining
        )
    }

    /// A stable identifier, for storage and localisation.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::CloudAnalysis => "purpose.cloud_analysis",
            Self::CloudLanguage => "purpose.cloud_language",
            Self::CloudStemSeparation => "purpose.cloud_stem_separation",
            Self::ProjectSync => "purpose.project_sync",
            Self::Collaboration => "purpose.collaboration",
            Self::CrashDiagnostics => "purpose.crash_diagnostics",
            Self::UsageAnalytics => "purpose.usage_analytics",
            Self::ModelTraining => "purpose.model_training",
            Self::PersonalisedSuggestions => "purpose.personalised_suggestions",
        }
    }
}

impl fmt::Display for Purpose {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.key())
    }
}

/// An agreement, and when it was given.
///
/// The ordinal is caller-supplied for the reason `prv-learning`'s is: ADR-0001
/// keeps the core away from the clock. A durable record of *when* a user agreed
/// is a real requirement and it belongs where the platform's calendar is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Grant {
    purpose: Purpose,
    ordinal: u64,
}

impl Grant {
    /// What was agreed to.
    #[must_use]
    pub const fn purpose(self) -> Purpose {
        self.purpose
    }

    /// Where in the sequence of decisions the agreement was given.
    #[must_use]
    pub const fn ordinal(self) -> u64 {
        self.ordinal
    }
}

/// Why something was not allowed to happen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ConsentRequired {
    /// What the user would have to agree to.
    pub purpose: Purpose,
}

impl fmt::Display for ConsentRequired {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} has not been agreed to", self.purpose)
    }
}

impl core::error::Error for ConsentRequired {}

/// Everything the user has agreed to.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Consents {
    granted: Vec<Grant>,
}

impl Consents {
    /// Nothing agreed to.
    ///
    /// The only starting state there is. There is no constructor that begins
    /// with anything granted, so a bug that fails to load a stored answer fails
    /// closed.
    #[must_use]
    pub const fn none() -> Self {
        Self {
            granted: Vec::new(),
        }
    }

    /// Records an agreement.
    ///
    /// `ordinal` is a caller-supplied sequence number. Returns whether this
    /// changed anything — re-granting something already granted keeps the
    /// original ordinal, because the question a record answers is *when did
    /// they first agree*.
    pub fn grant(&mut self, purpose: Purpose, ordinal: u64) -> bool {
        if self.allows(purpose) {
            return false;
        }
        self.granted.push(Grant { purpose, ordinal });
        self.granted.sort_unstable_by_key(|grant| grant.purpose);
        true
    }

    /// Withdraws an agreement.
    ///
    /// Every purpose is withdrawable, always, with no exception and nothing to
    /// confirm. Returns whether anything changed.
    pub fn withdraw(&mut self, purpose: Purpose) -> bool {
        let before = self.granted.len();
        self.granted.retain(|grant| grant.purpose != purpose);
        self.granted.len() != before
    }

    /// Withdraws everything.
    pub fn withdraw_all(&mut self) {
        self.granted.clear();
    }

    /// Whether a purpose is agreed to.
    #[must_use]
    pub fn allows(&self, purpose: Purpose) -> bool {
        self.granted.iter().any(|grant| grant.purpose == purpose)
    }

    /// Whether a purpose is agreed to, as a result.
    ///
    /// # Errors
    ///
    /// Returns [`ConsentRequired`] naming what the user would have to agree to,
    /// so that an interface can ask the right question rather than reporting a
    /// failure.
    pub fn check(&self, purpose: Purpose) -> Result<(), ConsentRequired> {
        if self.allows(purpose) {
            Ok(())
        } else {
            Err(ConsentRequired { purpose })
        }
    }

    /// When the user agreed to something, if they did.
    #[must_use]
    pub fn granted_at(&self, purpose: Purpose) -> Option<u64> {
        self.granted
            .iter()
            .find(|grant| grant.purpose == purpose)
            .map(|grant| grant.ordinal)
    }

    /// Everything agreed to, in a stable order.
    #[must_use]
    pub fn grants(&self) -> &[Grant] {
        &self.granted
    }

    /// Whether anything at all leaves the device under these agreements.
    ///
    /// What an interface shows as a single honest indicator, and what a user
    /// checks before a set in a venue with no network worth trusting.
    #[must_use]
    pub fn anything_leaves_the_device(&self) -> bool {
        self.granted
            .iter()
            .any(|grant| grant.purpose.location().leaves_the_device())
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
    fn nothing_is_agreed_to_by_default() {
        // Checked over every purpose rather than asserted about the sensitive
        // ones, because "on by default for the harmless ones" makes somebody
        // decide which those are, and it is not the person whose data it is.
        for consents in [Consents::none(), Consents::default()] {
            for purpose in Purpose::ALL {
                assert!(
                    !consents.allows(purpose),
                    "{purpose} was granted without being asked for"
                );
            }
            assert!(!consents.anything_leaves_the_device());
        }
    }

    #[test]
    fn training_on_a_users_work_is_reachable_from_nothing_else() {
        // Master Prompt #26's requirement, as a property rather than a promise:
        // agreeing to everything else must not agree to this.
        let mut consents = Consents::none();
        for (ordinal, purpose) in Purpose::ALL.into_iter().enumerate() {
            if purpose == Purpose::ModelTraining {
                continue;
            }
            consents.grant(purpose, ordinal as u64);
        }
        assert!(
            !consents.allows(Purpose::ModelTraining),
            "training was granted as a side effect of agreeing to something else"
        );
        assert_eq!(
            consents.check(Purpose::ModelTraining).err(),
            Some(ConsentRequired {
                purpose: Purpose::ModelTraining
            })
        );
    }

    #[test]
    fn every_purpose_can_be_withdrawn_and_withdrawal_is_immediate() {
        for purpose in Purpose::ALL {
            let mut consents = Consents::none();
            assert!(consents.grant(purpose, 1));
            assert!(consents.allows(purpose));
            assert!(
                consents.withdraw(purpose),
                "{purpose} could not be withdrawn"
            );
            assert!(!consents.allows(purpose));
        }
    }

    #[test]
    fn withdrawing_everything_returns_to_the_starting_state() {
        // A cleared set must be indistinguishable from one that was never
        // granted, or "withdraw" is a word for something else.
        let mut consents = Consents::none();
        for (ordinal, purpose) in Purpose::ALL.into_iter().enumerate() {
            consents.grant(purpose, ordinal as u64);
        }
        consents.withdraw_all();
        assert_eq!(consents, Consents::none());
    }

    #[test]
    fn agreeing_twice_keeps_the_first_answer() {
        // The question a record answers is when they *first* agreed.
        let mut consents = Consents::none();
        assert!(consents.grant(Purpose::ProjectSync, 7));
        assert!(!consents.grant(Purpose::ProjectSync, 99));
        assert_eq!(consents.granted_at(Purpose::ProjectSync), Some(7));
        assert_eq!(consents.grants().len(), 1);
        assert_eq!(consents.granted_at(Purpose::CloudAnalysis), None);
    }

    #[test]
    fn a_purpose_that_sends_the_users_own_material_says_so() {
        // "We send the tempo we measured" and "we send the recording" are both
        // cloud processing, and a user told only the first is being misled.
        assert!(Purpose::CloudAnalysis.sends_content());
        assert!(Purpose::ModelTraining.sends_content());
        assert!(
            !Purpose::UsageAnalytics.sends_content(),
            "usage counts are facts about behaviour, not the user's material"
        );
        assert!(!Purpose::CrashDiagnostics.sends_content());

        for purpose in Purpose::ALL {
            if purpose.sends_content() {
                assert!(
                    purpose.location().leaves_the_device(),
                    "{purpose} sends content without leaving the device"
                );
            }
        }
    }

    #[test]
    fn on_device_work_is_still_a_purpose() {
        // Inference about a person from their behaviour is low-risk when it
        // never leaves the machine. It is not exempt.
        assert_eq!(
            Purpose::PersonalisedSuggestions.location(),
            ProcessingLocation::OnDevice
        );
        assert!(!ProcessingLocation::OnDevice.leaves_the_device());
        assert!(!Consents::none().allows(Purpose::PersonalisedSuggestions));
    }

    #[test]
    fn the_indicator_reflects_only_what_actually_leaves() {
        let mut consents = Consents::none();
        consents.grant(Purpose::PersonalisedSuggestions, 1);
        assert!(
            !consents.anything_leaves_the_device(),
            "on-device learning must not light the network indicator"
        );

        consents.grant(Purpose::CloudAnalysis, 2);
        assert!(consents.anything_leaves_the_device());

        consents.withdraw(Purpose::CloudAnalysis);
        assert!(!consents.anything_leaves_the_device());
    }

    #[test]
    fn purpose_keys_are_distinct() {
        let keys: Vec<&str> = Purpose::ALL.iter().map(|p| p.key()).collect();
        for (index, key) in keys.iter().enumerate() {
            for (other, value) in keys.iter().enumerate() {
                assert!(index == other || key != value, "two purposes share {key}");
            }
        }
        assert_ne!(
            ProcessingLocation::OnDevice.key(),
            ProcessingLocation::Cloud.key()
        );
    }
}
