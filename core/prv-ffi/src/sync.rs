//! Where synchronisation is, and what has not gone yet.
//!
//! # Why this crosses the boundary at all, when the bytes already do
//!
//! [`crate::engine`] gives a host the four calls that produce and consume
//! messages, and a host could in principle keep its own idea of whether it is
//! connected. It should not, and the reason is that "connected" is the smallest
//! part of what this state means.
//!
//! `prv-sync`'s machine holds rules with teeth: editing is permitted in *every*
//! state, permanently; a conflict stops the transfer and nothing else; a pause is
//! lifted only by the user and never by a network event. Those are product
//! promises from Master Prompt #24, and a host that modelled synchronisation
//! itself would reimplement them — differently, eventually.
//!
//! So the machine crosses, and the host reports events into it rather than
//! deciding what they mean.
//!
//! # A separate handle from the engine
//!
//! Synchronisation state belongs to an installation, not to a project. A user
//! with three projects open is not offline three times, and pausing
//! synchronisation pauses it for the application rather than for whichever
//! window happens to be focused.

use prv_project::{DeviceId, OperationId};
use prv_sync::{advance, Outbox, SyncEvent, SyncState};

use crate::status::Status;

/// The synchronisation state of one installation.
#[derive(Debug, Default)]
pub struct Sync {
    state: SyncState,
    outbox: Outbox,
}

impl Sync {
    /// A fresh installation: offline, with nothing waiting.
    ///
    /// Offline rather than idle, because that is the state a device starts in
    /// before anything has confirmed otherwise, and assuming the optimistic one
    /// would make the first seconds of every launch a lie.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Reports something that happened.
    ///
    /// Total: every state and event pair has an answer, and a pair that means
    /// nothing leaves the state alone rather than failing. Events arrive from a
    /// network and from a user at the same time, so "that cannot happen here" is
    /// a claim about timing that no amount of care makes true.
    ///
    /// # Errors
    ///
    /// [`Status::InvalidArgument`] for an event code this version does not
    /// define — refused rather than guessed at, because guessing here would move
    /// a user's synchronisation into a state nobody asked for.
    pub fn apply(&mut self, event_code: i32) -> Result<(), Status> {
        let event = event_from_code(event_code).ok_or(Status::InvalidArgument)?;
        self.state = advance(self.state, event);
        Ok(())
    }

    /// The current state, as an ABI code.
    #[must_use]
    pub fn state(&self) -> i32 {
        state_code(self.state)
    }

    /// Whether the user may go on editing.
    ///
    /// Always true, in every state, and exposed anyway. A host that has to ask
    /// is a host that was considering disabling something, and the answer it
    /// gets is the one Master Prompt #24 requires: offline is the normal case,
    /// not a mode with fewer features.
    #[must_use]
    pub const fn editing_is_allowed(&self) -> bool {
        self.state.editing_is_allowed()
    }

    /// Whether a transfer is in progress.
    #[must_use]
    pub const fn is_transferring(&self) -> bool {
        self.state.is_transferring()
    }

    /// Whether something is waiting on a person.
    #[must_use]
    pub const fn needs_the_user(&self) -> bool {
        self.state.needs_the_user()
    }

    /// Records that an operation was authored here and has gone nowhere yet.
    ///
    /// # Errors
    ///
    /// [`Status::Refused`] when the outbox is full. It refuses rather than
    /// discarding its oldest entry, which is the opposite of what an audit log
    /// does with the same problem: one holds a record of what happened, and this
    /// holds the work itself.
    pub fn hold(&mut self, device: u64, sequence: u64) -> Result<(), Status> {
        self.outbox
            .hold(OperationId::new(DeviceId::new(device), sequence))
            .map_err(|_| Status::Refused)
    }

    /// Records that an operation reached somewhere else.
    ///
    /// Acknowledging something already acknowledged does nothing, which is what
    /// makes a lost reply safe: the client sends again, and the second delivery
    /// is recognised rather than corrupting anything.
    pub fn acknowledge(&mut self, device: u64, sequence: u64) {
        self.outbox
            .acknowledge(OperationId::new(DeviceId::new(device), sequence));
    }

    /// How much work is waiting to leave.
    #[must_use]
    pub fn waiting(&self) -> u64 {
        self.outbox.len().try_into().unwrap_or(u64::MAX)
    }

    /// Whether the outbox is close enough to full to be worth mentioning.
    ///
    /// Reaching the bound means a session has been offline for a very long time
    /// or a server has been refusing everything, and the user needs to know
    /// either way — which is why this exists before the refusal rather than
    /// after it.
    #[must_use]
    pub fn is_nearly_full(&self) -> bool {
        self.outbox.is_nearly_full()
    }
}

/// Names a state as an ABI code.
///
/// Numbered from one so that zero is never a state: a host reading an
/// uninitialised value gets something it can recognise as wrong rather than
/// "offline", which is a plausible answer and therefore the dangerous one.
#[must_use]
pub const fn state_code(state: SyncState) -> i32 {
    match state {
        SyncState::Offline => 1,
        SyncState::Idle => 2,
        SyncState::Sending => 3,
        SyncState::Receiving => 4,
        SyncState::Conflicted => 5,
        SyncState::Paused => 6,
        // `SyncState` is `non_exhaustive`, so this arm cannot be removed. Zero
        // is the "no code" value rather than a plausible-looking state, and
        // `every_state_and_event_has_a_code_and_only_one` walks `SyncState::ALL`
        // to make an unmapped state a failing test rather than a silent lie on
        // somebody's screen.
        _ => 0,
    }
}

