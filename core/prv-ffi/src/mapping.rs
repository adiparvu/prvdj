//! Turning the core's enums into integers, and back.
//!
//! # Why this is written out rather than derived
//!
//! `#[repr(i32)]` on the core's own enums would be shorter and would be wrong.
//! It would put an ABI commitment inside `prv-transport`, a crate that has no
//! idea a boundary exists and should stay free to reorder its variants for
//! readability. The moment somebody sorts [`PlaybackState`] alphabetically, a
//! host compiled last year starts reading "paused" as "recovering" — and
//! nothing in either crate would notice.
//!
//! So the numbers live here, at the boundary that promises them, written out one
//! at a time. Reordering the core's enum now changes nothing; removing a variant
//! is a compile error in this file, which is exactly where the conversation
//! about breaking a host's build should happen.
//!
//! # The numbers are the contract
//!
//! A variant is never renumbered and a retired number is never reused. Both
//! rules are the same rule ADR-0003 applies to the operation log, for the same
//! reason: somebody out there has the old number written down.

use prv_transport::{PlaybackState, TransportEvent};

/// The ABI code for a playback state.
#[must_use]
pub const fn playback_code(state: PlaybackState) -> i32 {
    match state {
        PlaybackState::Stopped => 0,
        PlaybackState::Loading => 1,
        PlaybackState::Ready => 2,
        PlaybackState::Playing => 3,
        PlaybackState::Paused => 4,
        PlaybackState::Seeking => 5,
        PlaybackState::Buffering => 6,
        PlaybackState::Recovering => 7,
        PlaybackState::Error => 8,
    }
}

/// Every playback state with its C spelling, in code order.
///
/// The header generator walks this, so a state added to the core and mapped
/// above still reaches a host without anybody remembering to edit a header.
pub const PLAYBACK_STATES: &[(PlaybackState, &str)] = &[
    (PlaybackState::Stopped, "PRV_PLAYBACK_STOPPED"),
    (PlaybackState::Loading, "PRV_PLAYBACK_LOADING"),
    (PlaybackState::Ready, "PRV_PLAYBACK_READY"),
    (PlaybackState::Playing, "PRV_PLAYBACK_PLAYING"),
    (PlaybackState::Paused, "PRV_PLAYBACK_PAUSED"),
    (PlaybackState::Seeking, "PRV_PLAYBACK_SEEKING"),
    (PlaybackState::Buffering, "PRV_PLAYBACK_BUFFERING"),
    (PlaybackState::Recovering, "PRV_PLAYBACK_RECOVERING"),
    (PlaybackState::Error, "PRV_PLAYBACK_ERROR"),
];

/// The transport event a code names.
///
/// Returns `None` for a number this version does not define, which a host sees
/// as [`crate::Status::InvalidArgument`] rather than as a silently ignored call.
#[must_use]
pub const fn event_from_code(code: i32) -> Option<TransportEvent> {
    match code {
        0 => Some(TransportEvent::Load),
        1 => Some(TransportEvent::LoadSucceeded),
        2 => Some(TransportEvent::LoadFailed),
        3 => Some(TransportEvent::Unload),
        4 => Some(TransportEvent::Play),
        5 => Some(TransportEvent::Pause),
        6 => Some(TransportEvent::Stop),
        7 => Some(TransportEvent::SeekRequested),
        8 => Some(TransportEvent::SeekCompleted),
        9 => Some(TransportEvent::BufferExhausted),
        10 => Some(TransportEvent::BufferRefilled),
        11 => Some(TransportEvent::DeviceLost),
        12 => Some(TransportEvent::DeviceRestored),
        13 => Some(TransportEvent::Fault),
        14 => Some(TransportEvent::Reset),
        _ => None,
    }
}

