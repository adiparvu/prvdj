//! What the product tells the user, and when.
//!
//! # The only question that matters is what may interrupt a performance
//!
//! Everything else in a notification system is presentation. The decision that
//! is genuinely difficult, and genuinely damaging to get wrong, is which
//! messages are allowed to appear in front of someone who is playing to a room.
//!
//! The rule here is narrow: **only a notice about the sound coming out right
//! now**. A plugin that has been passed over, an audio device that has gone, a
//! file that has stopped reading — three things, each of which the person on
//! stage is about to hear and can act on. Everything else waits, including
//! things that feel urgent to the system and are not urgent to the human: a
//! synchronisation conflict, a finished export, a full outbox.
//!
//! Master Prompt #8's rule that the interface never intervenes uninvited for a
//! professional and Master Prompt #19's rule that audio performance comes first
//! meet here, and this module is where they are decided once rather than at
//! every call site.
//!
//! # Nothing is dropped for being inconvenient
//!
//! A notice that is withheld during a performance is *held*, not discarded.
//! `prv-ai` defers work rather than dropping it and `prv-sync` refuses rather
//! than forgetting; this is the same rule applied to messages, and for the same
//! reason: a product that silently decides not to mention something is one whose
//! silence carries no information.
//!
//! # Repetition is coalesced, not suppressed
//!
//! Forty tracks finishing analysis is one notice with a count, not forty
//! notices and not one notice about the last of them. The count is the part that
//! makes it a summary rather than a lie by omission.

use std::collections::BTreeMap;

use core::fmt;

use prv_settings::ExperienceMode;

/// What a notice is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum Notice {
    /// A plugin was passed over after misbehaving.
    PluginBypassed,
    /// The audio device changed or went away.
    AudioDeviceChanged,
    /// A file a playing track needs stopped reading.
    MediaUnreadable,

    /// A track finished being analysed.
    TrackAnalysed,
    /// A track could not be analysed.
    AnalysisFailed,
    /// A set finished being planned.
    PlanReady,
    /// An export finished.
    ExportFinished,
    /// An export will not meet its target.
    ExportBlocked,
    /// Two devices changed the same thing.
    SyncConflict,
    /// Edits are piling up with nowhere to go.
    OutboxFilling,
    /// A plugin asked for a permission.
    PermissionRequested,
    /// Work was postponed until the set finishes.
    WorkDeferred,
}

impl Notice {
    /// Every notice.
    pub const ALL: [Self; 12] = [
        Self::PluginBypassed,
        Self::AudioDeviceChanged,
        Self::MediaUnreadable,
        Self::TrackAnalysed,
        Self::AnalysisFailed,
        Self::PlanReady,
        Self::ExportFinished,
        Self::ExportBlocked,
        Self::SyncConflict,
        Self::OutboxFilling,
        Self::PermissionRequested,
        Self::WorkDeferred,
    ];

    /// Whether this is about the sound coming out right now.
    ///
    /// The whole of the interruption rule. Three notices qualify, and each is
    /// something the person on stage is about to hear and can act on. A
    /// synchronisation conflict is urgent to the system and is not urgent to
    /// the human standing in front of an audience.
    #[must_use]
    pub const fn concerns_the_sound_right_now(self) -> bool {
        matches!(
            self,
            Self::PluginBypassed | Self::AudioDeviceChanged | Self::MediaUnreadable
        )
    }

    /// Whether this asks the user for something rather than telling them.
    ///
    /// A question that is never asked is a feature that never works, so these
    /// are held rather than merged away when they wait.
    #[must_use]
    pub const fn asks_a_question(self) -> bool {
        matches!(self, Self::PermissionRequested | Self::SyncConflict)
    }

    /// Whether this is routine enough to be worth saying only once with a
    /// count.
    #[must_use]
    pub const fn is_repetitive(self) -> bool {
        matches!(self, Self::TrackAnalysed | Self::AnalysisFailed)
    }

    /// The quietest mode that mentions this without being asked.
    ///
    /// A professional hears about the sound and about questions; everything else
    /// is available to look at and does not arrive on its own.
    #[must_use]
    pub const fn quietest_mode_that_volunteers_it(self) -> ExperienceMode {
        if self.concerns_the_sound_right_now() || self.asks_a_question() {
            ExperienceMode::Professional
        } else {
            ExperienceMode::Standard
        }
    }

