//! The settings themselves.
//!
//! # A closed vocabulary
//!
//! Settings are an enumeration rather than free-form strings, so that adding one
//! is a change a reviewer sees, with its default, its category and its permitted
//! values in the same diff. A string-keyed bag of preferences accumulates
//! entries nobody can account for, and the entries nobody can account for are
//! the ones that turn out to gate something important.
//!
//! # A mode may only change assistance
//!
//! Every setting belongs to a [`Category`], and only [`Category::Assistance`]
//! settings may have a default that varies with the experience mode. That is the
//! mechanical form of the rule in [`crate::mode`]: a mode changes how much the
//! product volunteers and never what the user may do. A test checks it over
//! every setting, so a future setting that tried to make a mode change behaviour
//! would fail the build rather than a review.

use core::fmt;

use crate::mode::ExperienceMode;

/// What kind of thing a setting holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ValueKind {
    /// On or off.
    Flag,
    /// A whole number within a stated range.
    Count,
    /// One of a fixed list of names.
    Choice,
}

impl ValueKind {
    /// A stable identifier, for storage and localisation.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Flag => "kind.flag",
            Self::Count => "kind.count",
            Self::Choice => "kind.choice",
        }
    }
}

impl fmt::Display for ValueKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.key())
    }
}

/// A setting's value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SettingValue {
    /// On or off.
    Flag(bool),
    /// A whole number.
    Count(i64),
    /// A name from the setting's permitted list.
    ///
    /// Static rather than owned because the permitted list is fixed at compile
    /// time; a value read from storage is resolved against that list, so an
    /// unrecognised name is refused at the boundary rather than stored and
    /// discovered later.
    Choice(&'static str),
}

impl SettingValue {
    /// Which kind this is.
    #[must_use]
    pub const fn kind(self) -> ValueKind {
        match self {
            Self::Flag(_) => ValueKind::Flag,
            Self::Count(_) => ValueKind::Count,
            Self::Choice(_) => ValueKind::Choice,
        }
    }

    /// The flag, if it is one.
    #[must_use]
    pub const fn as_flag(self) -> Option<bool> {
        match self {
            Self::Flag(value) => Some(value),
            Self::Count(_) | Self::Choice(_) => None,
        }
    }

    /// The number, if it is one.
    #[must_use]
    pub const fn as_count(self) -> Option<i64> {
        match self {
            Self::Count(value) => Some(value),
            Self::Flag(_) | Self::Choice(_) => None,
        }
    }

    /// The choice, if it is one.
    #[must_use]
    pub const fn as_choice(self) -> Option<&'static str> {
        match self {
            Self::Choice(value) => Some(value),
            Self::Flag(_) | Self::Count(_) => None,
        }
    }
}

impl fmt::Display for SettingValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::Flag(value) => f.write_str(if value { "true" } else { "false" }),
            Self::Count(value) => write!(f, "{value}"),
            Self::Choice(value) => f.write_str(value),
        }
    }
}

/// What a setting is about.
///
/// The category is what decides whether the experience mode is allowed to have
/// an opinion about it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Category {
    /// An accommodation. Never overridden by anything else, ever.
    Accessibility,
    /// How much the product volunteers. The only category a mode may change.
    Assistance,
    /// What the product does when the user acts.
    Behaviour,
    /// How something is shown or named.
    Presentation,
}

impl Category {
    /// Every category.
    pub const ALL: [Self; 4] = [
        Self::Accessibility,
        Self::Assistance,
        Self::Behaviour,
        Self::Presentation,
    ];

    /// Whether the experience mode may supply this category's default.
    #[must_use]
    pub const fn follows_the_mode(self) -> bool {
        matches!(self, Self::Assistance)
    }

    /// A stable identifier, for storage and localisation.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Accessibility => "category.accessibility",
            Self::Assistance => "category.assistance",
            Self::Behaviour => "category.behaviour",
            Self::Presentation => "category.presentation",
        }
    }
}

