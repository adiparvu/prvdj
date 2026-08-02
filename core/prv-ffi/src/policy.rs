//! Consent and entitlement, as a host sees it.
//!
//! # Why these two share a module
//!
//! They are the two questions asked before anything happens, and they are asked
//! in a fixed order that a host must not get wrong: *may this leave the device*
//! comes before *is this feature available*. A user who has not agreed to cloud
//! analysis is not offered a paywall for it — they are simply not sent
//! anywhere, whatever tier they are on.
//!
//! Putting them together is what makes that order visible. Two modules would
//! have let a host consult the licence first, which is a paywall in front of a
//! privacy decision, and the wrong shape.
//!
//! # Nothing here decides anything
//!
//! Every answer comes from `prv-security` and `prv-entitlements`. The boundary
//! adds no rule, no default and no shortcut — which matters more here than
//! anywhere else in the crate, because these are the two places where a
//! convenient shortcut looks like a feature and reads, later, as a breach.
//!
//! # Consent starts withheld and stays that way until asked for
//!
//! [`Policy::new`] grants nothing. Master Prompt #26 requires it, and a boundary
//! that defaulted a purpose to allowed would make every host that forgot to ask
//! into a host that sends audio to a server.

use prv_entitlements::{Feature, Licence, Tier};
use prv_security::{Consents, Purpose};

use crate::status::Status;

/// What the user has agreed to, and what their licence allows.
#[derive(Debug)]
pub struct Policy {
    consents: Consents,
    licence: Licence,
}

impl Default for Policy {
    fn default() -> Self {
        Self::new()
    }
}

impl Policy {
    /// Nothing agreed to, and the free tier.
    ///
    /// Both halves are the safe end of their range. A host that never configures
    /// this can still run the whole product offline, which is the behaviour
    /// Master Prompt #29 requires of the free tier and Master Prompt #26
    /// requires of an unasked user.
    #[must_use]
    pub fn new() -> Self {
        Self {
            consents: Consents::none(),
            licence: Licence::free(),
        }
    }

    /// Records that the user agreed to a purpose.
    ///
    /// `ordinal` is a monotonically increasing number the host supplies — a
    /// version of the wording they agreed to, or a sequence. The core keeps it
    /// so an audit can say *which* agreement was given, which is the difference
    /// between a consent record and a boolean.
    ///
    /// # Errors
    ///
    /// [`Status::InvalidArgument`] when the code names no purpose this version
    /// defines.
    pub fn grant(&mut self, purpose_code: i32, ordinal: u64) -> Result<(), Status> {
        let purpose = purpose_from_code(purpose_code).ok_or(Status::InvalidArgument)?;
        self.consents.grant(purpose, ordinal);
        Ok(())
    }

    /// Records that the user withdrew a purpose.
    ///
    /// # Errors
    ///
    /// [`Status::InvalidArgument`] when the code names no purpose.
    pub fn withdraw(&mut self, purpose_code: i32) -> Result<(), Status> {
        let purpose = purpose_from_code(purpose_code).ok_or(Status::InvalidArgument)?;
        self.consents.withdraw(purpose);
        Ok(())
    }

    /// Withdraws everything at once.
    ///
    /// The control a privacy screen needs to offer in one tap. Implemented as
    /// one call rather than a loop in the host, because a loop in the host is a
    /// loop that can be interrupted half way.
    pub fn withdraw_all(&mut self) {
        self.consents.withdraw_all();
    }

    /// Whether a purpose is currently agreed to.
    ///
    /// # Errors
    ///
    /// [`Status::InvalidArgument`] when the code names no purpose.
    pub fn allows(&self, purpose_code: i32) -> Result<bool, Status> {
        let purpose = purpose_from_code(purpose_code).ok_or(Status::InvalidArgument)?;
        Ok(self.consents.allows(purpose))
    }

