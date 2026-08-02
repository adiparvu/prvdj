//! How the application behaves, and what it is willing to interrupt for.
//!
//! # Why settings and notifications are one module
//!
//! Because they are one decision. `prv-notify` will not raise a notice unless
//! the experience mode is one that volunteers it, so a host that read the two
//! separately would have to reimplement that rule to know what to show — and
//! would get it wrong the first time somebody switched to Performing mid-set.
//!
//! Keeping them together means the host asks *what should I show* and gets an
//! answer that has already accounted for the mode.
//!
//! # Nothing is shown during a performance that can wait
//!
//! [`Attention::Performing`] is the state that matters most and is easiest to
//! get wrong. A dialogue over a set is worse than the problem it reports, almost
//! always — so a notice raised while performing is *held*, not dropped, and
//! [`Experience::release`] hands the backlog over when the user is at the desk
//! again.
//!
//! Dropping would be simpler and would lose the one notice that mattered.

use prv_notify::{Attention, Notice, Notifications};
use prv_settings::{ExperienceMode, SettingKey, SettingValue, Settings};

use crate::status::Status;

/// How the application behaves and what it has queued to say.
#[derive(Debug)]
pub struct Experience {
    settings: Settings,
    notifications: Notifications,
    attention: Attention,
    /// The notices handed over by the last [`Self::release`], kept so a host
    /// can read them back one at a time.
    released: Vec<(Notice, u32)>,
}

impl Default for Experience {
    fn default() -> Self {
        Self::new()
    }
}

impl Experience {
    /// Standard mode, at the desk, nothing queued.
    #[must_use]
    pub fn new() -> Self {
        Self {
            settings: Settings::new(),
            notifications: Notifications::new(),
            attention: Attention::AtTheDesk,
            released: Vec::new(),
        }
    }

    /// Sets the experience mode.
    ///
    /// # Errors
    ///
    /// [`Status::InvalidArgument`] when the code names no mode.
    pub fn set_mode(&mut self, mode_code: i32) -> Result<(), Status> {
        let mode = mode_from_code(mode_code).ok_or(Status::InvalidArgument)?;
        self.settings.set_mode(mode);
        Ok(())
    }

    /// The current mode's code.
    #[must_use]
    pub fn mode(&self) -> i32 {
        mode_code(self.settings.mode())
    }

    /// Sets where the user's attention is.
    ///
    /// # Errors
    ///
    /// [`Status::InvalidArgument`] when the code names no attention state.
    pub fn set_attention(&mut self, attention_code: i32) -> Result<(), Status> {
        self.attention = attention_from_code(attention_code).ok_or(Status::InvalidArgument)?;
        Ok(())
    }

    /// Reads a boolean setting.
    ///
    /// # Errors
    ///
    /// [`Status::InvalidArgument`] when the code names no setting.
    pub fn flag(&self, setting_code: i32) -> Result<bool, Status> {
        let key = setting_from_code(setting_code).ok_or(Status::InvalidArgument)?;
        Ok(self.settings.flag(key))
    }

    /// Sets a boolean setting.
    ///
    /// # Errors
    ///
    /// [`Status::InvalidArgument`] when the code names no setting, and
    /// [`Status::Refused`] when the setting does not take a boolean or the mode
    /// does not permit changing it.
    pub fn set_flag(&mut self, setting_code: i32, value: bool) -> Result<(), Status> {
        let key = setting_from_code(setting_code).ok_or(Status::InvalidArgument)?;
        self.settings
            .set(key, SettingValue::Flag(value))
            .map_err(|_| Status::Refused)
    }

    /// Raises a notice, and says whether it will be shown now.
    ///
    /// `false` does not mean it was discarded. A notice raised while performing
    /// is held and comes back from [`Self::release`]; one the current mode does
    /// not volunteer is not the user's problem to see.
    ///
    /// # Errors
    ///
    /// [`Status::InvalidArgument`] when the code names no notice.
    pub fn raise(&mut self, notice_code: i32) -> Result<bool, Status> {
        let notice = notice_from_code(notice_code).ok_or(Status::InvalidArgument)?;
        Ok(self
            .notifications
            .raise(notice, self.attention, self.settings.mode()))
    }

    /// Hands over everything held back during a performance.
    ///
    /// Returns how many notices came out. Read them with [`Self::released`].
    pub fn release(&mut self) -> u64 {
        self.released = self
            .notifications
            .release()
            .into_iter()
            .map(|pending| (pending.notice(), pending.occurrences()))
            .collect();
        self.released.len().try_into().unwrap_or(u64::MAX)
    }