    /// A stable identifier, for storage and localisation.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::PluginBypassed => "notice.plugin_bypassed",
            Self::AudioDeviceChanged => "notice.audio_device_changed",
            Self::MediaUnreadable => "notice.media_unreadable",
            Self::TrackAnalysed => "notice.track_analysed",
            Self::AnalysisFailed => "notice.analysis_failed",
            Self::PlanReady => "notice.plan_ready",
            Self::ExportFinished => "notice.export_finished",
            Self::ExportBlocked => "notice.export_blocked",
            Self::SyncConflict => "notice.sync_conflict",
            Self::OutboxFilling => "notice.outbox_filling",
            Self::PermissionRequested => "notice.permission_requested",
            Self::WorkDeferred => "notice.work_deferred",
        }
    }
}

impl fmt::Display for Notice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.key())
    }
}

/// What the user is doing.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Attention {
    /// At the desk, not playing to anyone.
    #[default]
    AtTheDesk,
    /// Playing to a room.
    Performing,
}

impl Attention {
    /// A stable identifier, for storage and localisation.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::AtTheDesk => "attention.at_the_desk",
            Self::Performing => "attention.performing",
        }
    }
}

/// A notice, with how many times the same thing has happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pending {
    notice: Notice,
    occurrences: u32,
}

impl Pending {
    /// What it is about.
    #[must_use]
    pub const fn notice(self) -> Notice {
        self.notice
    }

    /// How many times it happened.
    ///
    /// One unless the notice is repetitive. The count is what makes a summary a
    /// summary rather than a lie by omission — "forty tracks analysed" is a
    /// different message from "a track was analysed".
    #[must_use]
    pub const fn occurrences(self) -> u32 {
        self.occurrences
    }
}

/// What is waiting to be said.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Notifications {
    held: BTreeMap<Notice, u32>,
    shown: Vec<Pending>,
}

impl Notifications {
    /// How many distinct notices may wait.
    ///
    /// Bounded by the vocabulary rather than by a number: a repetitive notice
    /// coalesces, so what accumulates is kinds, not occurrences, and there are
    /// only ever [`Notice::ALL`] of those.
    pub const MAX_HELD: usize = Notice::ALL.len();

    /// Nothing waiting.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Offers a notice, given what the user is doing and how much they want to
    /// hear.
    ///
    /// Returns whether it is shown now. Anything not shown now is *held*, and
    /// [`Self::release`] produces it when the moment passes — a notice is never
    /// dropped for being inconvenient.
    pub fn raise(&mut self, notice: Notice, attention: Attention, mode: ExperienceMode) -> bool {
        let interrupts = match attention {
            // Playing to a room: only the sound coming out right now.
            Attention::Performing => notice.concerns_the_sound_right_now(),
            // At the desk: whatever this mode volunteers.
            Attention::AtTheDesk => mode <= notice.quietest_mode_that_volunteers_it(),
        };

        if interrupts {
            self.shown.push(Pending {
                notice,
                occurrences: 1,
            });
            return true;
        }

        let waiting = self.held.entry(notice).or_insert(0);
        *waiting = waiting.saturating_add(1);
        false
    }

    /// Everything held, coalesced, in notice order — and clears the queue.
    ///
    /// Called when a set finishes or a user opens the list. A repetitive notice
    /// arrives once with its count; a question arrives once per distinct
    /// question, because a question that is merged away is a feature that never
    /// works.
    pub fn release(&mut self) -> Vec<Pending> {
        let released: Vec<Pending> = self
            .held
            .iter()
            .flat_map(|(notice, occurrences)| {
                if notice.is_repetitive() {
                    vec![Pending {
                        notice: *notice,
                        occurrences: *occurrences,
                    }]
                } else {
                    // Not repetitive: each occurrence is its own event and
                    // deserves its own line. There are only ever a few, because
                    // the things that happen in bulk are the repetitive ones.
                    (0..*occurrences)
                        .map(|_| Pending {
                            notice: *notice,
                            occurrences: 1,
                        })
                        .collect()
                }
            })
            .collect();
        self.held.clear();
        released
    }

    /// What is waiting, without clearing it.
    #[must_use]
    pub fn waiting(&self) -> Vec<Pending> {
        self.held
            .iter()
            .map(|(notice, occurrences)| Pending {
                notice: *notice,
                occurrences: *occurrences,
            })
            .collect()
    }

    /// How many notices have been shown.
    #[must_use]
    pub fn shown(&self) -> &[Pending] {
        &self.shown
    }

