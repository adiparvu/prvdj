//! Which features a licence grants — and which are never withheld.
//!
//! # This crate is deliberately alone
//!
//! `prv-entitlements` depends on nothing, and nothing in the engines depends on
//! it. That is not tidiness: it is the mechanism by which Master Prompt #29's
//! promise is kept.
//!
//! The promise is that the product never artificially restricts essential
//! functionality. A promise like that decays. It survives the first release,
//! and then somebody adds one tier check inside the mixer because it was the
//! convenient place, and a year later nobody can say what the free tier
//! actually does without reading the DSP.
//!
//! So the check is structural. No engine crate names this one; the
//! architecture check enforces it, and a change that broke it fails the build
//! rather than a review. The consequence is that the audio graph, the planner
//! and the analysis pipeline behave *identically at every tier* — because they
//! cannot tell which tier they are running under. Entitlements are consulted at
//! the feature boundary, above all of them, and that is the only place they
//! exist.
//!
//! # What is never withheld
//!
//! [`Feature::is_essential`] is the list, and it is short and specific:
//! playing your own music, seeing your own library, opening and editing your own
//! projects, and getting your work back out. These stay available at every tier,
//! after any expiry, forever.
//!
//! The reasoning is Master Prompt #29's and Master Prompt #9's together: the
//! user owns their work, and a product that can hold it hostage does not really
//! mean that. A subscription buys *new* capability. It never buys back access to
//! what someone already made.
//!
//! # Expiry is not a lock
//!
//! [`Licence::expired`] keeps every essential feature and withdraws the rest.
//! An expired licence therefore behaves exactly like the free tier rather than
//! like a locked door — which is what "users retain ownership of their projects"
//! has to mean when the money stops.

use core::fmt;

/// What a user has paid for.
///
/// Ordered, so that comparison means what it reads like and a feature can be
/// gated by "at least this tier" rather than by a list that has to be kept in
/// step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Tier {
    /// No subscription.
    ///
    /// Not a trial and not a crippled build: a real, permanent tier at which
    /// the whole of a user's own work remains available to them.
    Free,
    /// The paid tier for someone who plays.
    Standard,
    /// The paid tier for someone who produces.
    Professional,
    /// Everything, including what is shared with collaborators.
    Studio,
}

impl Tier {
    /// Every tier, lowest first.
    pub const ALL: [Self; 4] = [Self::Free, Self::Standard, Self::Professional, Self::Studio];

    /// A stable identifier, for storage and localisation.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Free => "tier.free",
            Self::Standard => "tier.standard",
            Self::Professional => "tier.professional",
            Self::Studio => "tier.studio",
        }
    }
}

impl fmt::Display for Tier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.key())
    }
}

/// Something the product can do.
///
/// Enumerated rather than free-form so that a new capability cannot be gated
/// without a reviewer seeing the variant and its `is_essential` answer in the
/// same change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum Feature {
    /// Play music the user owns.
    Playback,
    /// Browse, search and organise the library.
    Library,
    /// Open, edit and save projects.
    ProjectEditing,
    /// Get finished work out as a file.
    ///
    /// Essential, and the one most likely to be argued about. A product that
    /// lets someone build a mix and then charges to get it out of the
    /// application is holding their work hostage, whatever the pricing page
    /// says. Master Prompt #29 forbids it and this is where that is enforced.
    Export,
    /// Analyse a track for tempo, key, structure and loudness.
    ///
    /// Essential because the library is unusable without it: a user cannot find
    /// the track they mean if nothing knows what key it is in.
    Analysis,

    /// Have the system plan a set.
    AiPlanning,
    /// Separate a track into stems.
    StemSeparation,
    /// Use the cloud for analysis and language.
    CloudAi,
    /// Synchronise projects between devices.
    CloudSync,
    /// Work on a project with someone else.
    Collaboration,
    /// Load third-party plugins.
    Plugins,
    /// Render at a higher resolution than the session.
    HighResolutionExport,
    /// Use the live performance surface.
    LivePerformance,
}