/// The permitted values of every choice setting, named once.
mod choices {
    /// Where editing snaps to.
    pub(super) const SNAP: [&str; 4] = ["snap.division", "snap.beat", "snap.bar", "snap.phrase"];
    /// How a key is written.
    pub(super) const KEY_NOTATION: [&str; 3] =
        ["notation.camelot", "notation.standard", "notation.open_key"];
    /// Where an export goes by default.
    pub(super) const DELIVERY: [&str; 4] = [
        "target.streaming",
        "target.club",
        "target.broadcast",
        "target.archive",
    ];
}

/// Something the user can choose.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum SettingKey {
    /// Ask for less motion than the platform did.
    ReduceMotion,
    /// Ask for more contrast than the platform did.
    IncreaseContrast,
    /// Ask for less transparency than the platform did.
    ReduceTransparency,

    /// Offer a suggestion that was not asked for.
    ShowAiSuggestions,
    /// Explain a decision without being asked.
    ExplainDecisions,
    /// Confirm an action the log can undo.
    ConfirmReversibleActions,

    /// Snap edits to the musical grid.
    SnapToGrid,
    /// What edits snap to.
    SnapResolution,
    /// Analyse a track when it is imported rather than when it is first needed.
    AnalyseOnImport,
    /// Load a track at its cue point rather than at its start.
    CueOnLoad,

    /// How a key is written.
    KeyNotation,
    /// Where an export goes unless told otherwise.
    DefaultDeliveryTarget,
    /// How many recent projects to offer.
    RecentProjectsShown,
}

impl SettingKey {
    /// Every setting, so that a preferences screen cannot omit one.
    pub const ALL: [Self; 13] = [
        Self::ReduceMotion,
        Self::IncreaseContrast,
        Self::ReduceTransparency,
        Self::ShowAiSuggestions,
        Self::ExplainDecisions,
        Self::ConfirmReversibleActions,
        Self::SnapToGrid,
        Self::SnapResolution,
        Self::AnalyseOnImport,
        Self::CueOnLoad,
        Self::KeyNotation,
        Self::DefaultDeliveryTarget,
        Self::RecentProjectsShown,
    ];

    /// The most recent projects that may be offered.
    pub const MAX_RECENT_PROJECTS: i64 = 50;

    /// What this setting is about.
    #[must_use]
    pub const fn category(self) -> Category {
        match self {
            Self::ReduceMotion | Self::IncreaseContrast | Self::ReduceTransparency => {
                Category::Accessibility
            }
            Self::ShowAiSuggestions | Self::ExplainDecisions | Self::ConfirmReversibleActions => {
                Category::Assistance
            }
            Self::SnapToGrid | Self::SnapResolution | Self::AnalyseOnImport | Self::CueOnLoad => {
                Category::Behaviour
            }
            Self::KeyNotation | Self::DefaultDeliveryTarget | Self::RecentProjectsShown => {
                Category::Presentation
            }
        }
    }

    /// What kind of value this setting holds.
    #[must_use]
    pub const fn kind(self) -> ValueKind {
        match self {
            Self::ReduceMotion
            | Self::IncreaseContrast
            | Self::ReduceTransparency
            | Self::ShowAiSuggestions
            | Self::ExplainDecisions
            | Self::ConfirmReversibleActions
            | Self::SnapToGrid
            | Self::AnalyseOnImport
            | Self::CueOnLoad => ValueKind::Flag,
            Self::RecentProjectsShown => ValueKind::Count,
            Self::SnapResolution | Self::KeyNotation | Self::DefaultDeliveryTarget => {
                ValueKind::Choice
            }
        }
    }