    /// One notice from the last release: its code, and how many times it
    /// happened.
    ///
    /// The count matters. Six identical buffer warnings during a set are one
    /// problem that happened six times, and showing six dialogues afterwards
    /// would be the notification doing more damage than the fault.
    ///
    /// # Errors
    ///
    /// [`Status::InvalidArgument`] when there is nothing at that index.
    pub fn released(&self, index: u64) -> Result<(i32, u32), Status> {
        let index = usize::try_from(index).map_err(|_| Status::InvalidArgument)?;
        self.released
            .get(index)
            .map(|(notice, count)| (notice_code(*notice), *count))
            .ok_or(Status::InvalidArgument)
    }

    /// Whether anything is waiting to be shown.
    #[must_use]
    pub fn has_waiting(&self) -> bool {
        self.notifications.has_waiting()
    }

    /// Whether a notice concerns the sound happening right now.
    ///
    /// The one class that may interrupt a performance, because a performer who
    /// is not told the right deck is silent finds out from the room.
    ///
    /// # Errors
    ///
    /// [`Status::InvalidArgument`] when the code names no notice.
    pub fn concerns_the_sound(notice_code: i32) -> Result<bool, Status> {
        let notice = notice_from_code(notice_code).ok_or(Status::InvalidArgument)?;
        Ok(notice.concerns_the_sound_right_now())
    }
}

/// The mode a code names.
#[must_use]
pub const fn mode_from_code(code: i32) -> Option<ExperienceMode> {
    match code {
        0 => Some(ExperienceMode::Guided),
        1 => Some(ExperienceMode::Standard),
        2 => Some(ExperienceMode::Professional),
        _ => None,
    }
}

/// The code a mode has.
#[must_use]
pub const fn mode_code(mode: ExperienceMode) -> i32 {
    match mode {
        ExperienceMode::Guided => 0,
        ExperienceMode::Professional => 2,
        // `Standard`, and any mode added later.
        //
        // `ExperienceMode` is `#[non_exhaustive]`, so a mode introduced next
        // year reaches here before anybody has thought about the ABI. Standard
        // is the safe landing: Guided would hide controls a professional
        // expects, and Professional would surface ones a new user should not
        // meet yet.
        _ => 1,
    }
}

/// The attention state a code names.
#[must_use]
pub const fn attention_from_code(code: i32) -> Option<Attention> {
    match code {
        0 => Some(Attention::AtTheDesk),
        1 => Some(Attention::Performing),
        _ => None,
    }
}

/// The setting a code names.
#[must_use]
pub fn setting_from_code(code: i32) -> Option<SettingKey> {
    usize::try_from(code)
        .ok()
        .and_then(|index| SettingKey::ALL.get(index).copied())
}

/// The notice a code names.
#[must_use]
pub fn notice_from_code(code: i32) -> Option<Notice> {
    usize::try_from(code)
        .ok()
        .and_then(|index| Notice::ALL.get(index).copied())
}

/// The code a notice has.
#[must_use]
pub fn notice_code(notice: Notice) -> i32 {
    i32::try_from(
        Notice::ALL
            .iter()
            .position(|candidate| *candidate == notice)
            .unwrap_or(0),
    )
    .unwrap_or(0)
}

/// Every mode with its C spelling, in code order.
pub const MODES: &[(ExperienceMode, &str)] = &[
    (ExperienceMode::Guided, "PRV_MODE_GUIDED"),
    (ExperienceMode::Standard, "PRV_MODE_STANDARD"),
    (ExperienceMode::Professional, "PRV_MODE_PROFESSIONAL"),
];

