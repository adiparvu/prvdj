//! Where synchronisation is, and what it is allowed to stop.
//!
//! # Offline is the normal case
//!
//! Master Prompt #24 requires editing to work with no network. That is not a
//! fallback mode with a reduced feature set — it is the state a laptop in a
//! basement is in for six hours, and it is the state every design decision here
//! is taken from. There is no state in which the user cannot edit, and there is
//! a test that says so over the whole machine.
//!
//! # A conflict does not stop anything
//!
//! [`SyncState::Conflicted`] means two devices changed the same thing and
//! somebody has to choose. It does not mean the project is locked, the edits are
//! held, or the outbox stops. `prv-project` already refuses to discard either
//! side; this module refuses to let the *question* become a blockage.
//!
//! # Pausing is the user's, and it is honoured exactly
//!
//! Somebody on a metered connection who paused synchronisation gets no
//! background transfer, no "just this small one", and no automatic resumption
//! when the network improves. The pause is a decision, not a hint.

use core::fmt;

/// Where synchronisation is.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SyncState {
    /// No network worth trying.
    ///
    /// The default, because it is the state a device starts in before anything
    /// has confirmed otherwise, and assuming the optimistic one would make the
    /// first seconds of every launch a lie.
    #[default]
    Offline,
    /// Connected, with nothing to do.
    Idle,
    /// Sending what was authored here.
    Sending,
    /// Receiving what was authored elsewhere.
    Receiving,
    /// Two devices changed the same thing and somebody has to choose.
    Conflicted,
    /// The user switched it off.
    Paused,
}

impl SyncState {
    /// Every state.
    pub const ALL: [Self; 6] = [
        Self::Offline,
        Self::Idle,
        Self::Sending,
        Self::Receiving,
        Self::Conflicted,
        Self::Paused,
    ];

    /// Whether the user may go on editing.
    ///
    /// True in every state, permanently. Master Prompt #24 requires editing to
    /// work with no network, and a synchronisation state that could withhold it
    /// would make the network a dependency of the product rather than a feature
    /// of it.
    #[must_use]
    pub const fn editing_is_allowed(self) -> bool {
        true
    }

    /// Whether anything is being transferred right now.
    #[must_use]
    pub const fn is_transferring(self) -> bool {
        matches!(self, Self::Sending | Self::Receiving)
    }

    /// Whether the user has to decide something before this clears.
    #[must_use]
    pub const fn needs_the_user(self) -> bool {
        matches!(self, Self::Conflicted)
    }

    /// A stable identifier, for storage and localisation.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Offline => "sync.offline",
            Self::Idle => "sync.idle",
            Self::Sending => "sync.sending",
            Self::Receiving => "sync.receiving",
            Self::Conflicted => "sync.conflicted",
            Self::Paused => "sync.paused",
        }
    }
}

impl fmt::Display for SyncState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.key())
    }
}

/// Something that happens to synchronisation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SyncEvent {
    /// A usable network appeared.
    NetworkAvailable,
    /// It went away.
    NetworkLost,
    /// There is something to send.
    WorkToSend,
    /// Something arrived to apply.
    WorkArrived,
    /// The transfer finished.
    TransferFinished,
    /// Merging found something two devices both changed.
    ConflictFound,
    /// The user chose.
    ConflictResolved,
    /// The user switched synchronisation off.
    Pause,
    /// The user switched it back on.
    Resume,
}

impl SyncEvent {
    /// A stable identifier, for storage and localisation.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::NetworkAvailable => "sync.event.network_available",
            Self::NetworkLost => "sync.event.network_lost",
            Self::WorkToSend => "sync.event.work_to_send",
            Self::WorkArrived => "sync.event.work_arrived",
            Self::TransferFinished => "sync.event.transfer_finished",
            Self::ConflictFound => "sync.event.conflict_found",
            Self::ConflictResolved => "sync.event.conflict_resolved",
            Self::Pause => "sync.event.pause",
            Self::Resume => "sync.event.resume",
        }
    }
}