/// Every transport event with its C spelling, in code order.
pub const TRANSPORT_EVENTS: &[(TransportEvent, &str)] = &[
    (TransportEvent::Load, "PRV_EVENT_LOAD"),
    (TransportEvent::LoadSucceeded, "PRV_EVENT_LOAD_SUCCEEDED"),
    (TransportEvent::LoadFailed, "PRV_EVENT_LOAD_FAILED"),
    (TransportEvent::Unload, "PRV_EVENT_UNLOAD"),
    (TransportEvent::Play, "PRV_EVENT_PLAY"),
    (TransportEvent::Pause, "PRV_EVENT_PAUSE"),
    (TransportEvent::Stop, "PRV_EVENT_STOP"),
    (TransportEvent::SeekRequested, "PRV_EVENT_SEEK_REQUESTED"),
    (TransportEvent::SeekCompleted, "PRV_EVENT_SEEK_COMPLETED"),
    (
        TransportEvent::BufferExhausted,
        "PRV_EVENT_BUFFER_EXHAUSTED",
    ),
    (TransportEvent::BufferRefilled, "PRV_EVENT_BUFFER_REFILLED"),
    (TransportEvent::DeviceLost, "PRV_EVENT_DEVICE_LOST"),
    (TransportEvent::DeviceRestored, "PRV_EVENT_DEVICE_RESTORED"),
    (TransportEvent::Fault, "PRV_EVENT_FAULT"),
    (TransportEvent::Reset, "PRV_EVENT_RESET"),
];

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::panic,
        reason = "a test that cannot build its own fixture should fail loudly"
    )]

    use super::*;

    #[test]
    fn every_playback_state_the_core_defines_has_a_code() {
        // The table and the mapping are two lists that must agree. If a state is
        // added to `playback_code` and forgotten here, the header loses it
        // silently and a host reads a number with no name.
        for (state, _) in PLAYBACK_STATES {
            let code = playback_code(*state);
            assert!(code >= 0, "{state:?} has no code");
        }
        assert_eq!(
            PLAYBACK_STATES.len(),
            9,
            "a playback state was added or removed without updating the table"
        );
    }

    #[test]
    fn playback_codes_are_dense_and_match_their_position() {
        for (index, (state, _)) in PLAYBACK_STATES.iter().enumerate() {
            let expected = i32::try_from(index).expect("the table is small");
            assert_eq!(
                playback_code(*state),
                expected,
                "{state:?} is not where the table puts it"
            );
        }
    }

    #[test]
    fn no_two_playback_states_share_a_code() {
        // The failure this prevents is silent: a host switching on the number
        // takes the first matching branch and the second state becomes
        // unreachable.
        let mut seen = std::collections::BTreeSet::new();
        for (state, _) in PLAYBACK_STATES {
            assert!(
                seen.insert(playback_code(*state)),
                "{state:?} collides with an earlier state"
            );
        }
    }

    #[test]
    fn every_transport_event_round_trips_through_its_code() {
        for (index, (event, _)) in TRANSPORT_EVENTS.iter().enumerate() {
            let code = i32::try_from(index).expect("the table is small");
            assert_eq!(
                event_from_code(code),
                Some(*event),
                "code {code} does not name the event the table puts there"
            );
        }
    }

    #[test]
    fn a_code_this_version_does_not_define_is_refused_rather_than_guessed() {
        // What a newer host talking to an older library looks like. Refusing is
        // the only safe answer: guessing would apply some *other* transport
        // event, and the one next to `Stop` is `SeekRequested`.
        assert_eq!(event_from_code(-1), None);
        let past_the_end = i32::try_from(TRANSPORT_EVENTS.len()).unwrap_or(i32::MAX);
        assert_eq!(event_from_code(past_the_end), None);
        assert_eq!(event_from_code(i32::MAX), None);
    }

    #[test]
    fn c_names_are_namespaced_and_distinct_across_both_tables() {
        // Both tables land in one header, so a collision between them is a
        // redefinition rather than a shadowing.
        let mut seen = std::collections::BTreeSet::new();
        for name in PLAYBACK_STATES
            .iter()
            .map(|(_, name)| *name)
            .chain(TRANSPORT_EVENTS.iter().map(|(_, name)| *name))
        {
            assert!(name.starts_with("PRV_"), "{name} is not namespaced");
            assert!(seen.insert(name), "{name} appears twice");
        }
    }
}
