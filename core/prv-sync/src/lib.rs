//! Synchronisation: where it is, and what has not arrived yet.
//!
//! # What this crate is, given that `prv-project` already merges
//!
//! The hard part of synchronising a project — deciding whether two edits were
//! made in sequence or in parallel, merging them, and reporting what genuinely
//! conflicts — is `prv-project`'s, and was built in Sprint 3. It is an algebra
//! over an append-only log with version vectors, and it does not care whether
//! anything is connected.
//!
//! What was missing is everything around it: whether there is a network, whether
//! the user wants one used, what has been authored here and not confirmed
//! anywhere else, and what to say about all of that. This crate is that, and it
//! is deliberately small — the interesting correctness was settled three sprints
//! before there was a network to have.
//!
//! # Two rules shape it
//!
//! **Offline is the normal case.** Not a fallback with a reduced feature set.
//! Editing works in every synchronisation state, permanently, and there is a
//! test that walks the whole machine to say so.
//!
//! **Nothing the user did is ever dropped.** The outbox refuses when it is full
//! rather than discarding its oldest entry — the opposite of what
//! `prv-security`'s audit log does with the same problem, for the reason
//! explained in [`outbox`]: one holds a record of what happened, the other holds
//! the work itself.

pub mod outbox;
pub mod state;

pub use outbox::{Outbox, OutboxError};
pub use state::{advance, SyncEvent, SyncState};

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::panic,
        reason = "a test that cannot build its own fixture should fail loudly"
    )]

    use super::*;
    use prv_project::{DeviceId, OperationId};
    use prv_security::{Consents, Purpose};

    fn op(sequence: u64) -> OperationId {
        OperationId::new(DeviceId::new(1), sequence)
    }

    #[test]
    fn six_hours_offline_then_a_reconnection() {
        // The session this crate is designed around, start to finish.
        let mut state = SyncState::Offline;
        let mut outbox = Outbox::new();

        // A long evening's editing, with no network and no complaint.
        for sequence in 1..=2_000 {
            assert!(state.editing_is_allowed());
            outbox.hold(op(sequence)).expect("held");
        }
        assert_eq!(state, SyncState::Offline);
        assert_eq!(outbox.len(), 2_000);
        assert!(!outbox.is_nearly_full());

        // The network comes back and everything travels.
        state = advance(state, SyncEvent::NetworkAvailable);
        state = advance(state, SyncEvent::WorkToSend);
        assert!(state.is_transferring());

        for sequence in 1..=2_000 {
            outbox.acknowledge(op(sequence));
        }
        state = advance(state, SyncEvent::TransferFinished);
        assert_eq!(state, SyncState::Idle);
        assert!(outbox.is_empty());
    }

    #[test]
    fn a_conflict_pauses_the_transfer_and_not_the_person() {
        let mut state = SyncState::Sending;
        state = advance(state, SyncEvent::ConflictFound);

        assert!(state.needs_the_user());
        assert!(!state.is_transferring());
        assert!(
            state.editing_is_allowed(),
            "an unanswered question stopped the user working"
        );

        // And the work done while the question is open still queues.
        let mut outbox = Outbox::new();
        outbox.hold(op(1)).expect("held");
        assert_eq!(outbox.len(), 1);

        state = advance(state, SyncEvent::ConflictResolved);
        assert_eq!(state, SyncState::Idle);
    }

    #[test]
    fn synchronising_is_a_purpose_the_user_agrees_to_like_any_other() {
        // This crate does not gate itself — `prv-security` holds the agreement
        // and the layer that opens a socket consults it. What is worth asserting
        // here is that the purpose exists and starts withheld, so nothing about
        // synchronisation is on by default.
        let consents = Consents::none();
        assert!(!consents.allows(Purpose::ProjectSync));
        assert!(Purpose::ProjectSync.sends_content());
    }
}