    /// Whether this purpose sends the user's own *material*, rather than a fact
    /// about it.
    ///
    /// # Two different questions, and a consent screen needs both
    ///
    /// "We send the tempo we measured" and "we send the recording" are both
    /// cloud processing, and a user told only the first is being misled. This
    /// answers the second; [`Self::anything_leaves_the_device`] answers whether
    /// *anything* is transmitted at all — a crash report leaves the device and
    /// carries none of the user's music, so the two genuinely differ.
    ///
    /// A host that exposed only one of them would build an honest-looking
    /// screen that is wrong in one direction or the other.
    ///
    /// # Errors
    ///
    /// [`Status::InvalidArgument`] when the code names no purpose.
    pub fn purpose_sends_content(purpose_code: i32) -> Result<bool, Status> {
        let purpose = purpose_from_code(purpose_code).ok_or(Status::InvalidArgument)?;
        Ok(purpose.sends_content())
    }

    /// Whether *anything at all* currently leaves the device.
    ///
    /// The single question a privacy screen leads with, and the one a user
    /// actually wants answered. Composed in the core from every purpose that
    /// sends content, so a purpose added later is included without any host
    /// being changed.
    #[must_use]
    pub fn anything_leaves_the_device(&self) -> bool {
        self.consents.anything_leaves_the_device()
    }

    /// Sets the licence tier.
    ///
    /// # Errors
    ///
    /// [`Status::InvalidArgument`] when the code names no tier.
    pub fn set_tier(&mut self, tier_code: i32) -> Result<(), Status> {
        let tier = tier_from_code(tier_code).ok_or(Status::InvalidArgument)?;
        self.licence = Licence::at(tier);
        Ok(())
    }

    /// Marks the licence expired, dropping it to what a free user has.
    ///
    /// Expiry is not a lock-out. Master Prompt #29 forbids restricting essential
    /// functionality, so an expired licence keeps everything essential and loses
    /// only what was paid for.
    pub fn expire(&mut self) {
        self.licence = self.licence.expired();
    }

    /// The current tier's code.
    #[must_use]
    pub fn tier(&self) -> i32 {
        tier_code(self.licence.tier())
    }

    /// Whether a feature is available under the current licence.
    ///
    /// # Errors
    ///
    /// [`Status::InvalidArgument`] when the code names no feature.
    pub fn feature_allowed(&self, feature_code: i32) -> Result<bool, Status> {
        let feature = feature_from_code(feature_code).ok_or(Status::InvalidArgument)?;
        Ok(self.licence.allows(feature))
    }

    /// Whether a feature is essential, and so available at every tier.
    ///
    /// A host shows this to decide whether an unavailable feature deserves an
    /// upgrade prompt or an explanation. An essential feature that is somehow
    /// unavailable is a defect, not an upsell.
    ///
    /// # Errors
    ///
    /// [`Status::InvalidArgument`] when the code names no feature.
    pub fn feature_is_essential(&self, feature_code: i32) -> Result<bool, Status> {
        let feature = feature_from_code(feature_code).ok_or(Status::InvalidArgument)?;
        Ok(feature.is_essential())
    }
}

/// The purpose a code names.
#[must_use]
pub fn purpose_from_code(code: i32) -> Option<Purpose> {
    usize::try_from(code)
        .ok()
        .and_then(|index| Purpose::ALL.get(index).copied())
}

/// The code a purpose has.
#[must_use]
pub fn purpose_code(purpose: Purpose) -> i32 {
    i32::try_from(
        Purpose::ALL
            .iter()
            .position(|candidate| *candidate == purpose)
            .unwrap_or(0),
    )
    .unwrap_or(0)
}

/// The tier a code names.
#[must_use]
pub fn tier_from_code(code: i32) -> Option<Tier> {
    usize::try_from(code)
        .ok()
        .and_then(|index| Tier::ALL.get(index).copied())
}

/// The code a tier has.
#[must_use]
pub fn tier_code(tier: Tier) -> i32 {
    i32::try_from(
        Tier::ALL
            .iter()
            .position(|candidate| *candidate == tier)
            .unwrap_or(0),
    )
    .unwrap_or(0)
}

