//! The settings document.
//!
//! # Only choices are stored
//!
//! A setting the user has not touched is *absent*, not stored at its default.
//! The difference is invisible until the first time a default changes — when the
//! product decides that suggestions should be quieter in professional mode, or
//! that a new delivery target should be the recommended one — and then it is the
//! whole thing. A document full of defaults freezes every user on the values
//! that were current the day they first ran the application, and nobody can tell
//! which of those values they actually chose.
//!
//! So [`Settings::set`] records a choice and [`Settings::clear`] withdraws one,
//! and a withdrawn setting goes back to following the mode rather than to the
//! value it happened to have.
//!
//! # A newer build's settings survive an older build
//!
//! Someone with two machines will open this document with two versions of the
//! product. If the older one dropped what it did not recognise, every launch on
//! the older machine would silently delete the newer machine's preferences. So
//! unrecognised entries are kept verbatim and written back unchanged — the same
//! discipline ADR-0003 applies to the operation log, for the same reason.

use std::collections::BTreeMap;

use core::fmt;

use crate::accessibility::{Accessibility, PlatformAccessibility};
use crate::mode::ExperienceMode;
use crate::setting::{SettingKey, SettingValue, ValueKind};

/// Why a setting could not be stored.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SettingsError {
    /// The value was not the kind the setting holds.
    WrongKind {
        /// Which setting.
        setting: SettingKey,
        /// What it holds.
        expected: ValueKind,
        /// What was offered.
        found: ValueKind,
    },
    /// The name was not one of the setting's permitted choices.
    NotPermitted {
        /// Which setting.
        setting: SettingKey,
    },
    /// The number was outside the setting's range.
    OutOfRange {
        /// Which setting.
        setting: SettingKey,
        /// The lowest permitted value.
        low: i64,
        /// The highest permitted value.
        high: i64,
    },
    /// An unrecognised entry could not be kept.
    Unkeepable {
        /// Why.
        reason: UnkeepableReason,
    },
}

/// Why an unrecognised entry was not kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum UnkeepableReason {
    /// Its name was empty or too long.
    NameNotUsable,
    /// Its value was too long.
    ValueTooLong,
    /// The document already holds as many as it will.
    TooMany,
}

impl fmt::Display for SettingsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::WrongKind {
                setting,
                expected,
                found,
            } => write!(f, "{setting} holds {expected}, not {found}"),
            Self::NotPermitted { setting } => write!(f, "{setting} does not permit that value"),
            Self::OutOfRange { setting, low, high } => {
                write!(f, "{setting} must be between {low} and {high}")
            }
            Self::Unkeepable { reason } => {
                write!(f, "an unrecognised setting was not kept ({reason:?})")
            }
        }
    }
}

impl core::error::Error for SettingsError {}

/// A setting this build has never heard of, kept as it was found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownSetting {
    name: String,
    value: String,
}

impl UnknownSetting {
    /// What it was called.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// What it held, verbatim.
    #[must_use]
    pub fn value(&self) -> &str {
        &self.value
    }
}

/// Everything a user has chosen.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Settings {
    mode: ExperienceMode,
    chosen: BTreeMap<SettingKey, SettingValue>,
    unknown: Vec<UnknownSetting>,
}

impl Settings {
    /// The longest name an unrecognised setting may have.
    pub const MAX_UNKNOWN_NAME: usize = 128;
    /// The longest value an unrecognised setting may have.
    pub const MAX_UNKNOWN_VALUE: usize = 1024;
    /// How many unrecognised settings are kept.
    ///
    /// Generous, because the cost of dropping one is a user's preference
    /// vanishing, and bounded, because a document that grows without limit is
    /// one a malformed writer can use to fill a disk.
    pub const MAX_UNKNOWN: usize = 256;

    /// A document with nothing chosen.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The experience mode in force.
    #[must_use]
    pub const fn mode(&self) -> ExperienceMode {
        self.mode
    }

    /// Changes the experience mode.
    ///
    /// Chosen settings are untouched. Switching to professional makes the
    /// product quieter about everything the user has not had an opinion on, and
    /// changes nothing they have — silently reversing an explicit choice because
    /// a mode changed would be the product overruling them.
    pub fn set_mode(&mut self, mode: ExperienceMode) {
        self.mode = mode;
    }

    /// What a setting is, chosen or defaulted.
    ///
    /// Never fails. A setting always has a value, and a caller reading one
    /// should not have to handle the possibility that it does not.
    #[must_use]
    pub fn get(&self, setting: SettingKey) -> SettingValue {
        self.chosen
            .get(&setting)
            .copied()
            .unwrap_or_else(|| setting.default_for(self.mode))
    }