impl Feature {
    /// Every feature, so a caller listing them cannot miss one.
    pub const ALL: [Self; 13] = [
        Self::Playback,
        Self::Library,
        Self::ProjectEditing,
        Self::Export,
        Self::Analysis,
        Self::AiPlanning,
        Self::StemSeparation,
        Self::CloudAi,
        Self::CloudSync,
        Self::Collaboration,
        Self::Plugins,
        Self::HighResolutionExport,
        Self::LivePerformance,
    ];

    /// Whether this is something the product never withholds.
    ///
    /// The list is short and specific on purpose. Every entry is a way of
    /// reaching *work the user already owns*; nothing that creates new
    /// capability is on it.
    #[must_use]
    pub const fn is_essential(self) -> bool {
        matches!(
            self,
            Self::Playback | Self::Library | Self::ProjectEditing | Self::Export | Self::Analysis
        )
    }

    /// The lowest tier that grants this feature.
    ///
    /// `None` for essential features, which have no lowest tier because they
    /// are not tiered at all — expressing that as `Tier::Free` would have been
    /// equivalent today and would have invited someone to raise it later.
    #[must_use]
    pub const fn required_tier(self) -> Option<Tier> {
        if self.is_essential() {
            return None;
        }
        match self {
            Self::AiPlanning | Self::Plugins => Some(Tier::Standard),
            Self::StemSeparation
            | Self::CloudAi
            | Self::CloudSync
            | Self::HighResolutionExport
            | Self::LivePerformance => Some(Tier::Professional),
            Self::Collaboration => Some(Tier::Studio),
            // Essential features return above; this arm exists so that adding a
            // variant without deciding its tier is a compile error rather than
            // a silent grant.
            Self::Playback
            | Self::Library
            | Self::ProjectEditing
            | Self::Export
            | Self::Analysis => None,
        }
    }

    /// A stable identifier, for storage and localisation.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Playback => "feature.playback",
            Self::Library => "feature.library",
            Self::ProjectEditing => "feature.project_editing",
            Self::Export => "feature.export",
            Self::Analysis => "feature.analysis",
            Self::AiPlanning => "feature.ai_planning",
            Self::StemSeparation => "feature.stem_separation",
            Self::CloudAi => "feature.cloud_ai",
            Self::CloudSync => "feature.cloud_sync",
            Self::Collaboration => "feature.collaboration",
            Self::Plugins => "feature.plugins",
            Self::HighResolutionExport => "feature.high_resolution_export",
            Self::LivePerformance => "feature.live_performance",
        }
    }
}

impl fmt::Display for Feature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.key())
    }
}

/// Why a feature is not available.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Denial {
    /// The licence is at a lower tier than the feature needs.
    ///
    /// Carries the tier that would grant it, so an interface can say what to do
    /// rather than only that something is unavailable — Master Prompt #10
    /// requires errors to explain, and "upgrade" with no destination is not an
    /// explanation.
    RequiresTier {
        /// The lowest tier that grants it.
        required: Tier,
        /// The tier in force.
        current: Tier,
    },
    /// The user turned the feature off themselves.
    ///
    /// Distinct from a tier denial because it is not a reason to offer an
    /// upgrade. Master Prompt #26 lets a user disable cloud features, and
    /// answering that choice with a sales prompt would be treating their
    /// privacy decision as a mistake.
    DisabledByUser,
}

/// What a user is entitled to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Licence {
    tier: Tier,
    disabled: Vec<Feature>,
}

impl Default for Licence {
    fn default() -> Self {
        Self::free()
    }
}

impl Licence {
    /// A licence at a tier.
    #[must_use]
    pub const fn at(tier: Tier) -> Self {
        Self {
            tier,
            disabled: Vec::new(),
        }
    }

    /// A licence with no subscription.
    #[must_use]
    pub const fn free() -> Self {
        Self::at(Tier::Free)
    }

