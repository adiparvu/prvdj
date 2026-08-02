//! What a plugin is doing, and what the audio graph does about it.
//!
//! # Every state has an audio behaviour, and none of them is "stop"
//!
//! ADR-0005's second success criterion is that a plugin which deliberately
//! crashes does not stop playback. The way that becomes true rather than
//! intended is [`LifecycleState::audio_behaviour`]: every state answers either
//! *processes* or *passes through*, and there is no third answer. A crash moves
//! a plugin from processing to passing through; it does not move the graph
//! anywhere.
//!
//! # Revocation is terminal
//!
//! Master Prompt #23 requires permissions to be revocable, and revocation that
//! can be undone by the thing being revoked is not revocation. [`Revoked`] has no
//! outgoing transition. Getting a revoked plugin back means installing it again,
//! which is a decision with a person in it.
//!
//! [`Revoked`]: LifecycleState::Revoked

use core::fmt;

/// What the graph does with a plugin's slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum AudioBehaviour {
    /// The plugin's output is used.
    Processes,
    /// The input is passed through unchanged, delayed by the plugin's declared
    /// latency so that bypassing does not move the music in time.
    PassesThrough,
}

impl AudioBehaviour {
    /// A stable identifier, for storage and localisation.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Processes => "audio.processes",
            Self::PassesThrough => "audio.passes_through",
        }
    }
}

/// Where a plugin is in its life.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum LifecycleState {
    /// Found, and nothing more. Its manifest has been read.
    Discovered,
    /// The user has approved its permissions. Not yet loaded.
    Approved,
    /// In memory, instantiated, not yet reachable from the callback.
    ///
    /// A real state rather than a step inside loading, because ADR-0005
    /// requires WebAssembly modules to be compiled and pre-instantiated
    /// *before* they become reachable from the audio thread. Instantiating one
    /// inside the callback would allocate, which the realtime contract forbids.
    Loaded,
    /// Processing.
    Running,
    /// Loaded and passed over.
    ///
    /// Where a plugin goes when it overruns its budget repeatedly or crashes.
    /// The music continues; the user is told.
    Bypassed,
    /// Switched off by the user, and still installed.
    Disabled,
    /// Withdrawn. Terminal.
    Revoked,
}

impl LifecycleState {
    /// Every state.
    pub const ALL: [Self; 7] = [
        Self::Discovered,
        Self::Approved,
        Self::Loaded,
        Self::Running,
        Self::Bypassed,
        Self::Disabled,
        Self::Revoked,
    ];

    /// What the graph does with the slot while a plugin is in this state.
    #[must_use]
    pub const fn audio_behaviour(self) -> AudioBehaviour {
        match self {
            Self::Running => AudioBehaviour::Processes,
            Self::Discovered
            | Self::Approved
            | Self::Loaded
            | Self::Bypassed
            | Self::Disabled
            | Self::Revoked => AudioBehaviour::PassesThrough,
        }
    }

    /// Whether anything can happen to a plugin in this state.
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Revoked)
    }

    /// A stable identifier, for storage and localisation.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Discovered => "lifecycle.discovered",
            Self::Approved => "lifecycle.approved",
            Self::Loaded => "lifecycle.loaded",
            Self::Running => "lifecycle.running",
            Self::Bypassed => "lifecycle.bypassed",
            Self::Disabled => "lifecycle.disabled",
            Self::Revoked => "lifecycle.revoked",
        }
    }
}

impl fmt::Display for LifecycleState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.key())
    }
}

/// Something that happens to a plugin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum LifecycleEvent {
    /// The user approved its permissions.
    Approve,
    /// It was compiled and instantiated.
    Load,
    /// It became reachable from the callback.
    Start,
    /// It exceeded its budget often enough to be passed over.
    Overran,
    /// It stopped working — a trap, a panic, a dead child process.
    Crashed,
    /// The supervisor restarted it and it is ready to try again.
    Recover,
    /// It became unreachable from the callback.
    Stop,
    /// The user switched it off.
    Disable,
    /// The user switched it back on.
    Enable,
    /// The user removed it, or its signature was withdrawn.
    Revoke,
}