/// The feature a code names.
#[must_use]
pub fn feature_from_code(code: i32) -> Option<Feature> {
    usize::try_from(code)
        .ok()
        .and_then(|index| Feature::ALL.get(index).copied())
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
    fn a_new_policy_sends_nothing_anywhere() {
        // Master Prompt #26. A boundary that defaulted a purpose to allowed
        // would turn every host that forgot to ask into a host that uploads
        // somebody's music.
        let policy = Policy::new();
        assert!(!policy.anything_leaves_the_device());
        for (index, _) in Purpose::ALL.iter().enumerate() {
            let code = i32::try_from(index).expect("small");
            assert_eq!(
                policy.allows(code),
                Ok(false),
                "purpose {code} was on by default"
            );
        }
    }

    #[test]
    fn granting_and_withdrawing_a_purpose_round_trips() {
        let mut policy = Policy::new();
        let sync = purpose_code(Purpose::ProjectSync);

        policy.grant(sync, 1).expect("a real purpose");
        assert_eq!(policy.allows(sync), Ok(true));
        assert!(policy.anything_leaves_the_device());

        policy.withdraw(sync).expect("a real purpose");
        assert_eq!(policy.allows(sync), Ok(false));
        assert!(!policy.anything_leaves_the_device());
    }

    #[test]
    fn withdrawing_everything_is_one_call_and_leaves_nothing_on() {
        // A privacy screen offers this in one tap, and a loop in the host is a
        // loop that can be interrupted half way.
        let mut policy = Policy::new();
        for index in 0..Purpose::ALL.len() {
            policy
                .grant(i32::try_from(index).expect("small"), 1)
                .expect("a real purpose");
        }
        assert!(policy.anything_leaves_the_device());

        policy.withdraw_all();
        assert!(!policy.anything_leaves_the_device());
        for index in 0..Purpose::ALL.len() {
            let code = i32::try_from(index).expect("small");
            assert_eq!(policy.allows(code), Ok(false));
        }
    }

    #[test]
    fn sending_content_implies_leaving_the_device_but_not_the_reverse() {
        // The two questions a consent screen has to keep apart, and the reason
        // both are exposed.
        //
        // This test was first written asserting they were the same thing, and
        // the core disagreed: `CrashDiagnostics` sends no music and still leaves
        // the device. The core was right. A screen built on either predicate
        // alone is misleading — one way it hides an upload, the other way it
        // claims the user's recordings are being sent when they are not.
        for purpose in Purpose::ALL {
            let mut policy = Policy::new();
            policy
                .grant(purpose_code(purpose), 1)
                .expect("a real purpose");

            if purpose.sends_content() {
                assert!(
                    policy.anything_leaves_the_device(),
                    "{purpose:?} sends the user's material without leaving the device"
                );
            }
        }

        // And at least one purpose transmits something while sending no
        // content, or the distinction would be theoretical.
        let quiet = Purpose::ALL
            .iter()
            .find(|purpose| !purpose.sends_content())
            .copied()
            .expect("at least one purpose sends no content");
        assert_eq!(
            Policy::purpose_sends_content(purpose_code(quiet)),
            Ok(false)
        );
    }

    #[test]
    fn a_host_can_tell_a_fact_about_the_music_from_the_music() {
        // What a consent screen shows beside each toggle.
        for purpose in Purpose::ALL {
            assert_eq!(
                Policy::purpose_sends_content(purpose_code(purpose)),
                Ok(purpose.sends_content())
            );
        }
        assert_eq!(
            Policy::purpose_sends_content(9_999),
            Err(Status::InvalidArgument)
        );
    }

    #[test]
    fn a_code_this_version_does_not_define_is_refused_rather_than_guessed() {
        // Guessing would grant *some* purpose, and the one next to model
        // training is personalised suggestions.
        let mut policy = Policy::new();
        assert_eq!(policy.grant(-1, 1), Err(Status::InvalidArgument));
        assert_eq!(policy.grant(9_999, 1), Err(Status::InvalidArgument));
        assert_eq!(policy.withdraw(9_999), Err(Status::InvalidArgument));
        assert_eq!(policy.allows(9_999), Err(Status::InvalidArgument));
        assert_eq!(policy.set_tier(9_999), Err(Status::InvalidArgument));
        assert_eq!(policy.feature_allowed(9_999), Err(Status::InvalidArgument));
    }

    #[test]
    fn every_purpose_tier_and_feature_round_trips_through_its_code() {
        for purpose in Purpose::ALL {
            assert_eq!(purpose_from_code(purpose_code(purpose)), Some(purpose));
        }
        for tier in Tier::ALL {
            assert_eq!(tier_from_code(tier_code(tier)), Some(tier));
        }
        for (index, feature) in Feature::ALL.iter().enumerate() {
            let code = i32::try_from(index).expect("small");
            assert_eq!(feature_from_code(code), Some(*feature));
        }
    }

    #[test]
    fn every_essential_feature_is_available_on_the_free_tier() {
        // Master Prompt #29: essential functionality is never artificially
        // restricted. Asserted here as well as in `prv-entitlements` because
        // this is the surface a host actually asks, and a boundary that lost
        // the guarantee in translation would be indistinguishable from one that
        // never had it.
        let policy = Policy::new();
        for (index, feature) in Feature::ALL.iter().enumerate() {
            let code = i32::try_from(index).expect("small");
            if feature.is_essential() {
                assert_eq!(
                    policy.feature_allowed(code),
                    Ok(true),
                    "{feature:?} is essential and unavailable on the free tier"
                );
            }
        }
    }

    #[test]
    fn an_expired_licence_keeps_everything_essential() {
        // Expiry is not a lock-out. A user whose card failed still gets their
        // work back.
        let mut policy = Policy::new();
        policy
            .set_tier(tier_code(Tier::Studio))
            .expect("a real tier");
        policy.expire();

        for (index, feature) in Feature::ALL.iter().enumerate() {
            if feature.is_essential() {
                let code = i32::try_from(index).expect("small");
                assert_eq!(
                    policy.feature_allowed(code),
                    Ok(true),
                    "{feature:?} was lost when the licence expired"
                );
            }
        }
    }

    #[test]
    fn a_higher_tier_never_takes_a_feature_away() {
        // The property a tier ladder must have and the one nothing else checks
        // from this side.
        for (lower, higher) in Tier::ALL.iter().zip(Tier::ALL.iter().skip(1)) {
            let mut below = Policy::new();
            below.set_tier(tier_code(*lower)).expect("a real tier");
            let mut above = Policy::new();
            above.set_tier(tier_code(*higher)).expect("a real tier");

            for index in 0..Feature::ALL.len() {
                let code = i32::try_from(index).expect("small");
                if below.feature_allowed(code) == Ok(true) {
                    assert_eq!(
                        above.feature_allowed(code),
                        Ok(true),
                        "{higher:?} lost a feature {lower:?} had"
                    );
                }
            }
        }
    }

    #[test]
    fn consent_and_licence_are_independent() {
        // A paid user who agreed to nothing sends nothing; a free user who
        // agreed to everything is still on the free tier. Coupling them would
        // make a purchase into a consent, which is the shape this module exists
        // to prevent.
        let mut policy = Policy::new();
        policy
            .set_tier(tier_code(Tier::Studio))
            .expect("a real tier");
        assert!(
            !policy.anything_leaves_the_device(),
            "a purchase granted consent"
        );

        let mut other = Policy::new();
        for index in 0..Purpose::ALL.len() {
            other
                .grant(i32::try_from(index).expect("small"), 1)
                .expect("a real purpose");
        }
        assert_eq!(
            other.tier(),
            tier_code(Tier::Free),
            "consent changed the tier"
        );
    }
}