/// Applies an event.
///
/// Total: every state and event pair has an answer, and a pair that means
/// nothing leaves the state alone rather than failing. Synchronisation events
/// arrive from a network and from a user at the same time, so "that cannot
/// happen here" is a claim about timing that no amount of care makes true.
#[must_use]
pub fn advance(state: SyncState, event: SyncEvent) -> SyncState {
    use SyncEvent as E;
    use SyncState as S;

    // Written as precedence rather than as a table, because the interesting
    // content *is* the precedence: what outranks what when two things are true
    // at once. A table hides that behind arm ordering.

    // The user's decision outranks everything, including a transfer in flight.
    // Somebody who pauses on a metered connection meant now, not after this one.
    if event == E::Pause {
        return S::Paused;
    }
    // And nothing but the user lifts it. Not a better network, not new work,
    // not a resolved conflict.
    if state == S::Paused {
        return if event == E::Resume {
            S::Idle
        } else {
            S::Paused
        };
    }

    // No network means no transfer, whatever else was happening.
    if event == E::NetworkLost {
        return S::Offline;
    }

    // An unanswered question outranks a transfer. Carrying on sending while it
    // is open is how the question stops being visible.
    if event == E::ConflictFound {
        return S::Conflicted;
    }
    if state == S::Conflicted {
        return if event == E::ConflictResolved {
            S::Idle
        } else {
            S::Conflicted
        };
    }

    // Offline is not an error and not a mode to escape from. Only a network
    // changes it.
    if state == S::Offline {
        return if event == E::NetworkAvailable {
            S::Idle
        } else {
            S::Offline
        };
    }

    // Connected, unpaused, nothing unanswered.
    match event {
        E::WorkToSend => S::Sending,
        E::WorkArrived => S::Receiving,
        E::TransferFinished => S::Idle,
        // Either already answered above, or saying nothing new here: a network
        // that is already present, a conflict that was never open, a resume
        // that was never paused. Written out rather than left to a wildcard so
        // that an event added later has to be considered.
        E::NetworkAvailable
        | E::ConflictResolved
        | E::Resume
        | E::Pause
        | E::NetworkLost
        | E::ConflictFound => state,
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

    const EVENTS: [SyncEvent; 9] = [
        SyncEvent::NetworkAvailable,
        SyncEvent::NetworkLost,
        SyncEvent::WorkToSend,
        SyncEvent::WorkArrived,
        SyncEvent::TransferFinished,
        SyncEvent::ConflictFound,
        SyncEvent::ConflictResolved,
        SyncEvent::Pause,
        SyncEvent::Resume,
    ];

    #[test]
    fn there_is_no_state_in_which_the_user_cannot_edit() {
        // Master Prompt #24. A synchronisation state that could withhold
        // editing would make the network a dependency of the product rather
        // than a feature of it.
        for state in SyncState::ALL {
            assert!(state.editing_is_allowed(), "{state} withheld editing");
            for event in EVENTS {
                assert!(advance(state, event).editing_is_allowed());
            }
        }
    }

    #[test]
    fn a_pause_is_honoured_exactly_and_only_the_user_lifts_it() {
        // Somebody on a metered connection who paused meant now, and did not
        // mean "until the network improves".
        for state in SyncState::ALL {
            assert_eq!(
                advance(state, SyncEvent::Pause),
                SyncState::Paused,
                "{state} ignored a pause"
            );
        }

        for event in EVENTS {
            if event == SyncEvent::Resume {
                continue;
            }
            assert_eq!(
                advance(SyncState::Paused, event),
                SyncState::Paused,
                "{} lifted a pause the user did not lift",
                event.key()
            );
        }
        assert_eq!(
            advance(SyncState::Paused, SyncEvent::Resume),
            SyncState::Idle
        );
    }

    #[test]
    fn a_conflict_stops_transfer_and_nothing_else() {
        // Carrying on sending while a question is unanswered is how the
        // question stops being visible.
        assert_eq!(
            advance(SyncState::Sending, SyncEvent::ConflictFound),
            SyncState::Conflicted
        );
        for event in [SyncEvent::WorkToSend, SyncEvent::WorkArrived] {
            assert_eq!(
                advance(SyncState::Conflicted, event),
                SyncState::Conflicted,
                "{} transferred through an unanswered conflict",
                event.key()
            );
        }
        assert!(SyncState::Conflicted.needs_the_user());
        assert!(SyncState::Conflicted.editing_is_allowed());
        assert_eq!(
            advance(SyncState::Conflicted, SyncEvent::ConflictResolved),
            SyncState::Idle
        );
    }

    #[test]
    fn losing_the_network_is_never_an_error_state() {
        // It is the normal case. A laptop in a basement is here for six hours.
        for state in SyncState::ALL {
            if state == SyncState::Paused {
                continue;
            }
            assert_eq!(
                advance(state, SyncEvent::NetworkLost),
                SyncState::Offline,
                "{state} did not go offline"
            );
        }
        assert!(!SyncState::Offline.needs_the_user());
        assert_eq!(SyncState::default(), SyncState::Offline);
    }

    #[test]
    fn nothing_transfers_while_there_is_no_network() {
        for event in EVENTS {
            let next = advance(SyncState::Offline, event);
            if matches!(event, SyncEvent::NetworkAvailable | SyncEvent::Pause) {
                continue;
            }
            if event == SyncEvent::ConflictFound {
                // A conflict found while offline is a real thing: it comes from
                // merging a file that arrived by another route.
                assert_eq!(next, SyncState::Conflicted);
                continue;
            }
            assert!(
                !next.is_transferring(),
                "{} started a transfer with no network",
                event.key()
            );
        }
    }

    #[test]
    fn every_pair_has_an_answer() {
        // Synchronisation events arrive from a network and from a user at the
        // same time, so "that cannot happen here" is a claim about timing that
        // no amount of care makes true.
        for state in SyncState::ALL {
            for event in EVENTS {
                let next = advance(state, event);
                assert!(SyncState::ALL.contains(&next));
            }
        }
    }

    #[test]
    fn a_finished_transfer_returns_to_idle() {
        assert_eq!(
            advance(SyncState::Sending, SyncEvent::TransferFinished),
            SyncState::Idle
        );
        assert_eq!(
            advance(SyncState::Receiving, SyncEvent::TransferFinished),
            SyncState::Idle
        );
        assert!(SyncState::Sending.is_transferring());
        assert!(!SyncState::Idle.is_transferring());
    }

    #[test]
    fn state_and_event_keys_are_distinct() {
        let states: Vec<&str> = SyncState::ALL.iter().map(|s| s.key()).collect();
        for (index, key) in states.iter().enumerate() {
            for (other, value) in states.iter().enumerate() {
                assert!(index == other || key != value, "two states share {key}");
            }
        }

        let events: Vec<&str> = EVENTS.iter().map(|e| e.key()).collect();
        for (index, key) in events.iter().enumerate() {
            for (other, value) in events.iter().enumerate() {
                assert!(index == other || key != value, "two events share {key}");
            }
        }
    }
}