    /// A flag setting's value, or its default if it is not a flag.
    ///
    /// The convenience a caller wants at a call site; the kind is fixed at
    /// compile time by the setting it names, so a mismatch here is a programming
    /// error rather than a condition to handle.
    #[must_use]
    pub fn flag(&self, setting: SettingKey) -> bool {
        self.get(setting).as_flag().unwrap_or(false)
    }

    /// A choice setting's value.
    #[must_use]
    pub fn choice(&self, setting: SettingKey) -> Option<&'static str> {
        self.get(setting).as_choice()
    }

    /// A count setting's value.
    #[must_use]
    pub fn count(&self, setting: SettingKey) -> Option<i64> {
        self.get(setting).as_count()
    }

    /// Whether the user has had an opinion about this.
    #[must_use]
    pub fn is_chosen(&self, setting: SettingKey) -> bool {
        self.chosen.contains_key(&setting)
    }

    /// Records a choice.
    ///
    /// # Errors
    ///
    /// Returns [`SettingsError`] if the value is the wrong kind for the setting,
    /// outside its range, or not one of its permitted choices.
    pub fn set(&mut self, setting: SettingKey, value: SettingValue) -> Result<(), SettingsError> {
        if value.kind() != setting.kind() {
            return Err(SettingsError::WrongKind {
                setting,
                expected: setting.kind(),
                found: value.kind(),
            });
        }
        if let Some(count) = value.as_count() {
            if let Some((low, high)) = setting.range() {
                if !(low..=high).contains(&count) {
                    return Err(SettingsError::OutOfRange { setting, low, high });
                }
            }
        }
        if let Some(choice) = value.as_choice() {
            if !setting.choices().contains(&choice) {
                return Err(SettingsError::NotPermitted { setting });
            }
        }
        self.chosen.insert(setting, value);
        Ok(())
    }

    /// Records a choice named as text, resolving it against the permitted list.
    ///
    /// The route a value takes when it comes from storage or from an interface,
    /// which is where an unrecognised name has to be caught.
    ///
    /// # Errors
    ///
    /// Returns [`SettingsError`] if the setting is not a choice setting or the
    /// name is not one it permits.
    pub fn set_choice(&mut self, setting: SettingKey, name: &str) -> Result<(), SettingsError> {
        if setting.kind() != ValueKind::Choice {
            return Err(SettingsError::WrongKind {
                setting,
                expected: setting.kind(),
                found: ValueKind::Choice,
            });
        }
        let permitted = setting
            .choices()
            .iter()
            .find(|candidate| **candidate == name)
            .ok_or(SettingsError::NotPermitted { setting })?;
        self.chosen.insert(setting, SettingValue::Choice(permitted));
        Ok(())
    }

    /// Withdraws a choice, so the setting follows the mode again.
    ///
    /// Returns whether anything changed.
    pub fn clear(&mut self, setting: SettingKey) -> bool {
        self.chosen.remove(&setting).is_some()
    }

    /// Withdraws every choice, keeping the mode and anything unrecognised.
    ///
    /// Unrecognised entries survive because they are another build's settings,
    /// not this user's history — discarding them here would make "reset my
    /// preferences" on one machine delete preferences on another.
    pub fn clear_all(&mut self) {
        self.chosen.clear();
    }

    /// Every setting the user has had an opinion about, in setting order.
    #[must_use]
    pub fn chosen(&self) -> Vec<(SettingKey, SettingValue)> {
        self.chosen
            .iter()
            .map(|(setting, value)| (*setting, *value))
            .collect()
    }

    /// Keeps a setting this build does not recognise.
    ///
    /// # Errors
    ///
    /// Returns [`SettingsError::Unkeepable`] if the entry is unusable or the
    /// document already holds [`Self::MAX_UNKNOWN`] of them. Refusing is
    /// reported rather than silent, so a caller can say that a document was not
    /// preserved whole.
    pub fn keep_unknown(&mut self, name: &str, value: &str) -> Result<(), SettingsError> {
        if name.is_empty() || name.len() > Self::MAX_UNKNOWN_NAME {
            return Err(SettingsError::Unkeepable {
                reason: UnkeepableReason::NameNotUsable,
            });
        }
        if value.len() > Self::MAX_UNKNOWN_VALUE {
            return Err(SettingsError::Unkeepable {
                reason: UnkeepableReason::ValueTooLong,
            });
        }
        if let Some(existing) = self.unknown.iter_mut().find(|entry| entry.name == name) {
            value.clone_into(&mut existing.value);
            return Ok(());
        }
        if self.unknown.len() >= Self::MAX_UNKNOWN {
            return Err(SettingsError::Unkeepable {
                reason: UnkeepableReason::TooMany,
            });
        }
        self.unknown.push(UnknownSetting {
            name: name.to_owned(),
            value: value.to_owned(),
        });
        self.unknown
            .sort_by(|left, right| left.name.cmp(&right.name));
        Ok(())
    }

    /// The settings this build does not recognise, to be written back unchanged.
    #[must_use]
    pub fn unknown(&self) -> &[UnknownSetting] {
        &self.unknown
    }

    /// The accommodations in force, given what the platform reports.
    ///
    /// The join between the stored preferences and the live platform state. The
    /// platform's answer can only add: see [`Accessibility`].
    #[must_use]
    pub fn accessibility(&self, platform: PlatformAccessibility) -> Accessibility {
        let mut accessibility = Accessibility::from_platform(platform);
        accessibility.request_reduced_motion(self.flag(SettingKey::ReduceMotion));
        accessibility.request_increased_contrast(self.flag(SettingKey::IncreaseContrast));
        accessibility.request_reduced_transparency(self.flag(SettingKey::ReduceTransparency));
        accessibility
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
    use crate::setting::Category;

    #[test]
    fn an_untouched_setting_follows_the_mode() {
        let mut settings = Settings::new();
        assert!(settings.flag(SettingKey::ShowAiSuggestions));

        settings.set_mode(ExperienceMode::Professional);
        assert!(
            !settings.flag(SettingKey::ShowAiSuggestions),
            "professional mode should stop volunteering"
        );
        assert!(!settings.is_chosen(SettingKey::ShowAiSuggestions));
    }

    #[test]
    fn changing_mode_never_discards_a_choice_the_user_made() {
        // Silently reversing an explicit choice because a mode changed would be
        // the product overruling the user.
        let mut settings = Settings::new();
        settings
            .set(SettingKey::ShowAiSuggestions, SettingValue::Flag(true))
            .expect("a flag on a flag setting");

        settings.set_mode(ExperienceMode::Professional);
        assert!(
            settings.flag(SettingKey::ShowAiSuggestions),
            "an explicit choice was overruled by a mode change"
        );
        assert!(settings.is_chosen(SettingKey::ShowAiSuggestions));
    }

    #[test]
    fn withdrawing_a_choice_returns_to_following_the_mode() {
        // Not to the value it happened to have — that is the difference between
        // "reset" and "stop having an opinion".
        let mut settings = Settings::new();
        settings.set_mode(ExperienceMode::Professional);
        settings
            .set(SettingKey::ShowAiSuggestions, SettingValue::Flag(true))
            .expect("a flag");
        assert!(settings.flag(SettingKey::ShowAiSuggestions));

        assert!(settings.clear(SettingKey::ShowAiSuggestions));
        assert!(!settings.flag(SettingKey::ShowAiSuggestions));
        assert!(!settings.clear(SettingKey::ShowAiSuggestions));
    }

    #[test]
    fn only_choices_are_stored() {
        // A document full of defaults freezes every user on the values current
        // the day they first ran the product, and nobody can tell which of them
        // they actually chose.
        let settings = Settings::new();
        assert!(settings.chosen().is_empty());
        for setting in SettingKey::ALL {
            assert!(!settings.is_chosen(setting), "{setting} was stored unasked");
        }
    }

    #[test]
    fn a_value_of_the_wrong_kind_is_refused_and_says_what_was_expected() {
        let mut settings = Settings::new();
        assert_eq!(
            settings.set(SettingKey::SnapToGrid, SettingValue::Count(1)),
            Err(SettingsError::WrongKind {
                setting: SettingKey::SnapToGrid,
                expected: ValueKind::Flag,
                found: ValueKind::Count,
            })
        );
        assert!(!settings.is_chosen(SettingKey::SnapToGrid));
    }

    #[test]
    fn a_choice_the_setting_does_not_permit_is_refused() {
        let mut settings = Settings::new();
        assert_eq!(
            settings.set_choice(SettingKey::KeyNotation, "notation.invented"),
            Err(SettingsError::NotPermitted {
                setting: SettingKey::KeyNotation
            })
        );
        assert_eq!(
            settings.set_choice(SettingKey::SnapToGrid, "snap.bar"),
            Err(SettingsError::WrongKind {
                setting: SettingKey::SnapToGrid,
                expected: ValueKind::Flag,
                found: ValueKind::Choice,
            })
        );

        settings
            .set_choice(SettingKey::KeyNotation, "notation.standard")
            .expect("a permitted choice");
        assert_eq!(
            settings.choice(SettingKey::KeyNotation),
            Some("notation.standard")
        );
    }

    #[test]
    fn a_count_outside_its_range_is_refused_at_both_ends() {
        let mut settings = Settings::new();
        let (low, high) = SettingKey::RecentProjectsShown
            .range()
            .expect("a count setting has a range");

        for offered in [low - 1, high + 1] {
            assert_eq!(
                settings.set(
                    SettingKey::RecentProjectsShown,
                    SettingValue::Count(offered)
                ),
                Err(SettingsError::OutOfRange {
                    setting: SettingKey::RecentProjectsShown,
                    low,
                    high,
                })
            );
        }

        settings
            .set(SettingKey::RecentProjectsShown, SettingValue::Count(high))
            .expect("the top of the range is permitted");
        assert_eq!(settings.count(SettingKey::RecentProjectsShown), Some(high));
    }

    #[test]
    fn a_newer_builds_settings_survive_this_one() {
        // Someone with two machines opens this document with two versions. If
        // the older one dropped what it did not recognise, every launch on the
        // older machine would delete the newer machine's preferences.
        let mut settings = Settings::new();
        settings
            .keep_unknown("setting.from_the_future", "whatever it said")
            .expect("an unrecognised setting is kept");
        settings
            .keep_unknown("setting.also_new", "42")
            .expect("kept");

        assert_eq!(settings.unknown().len(), 2);
        assert_eq!(
            settings.unknown().first().map(UnknownSetting::name),
            Some("setting.also_new")
        );
        assert_eq!(
            settings.unknown().last().map(UnknownSetting::value),
            Some("whatever it said")
        );

        // And resetting the user's own preferences does not touch them.
        settings.clear_all();
        assert_eq!(settings.unknown().len(), 2);
    }

    #[test]
    fn an_unrecognised_setting_seen_twice_keeps_the_later_value() {
        let mut settings = Settings::new();
        settings.keep_unknown("setting.new", "first").expect("kept");
        settings
            .keep_unknown("setting.new", "second")
            .expect("kept");
        assert_eq!(settings.unknown().len(), 1);
        assert_eq!(
            settings.unknown().first().map(UnknownSetting::value),
            Some("second")
        );
    }

    #[test]
    fn an_unkeepable_entry_is_reported_rather_than_dropped_quietly() {
        // A caller has to be able to say that a document was not preserved
        // whole, or "we keep what we do not understand" is only true when it is
        // convenient.
        let mut settings = Settings::new();
        assert_eq!(
            settings.keep_unknown("", "value"),
            Err(SettingsError::Unkeepable {
                reason: UnkeepableReason::NameNotUsable
            })
        );
        assert_eq!(
            settings.keep_unknown(&"n".repeat(Settings::MAX_UNKNOWN_NAME + 1), "value"),
            Err(SettingsError::Unkeepable {
                reason: UnkeepableReason::NameNotUsable
            })
        );
        assert_eq!(
            settings.keep_unknown("setting.big", &"v".repeat(Settings::MAX_UNKNOWN_VALUE + 1)),
            Err(SettingsError::Unkeepable {
                reason: UnkeepableReason::ValueTooLong
            })
        );

        for index in 0..Settings::MAX_UNKNOWN {
            settings
                .keep_unknown(&format!("setting.n{index}"), "v")
                .expect("within the limit");
        }
        assert_eq!(
            settings.keep_unknown("setting.one_too_many", "v"),
            Err(SettingsError::Unkeepable {
                reason: UnkeepableReason::TooMany
            })
        );
    }

    #[test]
    fn accessibility_preferences_join_with_what_the_platform_reports() {
        let mut settings = Settings::new();
        settings
            .set(SettingKey::ReduceMotion, SettingValue::Flag(true))
            .expect("a flag");

        let quiet_platform = settings.accessibility(PlatformAccessibility::none());
        assert!(quiet_platform.reduce_motion());
        assert!(!quiet_platform.increase_contrast());

        settings.clear(SettingKey::ReduceMotion);
        let insistent_platform = settings.accessibility(PlatformAccessibility {
            reduce_motion: true,
            ..PlatformAccessibility::none()
        });
        assert!(
            insistent_platform.reduce_motion(),
            "the platform's answer must survive the absence of a preference"
        );
    }

    #[test]
    fn no_mode_changes_anything_but_assistance() {
        // The rule this crate exists to hold, checked through the store rather
        // than only through the key: switching mode must move no setting
        // outside Category::Assistance.
        let mut settings = Settings::new();
        let baseline: Vec<SettingValue> = SettingKey::ALL.map(|s| settings.get(s)).to_vec();

        for mode in ExperienceMode::ALL {
            settings.set_mode(mode);
            for (index, setting) in SettingKey::ALL.into_iter().enumerate() {
                if setting.category() == Category::Assistance {
                    continue;
                }
                assert_eq!(
                    Some(settings.get(setting)),
                    baseline.get(index).copied(),
                    "{mode} changed {setting}, which is not assistance"
                );
            }
        }
    }
}