    /// What this licence becomes when a subscription lapses.
    ///
    /// The free tier, not a locked door. Every essential feature survives,
    /// because a subscription buys new capability and never buys back access to
    /// work someone already made — and Master Prompt #9 says the user owns that
    /// work, which cannot be true if it can be withheld.
    ///
    /// The user's own disabled-feature choices survive too. Someone who turned
    /// cloud analysis off has not changed their mind by failing to renew.
    #[must_use]
    pub fn expired(&self) -> Self {
        Self {
            tier: Tier::Free,
            disabled: self.disabled.clone(),
        }
    }

    /// The tier in force.
    #[must_use]
    pub const fn tier(&self) -> Tier {
        self.tier
    }

    /// Turns a feature off at the user's request.
    ///
    /// Essential features cannot be disabled — not to protect the business, but
    /// because "I have switched off the ability to open my own projects" is not
    /// a request anyone makes deliberately, and honouring it would strand them.
    ///
    /// Returns whether anything changed.
    pub fn disable(&mut self, feature: Feature) -> bool {
        if feature.is_essential() || self.disabled.contains(&feature) {
            return false;
        }
        self.disabled.push(feature);
        self.disabled.sort_unstable();
        true
    }

    /// Turns a feature back on.
    pub fn enable(&mut self, feature: Feature) -> bool {
        let before = self.disabled.len();
        self.disabled.retain(|entry| *entry != feature);
        self.disabled.len() != before
    }

    /// The features the user has turned off.
    #[must_use]
    pub fn disabled(&self) -> &[Feature] {
        &self.disabled
    }

    /// Whether a feature is available.
    #[must_use]
    pub fn allows(&self, feature: Feature) -> bool {
        self.check(feature).is_ok()
    }

    /// Whether a feature is available, and why not if it is not.
    ///
    /// # Errors
    ///
    /// Returns [`Denial`] naming what would grant the feature, or saying that
    /// the user turned it off.
    pub fn check(&self, feature: Feature) -> Result<(), Denial> {
        if self.disabled.contains(&feature) {
            return Err(Denial::DisabledByUser);
        }
        let Some(required) = feature.required_tier() else {
            // Essential. No tier, no check, no exception.
            return Ok(());
        };
        if self.tier >= required {
            Ok(())
        } else {
            Err(Denial::RequiresTier {
                required,
                current: self.tier,
            })
        }
    }