/// Every attention state with its C spelling, in code order.
pub const ATTENTION: &[(Attention, &str)] = &[
    (Attention::AtTheDesk, "PRV_ATTENTION_AT_THE_DESK"),
    (Attention::Performing, "PRV_ATTENTION_PERFORMING"),
];

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::panic,
        reason = "a test that cannot build its own fixture should fail loudly"
    )]

    use super::*;

    /// A notice that is not about the sound right now, so it is one a
    /// performance should hold rather than show.
    fn a_quiet_notice() -> i32 {
        let notice = Notice::ALL
            .iter()
            .find(|notice| !notice.concerns_the_sound_right_now() && !notice.asks_a_question())
            .copied()
            .expect("at least one notice can wait");
        notice_code(notice)
    }

    #[test]
    fn nothing_that_can_wait_interrupts_a_performance() {
        // A dialogue over a set is worse than the problem it reports, almost
        // always.
        let mut experience = Experience::new();
        experience.set_attention(1).expect("performing");

        let quiet = a_quiet_notice();
        let shown = experience.raise(quiet).expect("a real notice");
        assert!(!shown, "a notice that could wait was shown mid-performance");
        assert!(experience.has_waiting());
    }

    #[test]
    fn what_was_held_back_comes_out_afterwards_rather_than_being_dropped() {
        // Dropping would be simpler and would lose the one notice that
        // mattered.
        let mut experience = Experience::new();
        experience.set_attention(1).expect("performing");
        let quiet = a_quiet_notice();
        experience.raise(quiet).expect("a real notice");

        experience.set_attention(0).expect("at the desk");
        assert_eq!(experience.release(), 1);
        let (code, occurrences) = experience.released(0).expect("one was released");
        assert_eq!(code, quiet);
        assert_eq!(occurrences, 1);
    }

    #[test]
    fn six_of_the_same_fault_is_one_notice_that_happened_six_times() {
        // Showing six dialogues afterwards would be the notification doing more
        // damage than the fault.
        let mut experience = Experience::new();
        experience.set_attention(1).expect("performing");
        let quiet = a_quiet_notice();
        for _ in 0..6 {
            experience.raise(quiet).expect("a real notice");
        }

        experience.set_attention(0).expect("at the desk");
        assert_eq!(experience.release(), 1, "six dialogues instead of one");
        let (_, occurrences) = experience.released(0).expect("one was released");
        assert_eq!(
            occurrences, 6,
            "the count was lost, so the user cannot judge"
        );
    }

    #[test]
    fn a_notice_about_the_sound_right_now_may_still_interrupt() {
        // A performer who is not told the right deck is silent finds out from
        // the room.
        let audible = Notice::ALL
            .iter()
            .find(|notice| notice.concerns_the_sound_right_now())
            .copied()
            .expect("at least one notice is about the sound");
        assert_eq!(
            Experience::concerns_the_sound(notice_code(audible)),
            Ok(true)
        );
    }

    #[test]
    fn releasing_with_nothing_held_is_empty_rather_than_an_error() {
        let mut experience = Experience::new();
        assert_eq!(experience.release(), 0);
        assert_eq!(experience.released(0), Err(Status::InvalidArgument));
    }

    #[test]
    fn every_code_round_trips_and_an_unknown_one_is_refused() {
        for (index, (mode, name)) in MODES.iter().enumerate() {
            let code = i32::try_from(index).expect("small");
            assert_eq!(mode_from_code(code), Some(*mode));
            assert_eq!(mode_code(*mode), code);
            assert!(name.starts_with("PRV_MODE_"));
        }
        for (index, (attention, _)) in ATTENTION.iter().enumerate() {
            assert_eq!(
                attention_from_code(i32::try_from(index).expect("small")),
                Some(*attention)
            );
        }
        for notice in Notice::ALL {
            assert_eq!(notice_from_code(notice_code(notice)), Some(notice));
        }

        let mut experience = Experience::new();
        assert_eq!(experience.set_mode(99), Err(Status::InvalidArgument));
        assert_eq!(experience.set_attention(99), Err(Status::InvalidArgument));
        assert_eq!(experience.raise(9_999), Err(Status::InvalidArgument));
        assert_eq!(experience.flag(9_999), Err(Status::InvalidArgument));
    }

    #[test]
    fn the_mode_survives_being_set_and_read_back() {
        let mut experience = Experience::new();
        for (index, _) in MODES.iter().enumerate() {
            let code = i32::try_from(index).expect("small");
            experience.set_mode(code).expect("a real mode");
            assert_eq!(experience.mode(), code);
        }
    }

    #[test]
    fn a_setting_that_does_not_take_a_flag_is_refused_rather_than_coerced() {
        // Coercing would store something the setting cannot mean, and the next
        // read would be a surprise.
        let mut experience = Experience::new();
        let refused = SettingKey::ALL
            .iter()
            .enumerate()
            .find(|(index, _)| {
                let code = i32::try_from(*index).unwrap_or(0);
                experience.set_flag(code, true).is_err()
            })
            .is_some();
        assert!(
            refused || SettingKey::ALL.is_empty(),
            "every setting accepted a flag, so the type check is not being made"
        );
    }
}