/// Turns an ABI code back into an event.
///
/// `None` for a code this version does not define.
#[must_use]
pub const fn event_from_code(code: i32) -> Option<SyncEvent> {
    match code {
        1 => Some(SyncEvent::NetworkAvailable),
        2 => Some(SyncEvent::NetworkLost),
        3 => Some(SyncEvent::WorkToSend),
        4 => Some(SyncEvent::WorkArrived),
        5 => Some(SyncEvent::TransferFinished),
        6 => Some(SyncEvent::ConflictFound),
        7 => Some(SyncEvent::ConflictResolved),
        8 => Some(SyncEvent::Pause),
        9 => Some(SyncEvent::Resume),
        _ => None,
    }
}

/// The ABI code for an event.
#[must_use]
pub const fn event_code(event: SyncEvent) -> i32 {
    match event {
        SyncEvent::NetworkAvailable => 1,
        SyncEvent::NetworkLost => 2,
        SyncEvent::WorkToSend => 3,
        SyncEvent::WorkArrived => 4,
        SyncEvent::TransferFinished => 5,
        SyncEvent::ConflictFound => 6,
        SyncEvent::ConflictResolved => 7,
        SyncEvent::Pause => 8,
        SyncEvent::Resume => 9,
        // As above, and walked by the same test through `SyncEvent::ALL`.
        _ => 0,
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
    fn every_state_and_event_has_a_code_and_only_one() {
        let mut codes = Vec::new();
        for state in SyncState::ALL {
            let code = state_code(state);
            assert!(code > 0, "{state:?} is numbered zero");
            assert!(!codes.contains(&code), "two states share the code {code}");
            codes.push(code);
        }

        let mut event_codes = Vec::new();
        for event in SyncEvent::ALL {
            let code = event_code(event);
            assert!(code > 0, "{event:?} is numbered zero");
            assert!(
                !event_codes.contains(&code),
                "two events share the code {code}"
            );
            event_codes.push(code);

            // Round trips, which is the property the boundary actually needs: a
            // host sends a number and the core must recover the event that
            // number was minted for.
            assert_eq!(event_from_code(event_code(event)), Some(event));
        }
        assert_eq!(event_from_code(0), None);
        assert_eq!(event_from_code(-1), None);
        assert_eq!(event_from_code(9_999), None);
    }

    #[test]
    fn a_fresh_installation_is_offline_rather_than_optimistic() {
        let sync = Sync::new();
        assert_eq!(sync.state(), state_code(SyncState::Offline));
        assert_eq!(sync.waiting(), 0);
        assert!(sync.editing_is_allowed());
    }

    #[test]
    fn editing_is_allowed_in_every_state_this_boundary_can_reach() {
        // The promise Master Prompt #24 makes, checked at the boundary rather
        // than only in the crate — because this is the layer a host asks, and a
        // host asks in order to decide whether to disable something.
        let mut sync = Sync::new();
        for code in 1..=9 {
            sync.apply(code).expect("a defined event");
            assert!(
                sync.editing_is_allowed(),
                "editing was refused after event {code}"
            );
        }
    }

    #[test]
    fn an_event_this_version_does_not_define_is_refused_rather_than_guessed_at() {
        let mut sync = Sync::new();
        assert_eq!(sync.apply(0), Err(Status::InvalidArgument));
        assert_eq!(sync.apply(9_999), Err(Status::InvalidArgument));
        assert_eq!(
            sync.state(),
            state_code(SyncState::Offline),
            "a refused event moved the state anyway"
        );
    }

    #[test]
    fn a_conflict_stops_the_transfer_and_nothing_else() {
        let mut sync = Sync::new();
        sync.apply(event_code(SyncEvent::NetworkAvailable))
            .expect("defined");
        sync.apply(event_code(SyncEvent::WorkToSend))
            .expect("defined");
        assert!(sync.is_transferring());

        sync.apply(event_code(SyncEvent::ConflictFound))
            .expect("defined");
        assert!(sync.needs_the_user());
        assert!(!sync.is_transferring());
        assert!(sync.editing_is_allowed());

        // And work done while the question is open still queues.
        sync.hold(1, 1).expect("held");
        assert_eq!(sync.waiting(), 1);
    }

    #[test]
    fn a_pause_is_lifted_by_the_user_and_by_nothing_else() {
        let mut sync = Sync::new();
        sync.apply(event_code(SyncEvent::Pause)).expect("defined");
        let paused = sync.state();

        for event in [
            SyncEvent::NetworkAvailable,
            SyncEvent::WorkToSend,
            SyncEvent::WorkArrived,
            SyncEvent::TransferFinished,
        ] {
            sync.apply(event_code(event)).expect("defined");
            assert_eq!(sync.state(), paused, "a network event lifted a pause");
        }

        sync.apply(event_code(SyncEvent::Resume)).expect("defined");
        assert_ne!(sync.state(), paused);
    }

    #[test]
    fn a_lost_reply_costs_nothing_because_acknowledging_twice_is_a_no_op() {
        let mut sync = Sync::new();
        sync.hold(1, 1).expect("held");
        sync.hold(1, 2).expect("held");
        assert_eq!(sync.waiting(), 2);

        sync.acknowledge(1, 1);
        sync.acknowledge(1, 1);
        assert_eq!(sync.waiting(), 1);

        // And acknowledging something never held is not an error either: a
        // server may confirm what a previous session sent.
        sync.acknowledge(9, 9);
        assert_eq!(sync.waiting(), 1);
    }

    #[test]
    fn an_evening_offline_fills_the_outbox_without_complaint() {
        let mut sync = Sync::new();
        for sequence in 1..=2_000 {
            sync.hold(1, sequence).expect("held");
            assert!(sync.editing_is_allowed());
        }
        assert_eq!(sync.waiting(), 2_000);
        assert!(!sync.is_nearly_full());
    }
}