    /// Every feature this licence grants, in a stable order.
    #[must_use]
    pub fn granted(&self) -> Vec<Feature> {
        Feature::ALL
            .iter()
            .copied()
            .filter(|feature| self.allows(*feature))
            .collect()
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
    fn every_tier_can_reach_the_users_own_work() {
        // Master Prompt #29's promise, checked over the whole matrix rather
        // than asserted about the free tier alone.
        for tier in Tier::ALL {
            let licence = Licence::at(tier);
            for feature in Feature::ALL {
                if feature.is_essential() {
                    assert!(
                        licence.allows(feature),
                        "{tier} does not grant the essential feature {feature}"
                    );
                }
            }
        }
    }

    #[test]
    fn getting_work_out_is_never_charged_for() {
        // The one most likely to be argued about. A product that lets someone
        // build a mix and then charges to get it out is holding their work
        // hostage, whatever the pricing page says.
        assert!(Feature::Export.is_essential());
        assert_eq!(Feature::Export.required_tier(), None);
        assert!(Licence::free().allows(Feature::Export));
    }

    #[test]
    fn an_expired_licence_is_the_free_tier_and_not_a_locked_door() {
        // A subscription buys new capability. It never buys back access to work
        // someone already made.
        let mut paid = Licence::at(Tier::Studio);
        paid.disable(Feature::CloudAi);

        let lapsed = paid.expired();
        assert_eq!(lapsed.tier(), Tier::Free);
        for feature in Feature::ALL {
            if feature.is_essential() {
                assert!(
                    lapsed.allows(feature),
                    "an expired licence withheld {feature}"
                );
            }
        }
        assert!(!lapsed.allows(Feature::Collaboration));

        // And the user's own privacy choice survives: someone who turned cloud
        // analysis off has not changed their mind by failing to renew.
        assert_eq!(
            lapsed.check(Feature::CloudAi).err(),
            Some(Denial::DisabledByUser)
        );
    }

    #[test]
    fn a_denial_says_what_would_grant_the_feature() {
        // Master Prompt #10 requires errors to explain, and "upgrade" with no
        // destination is not an explanation.
        let licence = Licence::at(Tier::Standard);
        assert_eq!(
            licence.check(Feature::Collaboration).err(),
            Some(Denial::RequiresTier {
                required: Tier::Studio,
                current: Tier::Standard,
            })
        );
    }

    #[test]
    fn a_privacy_choice_is_not_answered_with_a_sales_prompt() {
        // Master Prompt #26 lets a user disable cloud features. Treating that
        // as a tier problem would be treating their decision as a mistake.
        let mut licence = Licence::at(Tier::Studio);
        assert!(licence.disable(Feature::CloudAi));
        assert_eq!(
            licence.check(Feature::CloudAi).err(),
            Some(Denial::DisabledByUser)
        );

        assert!(licence.enable(Feature::CloudAi));
        assert!(licence.allows(Feature::CloudAi));
        assert!(
            !licence.enable(Feature::CloudAi),
            "enabling twice should be a no-op"
        );
    }

    #[test]
    fn an_essential_feature_cannot_be_switched_off_even_deliberately() {
        // Not to protect the business: "I have switched off the ability to open
        // my own projects" is not a request anyone makes deliberately, and
        // honouring it would strand them.
        let mut licence = Licence::at(Tier::Studio);
        for feature in Feature::ALL {
            if feature.is_essential() {
                assert!(
                    !licence.disable(feature),
                    "the essential feature {feature} could be disabled"
                );
                assert!(licence.allows(feature));
            }
        }
    }

    #[test]
    fn tiers_are_cumulative() {
        // A higher tier must never grant less. Getting this wrong would be
        // invisible in any single check and obvious to the one user who
        // upgraded and lost something.
        for feature in Feature::ALL {
            let mut previously_granted = false;
            for tier in Tier::ALL {
                let granted = Licence::at(tier).allows(feature);
                assert!(
                    granted || !previously_granted,
                    "{tier} withdrew {feature}, which a lower tier granted"
                );
                previously_granted = granted;
            }
        }
    }

    #[test]
    fn the_top_tier_grants_everything() {
        let licence = Licence::at(Tier::Studio);
        assert_eq!(licence.granted().len(), Feature::ALL.len());
    }

    #[test]
    fn granted_comes_back_in_a_stable_order() {
        let licence = Licence::at(Tier::Professional);
        assert_eq!(licence.granted(), licence.granted());
        let mut sorted = licence.granted();
        sorted.sort_unstable();
        assert_eq!(
            licence.granted(),
            sorted,
            "the granted list is not in feature order"
        );
    }

    #[test]
    fn every_non_essential_feature_has_a_tier_and_every_essential_one_has_none() {
        // The pairing that makes `required_tier` trustworthy. A feature that
        // was neither would be silently free forever.
        for feature in Feature::ALL {
            if feature.is_essential() {
                assert_eq!(feature.required_tier(), None, "{feature}");
            } else {
                assert!(feature.required_tier().is_some(), "{feature} has no tier");
            }
        }
    }

    #[test]
    fn keys_are_distinct() {
        let features: Vec<&str> = Feature::ALL.iter().map(|f| f.key()).collect();
        for (index, key) in features.iter().enumerate() {
            for (other, value) in features.iter().enumerate() {
                assert!(index == other || key != value, "two features share {key}");
            }
        }

        let tiers: Vec<&str> = Tier::ALL.iter().map(|t| t.key()).collect();
        for (index, key) in tiers.iter().enumerate() {
            for (other, value) in tiers.iter().enumerate() {
                assert!(index == other || key != value, "two tiers share {key}");
            }
        }
    }
}