    /// The permitted names, for a choice setting.
    #[must_use]
    pub const fn choices(self) -> &'static [&'static str] {
        match self {
            Self::SnapResolution => &choices::SNAP,
            Self::KeyNotation => &choices::KEY_NOTATION,
            Self::DefaultDeliveryTarget => &choices::DELIVERY,
            _ => &[],
        }
    }

    /// The permitted range, for a count setting.
    #[must_use]
    pub const fn range(self) -> Option<(i64, i64)> {
        match self {
            Self::RecentProjectsShown => Some((0, Self::MAX_RECENT_PROJECTS)),
            _ => None,
        }
    }

    /// What this setting is when the user has not chosen.
    ///
    /// Only [`Category::Assistance`] settings look at the mode. Everything else
    /// returns the same answer whatever mode is in force, which is the rule that
    /// keeps a mode from changing behaviour.
    #[must_use]
    pub const fn default_for(self, mode: ExperienceMode) -> SettingValue {
        match self {
            // Accessibility: nothing extra is requested here by default, because
            // the platform's answer is the one that matters and this is only the
            // addition on top of it.
            Self::ReduceMotion | Self::IncreaseContrast | Self::ReduceTransparency => {
                SettingValue::Flag(false)
            }

            // Assistance: the mode's whole job.
            Self::ShowAiSuggestions => SettingValue::Flag(mode.volunteers_suggestions()),
            Self::ExplainDecisions => SettingValue::Flag(mode.volunteers_explanations()),
            Self::ConfirmReversibleActions => {
                SettingValue::Flag(mode.confirms_reversible_actions())
            }

            // Behaviour: the same at every mode. Snapping, analysing on import
            // and cueing on load are all on by default, because each is what
            // someone would do by hand a moment later.
            Self::SnapToGrid | Self::AnalyseOnImport | Self::CueOnLoad => SettingValue::Flag(true),
            Self::SnapResolution => SettingValue::Choice("snap.bar"),

            // Presentation: the same at every mode.
            Self::KeyNotation => SettingValue::Choice("notation.camelot"),
            Self::DefaultDeliveryTarget => SettingValue::Choice("target.streaming"),
            Self::RecentProjectsShown => SettingValue::Count(10),
        }
    }

    /// Whether this setting's default depends on the experience mode.
    #[must_use]
    pub fn follows_the_mode(self) -> bool {
        ExperienceMode::ALL
            .into_iter()
            .any(|mode| self.default_for(mode) != self.default_for(ExperienceMode::Standard))
    }

    /// A stable identifier, for storage and localisation.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::ReduceMotion => "setting.reduce_motion",
            Self::IncreaseContrast => "setting.increase_contrast",
            Self::ReduceTransparency => "setting.reduce_transparency",
            Self::ShowAiSuggestions => "setting.show_ai_suggestions",
            Self::ExplainDecisions => "setting.explain_decisions",
            Self::ConfirmReversibleActions => "setting.confirm_reversible_actions",
            Self::SnapToGrid => "setting.snap_to_grid",
            Self::SnapResolution => "setting.snap_resolution",
            Self::AnalyseOnImport => "setting.analyse_on_import",
            Self::CueOnLoad => "setting.cue_on_load",
            Self::KeyNotation => "setting.key_notation",
            Self::DefaultDeliveryTarget => "setting.default_delivery_target",
            Self::RecentProjectsShown => "setting.recent_projects_shown",
        }
    }

    /// Reads a stored identifier.
    ///
    /// `None` for anything unrecognised — which is not an error. A settings
    /// document written by a newer build contains settings this one has never
    /// heard of, and the right response is to keep them, not to reject the file.
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|setting| setting.key() == key)
    }
}

