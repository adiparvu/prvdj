//! How much the product volunteers.
//!
//! # A mode changes what is offered, never what is possible
//!
//! Master Prompt #8 requires that the interface never intervenes uninvited for a
//! professional. The obvious implementation of that is a "professional mode"
//! that hides things, and it is the wrong one: a mode that removes capability
//! turns a preference into a downgrade, and the user who picked it discovers
//! six months later that the feature they needed was there all along behind a
//! setting they chose for unrelated reasons.
//!
//! So the rule here is narrow and absolute. A mode changes *defaults* — how much
//! is explained, how much is suggested, how much appears without being asked
//! for. It never changes what a user may do. There is a test that says so over
//! every setting, and it is the test to keep if the rest of this module is ever
//! rewritten.
//!
//! # It is not a skill level
//!
//! [`ExperienceMode::Guided`] is not "beginner". People switch to it when they
//! are working in an unfamiliar genre, when they are tired, or when they are
//! doing something they only do twice a year. Naming it after the *behaviour*
//! rather than after the user is deliberate: nobody has to identify as a
//! beginner to get an explanation.

use core::fmt;

/// How much the product does without being asked.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum ExperienceMode {
    /// Explains, suggests, and confirms before anything destructive.
    Guided,

    /// The middle setting, and the default.
    ///
    /// Suggestions appear where they are useful and nowhere else. Chosen as the
    /// default because a product that starts at either extreme teaches the user
    /// the wrong thing about itself on first run.
    #[default]
    Standard,

    /// Never intervenes uninvited.
    ///
    /// No unsolicited suggestion, no explanatory overlay, no confirmation for an
    /// action the log can undo. Everything remains reachable; nothing arrives on
    /// its own.
    Professional,
}

impl ExperienceMode {
    /// Every mode, most assistance first.
    pub const ALL: [Self; 3] = [Self::Guided, Self::Standard, Self::Professional];

    /// Whether the product offers a suggestion the user did not ask for.
    #[must_use]
    pub const fn volunteers_suggestions(self) -> bool {
        matches!(self, Self::Guided | Self::Standard)
    }

    /// Whether the product explains itself without being asked.
    #[must_use]
    pub const fn volunteers_explanations(self) -> bool {
        matches!(self, Self::Guided)
    }

    /// Whether an action that can be undone is confirmed first.
    ///
    /// False beyond [`Self::Guided`], and this is a considered position rather
    /// than a convenience. Master Prompt #9 makes every edit reversible, and a
    /// confirmation dialogue for a reversible action trains the user to dismiss
    /// dialogues — which is precisely what makes the *irreversible* one
    /// dangerous. Actions the log cannot undo are confirmed at every mode, and
    /// that is not a setting.
    #[must_use]
    pub const fn confirms_reversible_actions(self) -> bool {
        matches!(self, Self::Guided)
    }

    /// A stable identifier, for storage and localisation.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Guided => "mode.guided",
            Self::Standard => "mode.standard",
            Self::Professional => "mode.professional",
        }
    }

    /// Reads a stored identifier.
    ///
    /// Returns `None` for anything unrecognised, so a settings document written
    /// by a newer build falls back to the default rather than failing to open.
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|mode| mode.key() == key)
    }
}

impl fmt::Display for ExperienceMode {
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
    fn assistance_decreases_and_never_reverses() {
        // Guided offers at least as much as Standard, which offers at least as
        // much as Professional. A mode that volunteered *more* than a more
        // guided one would make the ordering meaningless.
        let volunteering: Vec<(bool, bool, bool)> = ExperienceMode::ALL
            .into_iter()
            .map(|mode| {
                (
                    mode.volunteers_explanations(),
                    mode.volunteers_suggestions(),
                    mode.confirms_reversible_actions(),
                )
            })
            .collect();

        for window in volunteering.windows(2) {
            let [earlier, later] = window else { continue };
            assert!(earlier.0 >= later.0, "explanations increased");
            assert!(earlier.1 >= later.1, "suggestions increased");
            assert!(earlier.2 >= later.2, "confirmations increased");
        }
    }

    #[test]
    fn the_professional_mode_is_quiet_rather_than_reduced() {
        // The distinction the module exists to hold. Nothing arrives on its
        // own; everything remains reachable.
        let professional = ExperienceMode::Professional;
        assert!(!professional.volunteers_suggestions());
        assert!(!professional.volunteers_explanations());
        assert!(!professional.confirms_reversible_actions());
    }

    #[test]
    fn the_default_is_the_middle_setting() {
        // A product that starts at either extreme teaches the user the wrong
        // thing about itself on first run.
        assert_eq!(ExperienceMode::default(), ExperienceMode::Standard);
    }

    #[test]
    fn an_unrecognised_stored_mode_falls_back_rather_than_failing() {
        // A settings document written by a newer build must still open.
        for mode in ExperienceMode::ALL {
            assert_eq!(ExperienceMode::from_key(mode.key()), Some(mode));
        }
        assert_eq!(ExperienceMode::from_key("mode.from_the_future"), None);
        assert_eq!(ExperienceMode::from_key(""), None);
    }

    #[test]
    fn mode_keys_are_distinct() {
        let keys: Vec<&str> = ExperienceMode::ALL.iter().map(|m| m.key()).collect();
        for (index, key) in keys.iter().enumerate() {
            for (other, value) in keys.iter().enumerate() {
                assert!(index == other || key != value, "two modes share {key}");
            }
        }
    }
}
