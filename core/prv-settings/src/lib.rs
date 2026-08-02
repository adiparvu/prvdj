//! Preferences, experience modes and accessibility.
//!
//! # What a setting may and may not do
//!
//! Master Prompt #8 asks for an interface that suits both someone learning and
//! someone working, and that never intervenes uninvited for a professional. The
//! shape that requirement takes here is one rule applied everywhere:
//!
//! **A setting changes what the product offers. It never changes what the user
//! may do.**
//!
//! Capability belongs to `prv-security`, which answers whether someone is
//! allowed, and to `prv-entitlements`, which answers what they paid for. Neither
//! question is asked here, and this crate depends on neither. A preferences
//! screen that could remove capability would be a place where a user quietly
//! disables something they will need in six months and will not connect to a
//! choice they made today.
//!
//! The rule is mechanical: only [`Category::Assistance`] settings may vary with
//! the experience mode, and a test checks that over every setting. A future
//! setting that made a mode change behaviour fails the build.
//!
//! # Three things live here and one deliberately does not
//!
//! [`ExperienceMode`] decides how much the product volunteers. [`Settings`] holds
//! what the user chose — only what they chose, never a copy of the defaults.
//! [`Accessibility`] joins the platform's accommodations with the user's own, in
//! the one direction that is safe.
//!
//! What does not live here is *where the document is stored*. That is a file,
//! and ADR-0001 keeps files outside the core. This crate defines the document;
//! the platform layer reads and writes it.
//!
//! # It is the home the per-user state was missing
//!
//! `prv-security`'s consent record and `prv-learning`'s profile are both per
//! *user* rather than per project, so neither belongs in the operation log —
//! and until this crate existed neither had anywhere else to be. Both are
//! carried alongside these settings by the platform layer that persists them.
//! Neither is re-declared here: this crate names no purpose and infers no taste,
//! because a settings document that also held them would make deleting a profile
//! and resetting a preference the same operation, and they are not.

pub mod accessibility;
pub mod mode;
pub mod setting;
pub mod store;

pub use accessibility::{Accessibility, PlatformAccessibility};
pub use mode::ExperienceMode;
pub use setting::{Category, SettingKey, SettingValue, ValueKind};
pub use store::{Settings, SettingsError, UnkeepableReason, UnknownSetting};

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::panic,
        reason = "a test that cannot build its own fixture should fail loudly"
    )]

    use super::*;

    #[test]
    fn a_professional_gets_quiet_and_loses_nothing() {
        // The requirement in one test. Every setting outside assistance answers
        // the same at every mode, so the professional user's product is the
        // same product with less arriving on its own.
        let mut guided = Settings::new();
        guided.set_mode(ExperienceMode::Guided);
        let mut professional = Settings::new();
        professional.set_mode(ExperienceMode::Professional);

        assert!(guided.flag(SettingKey::ShowAiSuggestions));
        assert!(!professional.flag(SettingKey::ShowAiSuggestions));
        assert!(guided.flag(SettingKey::ConfirmReversibleActions));
        assert!(!professional.flag(SettingKey::ConfirmReversibleActions));

        for setting in SettingKey::ALL {
            if setting.category() == Category::Assistance {
                continue;
            }
            assert_eq!(
                guided.get(setting),
                professional.get(setting),
                "{setting} differs between modes and is not assistance"
            );
        }
    }

    #[test]
    fn an_accommodation_survives_every_other_preference() {
        // Accessibility is not one preference among several. Nothing in the
        // settings document can withdraw what the platform asked for.
        let mut settings = Settings::new();
        settings.set_mode(ExperienceMode::Professional);
        settings
            .set(SettingKey::ReduceMotion, SettingValue::Flag(false))
            .expect("a flag");

        let accessibility = settings.accessibility(PlatformAccessibility {
            reduce_motion: true,
            ..PlatformAccessibility::none()
        });
        assert!(
            accessibility.reduce_motion(),
            "a preference withdrew a platform accommodation"
        );
    }

    #[test]
    fn a_document_written_by_a_newer_build_opens_and_is_written_back_whole() {
        let mut settings = Settings::new();
        settings
            .set_choice(SettingKey::KeyNotation, "notation.open_key")
            .expect("a permitted choice");
        settings
            .keep_unknown("setting.something_new", "true")
            .expect("kept");

        assert_eq!(settings.chosen().len(), 1);
        assert_eq!(settings.unknown().len(), 1);
        assert_eq!(
            settings.choice(SettingKey::KeyNotation),
            Some("notation.open_key")
        );
    }
}