impl fmt::Display for SettingKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.key())
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
    fn only_assistance_settings_follow_the_mode() {
        // The mechanical form of the rule that a mode changes how much the
        // product volunteers and never what the user may do. A future setting
        // that made a mode change behaviour fails here rather than in a review.
        for setting in SettingKey::ALL {
            if setting.follows_the_mode() {
                assert!(
                    setting.category().follows_the_mode(),
                    "{setting} is {} and still varies with the mode",
                    setting.category().key()
                );
            }
        }
    }

    #[test]
    fn every_assistance_setting_actually_uses_the_mode() {
        // The other direction. An assistance setting with a fixed default is
        // one the mode was supposed to reach and does not, which is a silent
        // gap rather than a visible one.
        for setting in SettingKey::ALL {
            if setting.category() == Category::Assistance {
                assert!(
                    setting.follows_the_mode(),
                    "{setting} is assistance and ignores the mode"
                );
            }
        }
    }

    #[test]
    fn every_default_matches_its_declared_kind() {
        for setting in SettingKey::ALL {
            for mode in ExperienceMode::ALL {
                assert_eq!(
                    setting.default_for(mode).kind(),
                    setting.kind(),
                    "{setting} has a default of the wrong kind"
                );
            }
        }
    }

    #[test]
    fn every_choice_default_is_one_of_the_permitted_choices() {
        // A default outside its own permitted list would be a value the user
        // could never return to after changing it.
        for setting in SettingKey::ALL {
            if setting.kind() != ValueKind::Choice {
                assert!(setting.choices().is_empty(), "{setting} lists choices");
                continue;
            }
            let choices = setting.choices();
            assert!(!choices.is_empty(), "{setting} permits nothing");
            let default = setting
                .default_for(ExperienceMode::Standard)
                .as_choice()
                .expect("a choice setting has a choice default");
            assert!(
                choices.contains(&default),
                "{setting} defaults to {default}, which it does not permit"
            );
        }
    }

    #[test]
    fn every_count_default_is_inside_its_own_range() {
        for setting in SettingKey::ALL {
            let Some((low, high)) = setting.range() else {
                assert_ne!(setting.kind(), ValueKind::Count, "{setting} has no range");
                continue;
            };
            assert!(low <= high, "{setting} has an empty range");
            let default = setting
                .default_for(ExperienceMode::Standard)
                .as_count()
                .expect("a count setting has a count default");
            assert!(
                (low..=high).contains(&default),
                "{setting} defaults outside its range"
            );
        }
    }

    #[test]
    fn an_accessibility_setting_defaults_to_adding_nothing() {
        // The platform's answer is the one that matters; this is only what is
        // added on top of it.
        for setting in SettingKey::ALL {
            if setting.category() == Category::Accessibility {
                assert_eq!(
                    setting.default_for(ExperienceMode::Standard),
                    SettingValue::Flag(false),
                    "{setting} presumes an accommodation the user did not ask for"
                );
            }
        }
    }

    #[test]
    fn an_unrecognised_stored_setting_is_not_an_error() {
        for setting in SettingKey::ALL {
            assert_eq!(SettingKey::from_key(setting.key()), Some(setting));
        }
        assert_eq!(SettingKey::from_key("setting.from_the_future"), None);
    }

    #[test]
    fn setting_keys_are_distinct() {
        let keys: Vec<&str> = SettingKey::ALL.iter().map(|s| s.key()).collect();
        for (index, key) in keys.iter().enumerate() {
            for (other, value) in keys.iter().enumerate() {
                assert!(index == other || key != value, "two settings share {key}");
            }
        }
    }

    #[test]
    fn a_value_reports_only_the_kind_it_is() {
        assert_eq!(SettingValue::Flag(true).as_flag(), Some(true));
        assert_eq!(SettingValue::Flag(true).as_count(), None);
        assert_eq!(SettingValue::Count(3).as_count(), Some(3));
        assert_eq!(SettingValue::Count(3).as_choice(), None);
        assert_eq!(SettingValue::Choice("a").as_choice(), Some("a"));
        assert_eq!(SettingValue::Choice("a").as_flag(), None);
        assert_eq!(SettingValue::Flag(false).to_string(), "false");
        assert_eq!(SettingValue::Count(-2).to_string(), "-2");
    }
}
