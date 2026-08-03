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
//!
//! The same rule is why [`backup`] lives here rather than in `prv-project`.
//! Thinning restore points is the one place in the system that deliberately
//! discards something, so it belongs beside the module whose whole argument is
//! about not discarding — where the exception has to be justified in front of
//! the rule.

pub mod backup;
pub mod outbox;
pub mod state;

pub use backup::{thin, PointKind, RestorePoint, RetentionPolicy};
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
    use prv_project::{
        wire, DeviceId, MarkerId, OperationId, OperationLog, OperationPayload, PlacementId,
        VersionVector,
    };
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
    fn two_laptops_a_night_apart_and_only_bytes_between_them() {
        // The exchange the whole crate exists to make possible, with nothing
        // simulated except the network. `prv-project` decides what merges and
        // what conflicts; `prv-project::wire` turns operations into bytes; this
        // crate decides when to send and what is still owed. The test is here
        // because it is the only place all three meet.
        let studio = DeviceId::new(1);
        let laptop = DeviceId::new(2);

        // A project that already exists on both machines.
        let mut here = OperationLog::new();
        let shared = here.author(
            studio,
            1_000,
            OperationPayload::SetProjectName {
                name: "Saturday".to_owned(),
            },
        );
        here.append(shared).expect("appends");
        let mut there = here.clone();

        // The evening: edits on one machine, with no network and nothing lost.
        let mut state = SyncState::Offline;
        let mut outbox = Outbox::new();
        for index in 0..64_u64 {
            let payload = OperationPayload::RemovePlacement {
                placement: PlacementId::new(index),
            };
            let operation = here.author(
                studio,
                2_000 + i64::try_from(index).expect("small"),
                payload,
            );
            let id = operation.id;
            here.append(operation).expect("appends");
            outbox.hold(id).expect("held");
            assert!(state.editing_is_allowed());
        }

        // Meanwhile the other machine is edited too, by someone who has not seen
        // any of that. Concurrent, and on a different entity, so it merges
        // silently — the case that must never interrupt anyone.
        let elsewhere = there.author(
            laptop,
            2_500,
            OperationPayload::RemoveMarker {
                marker: MarkerId::new(1),
            },
        );
        there.append(elsewhere).expect("appends");

        // Morning. Each side sends what the other has not seen — which is what
        // the version vector is for, and why synchronisation is incremental
        // rather than a full exchange.
        state = advance(state, SyncEvent::NetworkAvailable);
        state = advance(state, SyncEvent::WorkToSend);
        assert!(state.is_transferring());

        let outbound =
            wire::encode(&here.operations_since(there.version_vector())).expect("encodes");
        let inbound =
            wire::encode(&there.operations_since(here.version_vector())).expect("encodes");
        assert!(outbound.len() < 8_192, "64 edits should not cost a page");

        let received = wire::decode(&outbound).expect("decodes");
        assert_eq!(received.unrecognised_count(), 0);
        let report = there.merge(&received.to_operations()).expect("merges");
        assert_eq!(report.applied, 64);
        assert!(!report.needs_review(), "{report}");

        let returning = wire::decode(&inbound).expect("decodes");
        let report = here.merge(&returning.to_operations()).expect("merges");
        assert_eq!(report.applied, 1);
        assert!(!report.needs_review(), "{report}");

        // Both machines now hold the same project, and neither had to be told
        // which one was authoritative.
        assert_eq!(here.state(), there.state());
        assert_eq!(here.len(), there.len());

        // And the work is no longer owed.
        outbox.acknowledge_through(there.version_vector());
        assert!(outbox.is_empty());
        state = advance(state, SyncEvent::TransferFinished);
        assert_eq!(state, SyncState::Idle);
    }

    #[test]
    fn sending_the_same_bytes_twice_costs_nothing() {
        // A network that loses a reply makes a client send again. Operations
        // carry identities allocated without coordination, so the second
        // delivery is recognised rather than duplicated — the property the
        // outbox already relies on, checked here through the wire.
        let device = DeviceId::new(1);
        let mut here = OperationLog::new();
        let operation = here.author(
            device,
            1_000,
            OperationPayload::RemoveMarker {
                marker: MarkerId::new(4),
            },
        );
        here.append(operation).expect("appends");

        let bytes = wire::encode(&here.operations_since(&VersionVector::new())).expect("encodes");
        let mut there = OperationLog::new();

        let first = there
            .merge(&wire::decode(&bytes).expect("decodes").to_operations())
            .expect("merges");
        let second = there
            .merge(&wire::decode(&bytes).expect("decodes").to_operations())
            .expect("merges");

        assert_eq!(first.applied, 1);
        assert_eq!(second.applied, 0);
        assert_eq!(second.already_present, 1);
        assert_eq!(there.len(), 1);
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