    /// Whether anything is waiting.
    #[must_use]
    pub fn has_waiting(&self) -> bool {
        !self.held.is_empty()
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
    fn only_the_sound_coming_out_right_now_reaches_someone_on_stage() {
        // The decision this module exists for. A synchronisation conflict is
        // urgent to the system and is not urgent to the human standing in front
        // of an audience.
        for mode in ExperienceMode::ALL {
            for notice in Notice::ALL {
                let mut notifications = Notifications::new();
                let shown = notifications.raise(notice, Attention::Performing, mode);
                assert_eq!(
                    shown,
                    notice.concerns_the_sound_right_now(),
                    "{notice} in {mode} reached the wrong place during a performance"
                );
            }
        }
    }

    #[test]
    fn exactly_three_things_may_interrupt_a_set() {
        // Named individually, so that adding a fourth is a change somebody
        // argues for rather than one that slips in with a feature.
        let interrupting: Vec<Notice> = Notice::ALL
            .into_iter()
            .filter(|notice| notice.concerns_the_sound_right_now())
            .collect();
        assert_eq!(
            interrupting,
            vec![
                Notice::PluginBypassed,
                Notice::AudioDeviceChanged,
                Notice::MediaUnreadable,
            ]
        );
    }

    #[test]
    fn nothing_withheld_during_a_set_is_thrown_away() {
        // A product that silently decides not to mention something is one whose
        // silence carries no information.
        let mut notifications = Notifications::new();
        for notice in Notice::ALL {
            notifications.raise(notice, Attention::Performing, ExperienceMode::Standard);
        }

        let released = notifications.release();
        let kinds: Vec<Notice> = released.iter().map(|p| p.notice()).collect();
        for notice in Notice::ALL {
            if notice.concerns_the_sound_right_now() {
                continue;
            }
            assert!(kinds.contains(&notice), "{notice} was lost");
        }
        assert!(!notifications.has_waiting(), "release did not clear");
    }

    #[test]
    fn forty_analysed_tracks_are_one_notice_with_a_count() {
        // The count is what makes it a summary rather than a lie by omission:
        // "forty tracks analysed" is a different message from "a track was
        // analysed".
        let mut notifications = Notifications::new();
        for _ in 0..40 {
            notifications.raise(
                Notice::TrackAnalysed,
                Attention::Performing,
                ExperienceMode::Standard,
            );
        }

        let released = notifications.release();
        assert_eq!(released.len(), 1);
        assert_eq!(
            released.first().map(|pending| pending.occurrences()),
            Some(40)
        );
    }

    #[test]
    fn a_question_is_never_merged_away() {
        // A question that is merged away is a feature that never works: two
        // plugins asking for permission is two decisions, and answering one is
        // not answering the other.
        let mut notifications = Notifications::new();
        for _ in 0..3 {
            notifications.raise(
                Notice::PermissionRequested,
                Attention::Performing,
                ExperienceMode::Standard,
            );
        }

        let released = notifications.release();
        assert_eq!(released.len(), 3, "two permission requests became one");
        assert!(Notice::PermissionRequested.asks_a_question());
        assert!(!Notice::PermissionRequested.is_repetitive());
    }

    #[test]
    fn a_professional_at_the_desk_hears_about_the_sound_and_about_questions() {
        // Master Prompt #8: the interface never intervenes uninvited for a
        // professional. Everything else is available to look at.
        let mut notifications = Notifications::new();
        for notice in Notice::ALL {
            let shown =
                notifications.raise(notice, Attention::AtTheDesk, ExperienceMode::Professional);
            let expected = notice.concerns_the_sound_right_now() || notice.asks_a_question();
            assert_eq!(shown, expected, "{notice} was wrong for a professional");
        }
    }

    #[test]
    fn a_guided_user_hears_about_everything_at_the_desk() {
        let mut notifications = Notifications::new();
        for notice in Notice::ALL {
            assert!(
                notifications.raise(notice, Attention::AtTheDesk, ExperienceMode::Guided),
                "{notice} was withheld from a guided user at the desk"
            );
        }
        assert!(!notifications.has_waiting());
        assert_eq!(notifications.shown().len(), Notice::ALL.len());
    }

    #[test]
    fn what_waits_is_bounded_by_the_vocabulary_and_not_by_how_much_happened() {
        // A repetitive notice coalesces, so what accumulates is kinds.
        let mut notifications = Notifications::new();
        for _ in 0..1000 {
            notifications.raise(
                Notice::TrackAnalysed,
                Attention::Performing,
                ExperienceMode::Standard,
            );
        }
        assert_eq!(notifications.waiting().len(), 1);
        assert!(
            notifications.waiting().len() <= Notifications::MAX_HELD,
            "more kinds are waiting than there are kinds"
        );
    }

    #[test]
    fn notice_keys_are_distinct() {
        let keys: Vec<&str> = Notice::ALL.iter().map(|n| n.key()).collect();
        for (index, key) in keys.iter().enumerate() {
            for (other, value) in keys.iter().enumerate() {
                assert!(index == other || key != value, "two notices share {key}");
            }
        }
        assert_ne!(Attention::AtTheDesk.key(), Attention::Performing.key());
        assert_eq!(Attention::default(), Attention::AtTheDesk);
    }
}