impl LifecycleEvent {
    /// A stable identifier, for storage and localisation.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Approve => "event.approve",
            Self::Load => "event.load",
            Self::Start => "event.start",
            Self::Overran => "event.overran",
            Self::Crashed => "event.crashed",
            Self::Recover => "event.recover",
            Self::Stop => "event.stop",
            Self::Disable => "event.disable",
            Self::Enable => "event.enable",
            Self::Revoke => "event.revoke",
        }
    }
}

/// Why an event did not apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum LifecycleError {
    /// The event means nothing in this state.
    NotApplicable {
        /// Where it was.
        state: LifecycleState,
        /// What happened.
        event: LifecycleEvent,
    },
    /// The plugin is revoked, and revocation is not undone by an event.
    Revoked,
}

impl fmt::Display for LifecycleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::NotApplicable { state, event } => {
                write!(f, "{} does not apply in {state}", event.key())
            }
            Self::Revoked => f.write_str("the plugin is revoked"),
        }
    }
}

impl core::error::Error for LifecycleError {}

/// Applies an event to a state.
///
/// # Errors
///
/// Returns [`LifecycleError`] if the event means nothing where the plugin is, or
/// if the plugin is revoked.
pub fn advance(
    state: LifecycleState,
    event: LifecycleEvent,
) -> Result<LifecycleState, LifecycleError> {
    use LifecycleEvent as E;
    use LifecycleState as S;

    if state.is_terminal() {
        return Err(LifecycleError::Revoked);
    }

    // Revocation applies wherever the plugin is. A plugin that had to be
    // stopped before it could be revoked would be one whose removal a wedged
    // instance could refuse.
    if event == E::Revoke {
        return Ok(S::Revoked);
    }

    let next = match (state, event) {
        (S::Discovered, E::Approve) => S::Approved,

        // The two that matter. Neither stops the graph; both leave the slot
        // passing through, and the user is told.
        (S::Running, E::Overran | E::Crashed) => S::Bypassed,

        // Loaded is the resting state: instantiated, and not yet reachable from
        // the callback. Four different journeys end there — first load, a
        // supervisor restart, an ordinary stop, and being switched back on —
        // and they converge deliberately. Recovery is not a special path; it is
        // the ordinary path taken again, which is why a recovered plugin has to
        // be started like any other rather than resuming mid-block.
        (S::Approved, E::Load)
        | (S::Bypassed, E::Recover)
        | (S::Running, E::Stop)
        | (S::Disabled, E::Enable) => S::Loaded,

        (S::Loaded, E::Start) => S::Running,

        (S::Discovered | S::Approved | S::Loaded | S::Running | S::Bypassed, E::Disable) => {
            S::Disabled
        }

        _ => return Err(LifecycleError::NotApplicable { state, event }),
    };
    Ok(next)
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::panic,
        reason = "a test that cannot build its own fixture should fail loudly"
    )]

    use super::*;

    const EVENTS: [LifecycleEvent; 10] = [
        LifecycleEvent::Approve,
        LifecycleEvent::Load,
        LifecycleEvent::Start,
        LifecycleEvent::Overran,
        LifecycleEvent::Crashed,
        LifecycleEvent::Recover,
        LifecycleEvent::Stop,
        LifecycleEvent::Disable,
        LifecycleEvent::Enable,
        LifecycleEvent::Revoke,
    ];

    #[test]
    fn nothing_that_can_happen_to_a_plugin_stops_the_graph() {
        // ADR-0005's second success criterion, as a property over the whole
        // machine rather than a test of one path. Every state answers either
        // "processes" or "passes through"; there is no third answer, so there
        // is no event whose outcome is silence.
        for state in LifecycleState::ALL {
            for event in EVENTS {
                let Ok(next) = advance(state, event) else {
                    continue;
                };
                let behaviour = next.audio_behaviour();
                assert!(
                    behaviour == AudioBehaviour::Processes
                        || behaviour == AudioBehaviour::PassesThrough,
                    "{state} + {} left the graph nowhere",
                    event.key()
                );
            }
        }
    }

    #[test]
    fn a_crash_and_an_overrun_both_land_on_bypassed() {
        // The music continues. The plugin is passed over, not removed, and the
        // supervisor may bring it back.
        for event in [LifecycleEvent::Crashed, LifecycleEvent::Overran] {
            assert_eq!(
                advance(LifecycleState::Running, event),
                Ok(LifecycleState::Bypassed)
            );
        }
        assert_eq!(
            LifecycleState::Bypassed.audio_behaviour(),
            AudioBehaviour::PassesThrough
        );
        assert_eq!(
            advance(LifecycleState::Bypassed, LifecycleEvent::Recover),
            Ok(LifecycleState::Loaded)
        );
    }

    #[test]
    fn only_running_processes() {
        // If any other state processed, a plugin could reach the callback
        // before it was instantiated — which is the allocation ADR-0005 exists
        // to keep off the audio thread.
        for state in LifecycleState::ALL {
            assert_eq!(
                state.audio_behaviour() == AudioBehaviour::Processes,
                state == LifecycleState::Running,
                "{state} disagrees about whether it processes"
            );
        }
    }

    #[test]
    fn revocation_applies_everywhere_and_is_final() {
        // Revocation that can be undone by the thing being revoked is not
        // revocation, and one that a wedged instance could refuse is worse.
        for state in LifecycleState::ALL {
            if state.is_terminal() {
                continue;
            }
            assert_eq!(
                advance(state, LifecycleEvent::Revoke),
                Ok(LifecycleState::Revoked),
                "{state} could not be revoked"
            );
        }

        for event in EVENTS {
            assert_eq!(
                advance(LifecycleState::Revoked, event),
                Err(LifecycleError::Revoked),
                "{} reached a revoked plugin",
                event.key()
            );
        }
    }

    #[test]
    fn a_plugin_cannot_reach_the_callback_without_being_approved_and_loaded() {
        // The ordering that makes pre-instantiation real rather than intended.
        assert!(advance(LifecycleState::Discovered, LifecycleEvent::Start).is_err());
        assert!(advance(LifecycleState::Approved, LifecycleEvent::Start).is_err());

        let approved =
            advance(LifecycleState::Discovered, LifecycleEvent::Approve).expect("approval");
        let loaded = advance(approved, LifecycleEvent::Load).expect("loading");
        assert_eq!(
            advance(loaded, LifecycleEvent::Start),
            Ok(LifecycleState::Running)
        );
    }

    #[test]
    fn switching_a_plugin_off_works_from_wherever_it_is() {
        // A user turning something off should not have to know what it is
        // doing at that moment.
        for state in LifecycleState::ALL {
            if state.is_terminal() || state == LifecycleState::Disabled {
                continue;
            }
            assert_eq!(
                advance(state, LifecycleEvent::Disable),
                Ok(LifecycleState::Disabled),
                "{state} could not be switched off"
            );
        }
        assert_eq!(
            advance(LifecycleState::Disabled, LifecycleEvent::Enable),
            Ok(LifecycleState::Loaded),
            "re-enabling should return it to loaded, not to running"
        );
    }

    #[test]
    fn an_event_that_means_nothing_says_so_rather_than_being_ignored() {
        assert_eq!(
            advance(LifecycleState::Running, LifecycleEvent::Load),
            Err(LifecycleError::NotApplicable {
                state: LifecycleState::Running,
                event: LifecycleEvent::Load,
            })
        );
        assert_eq!(
            advance(LifecycleState::Loaded, LifecycleEvent::Recover),
            Err(LifecycleError::NotApplicable {
                state: LifecycleState::Loaded,
                event: LifecycleEvent::Recover,
            })
        );
    }

    #[test]
    fn state_and_event_keys_are_distinct() {
        let states: Vec<&str> = LifecycleState::ALL.iter().map(|s| s.key()).collect();
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

        assert_ne!(
            AudioBehaviour::Processes.key(),
            AudioBehaviour::PassesThrough.key()
        );
    }
}
