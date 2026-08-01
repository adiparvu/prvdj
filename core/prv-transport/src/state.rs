use core::fmt;

/// What the engine is currently doing.
///
/// The nine states of Module Specification #002. Every one describes an
/// observable condition of the engine; none is a flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PlaybackState {
    /// No track is loaded, or playback has been stopped and the position reset.
    Stopped,
    /// A track is being opened and its first buffers filled.
    Loading,
    /// A track is loaded and cued, waiting to play.
    Ready,
    /// Audio is being rendered.
    Playing,
    /// Playback is suspended at its current position.
    Paused,
    /// The position is moving and buffers are being refilled at the destination.
    Seeking,
    /// Playback has run ahead of the decoder and is waiting for audio.
    ///
    /// Distinct from [`Self::Paused`]: the transport intends to continue and
    /// will do so without user action. A performer must be able to tell the two
    /// apart at a glance, which is why they are different states rather than one
    /// state with a reason attached.
    Buffering,
    /// The output device changed or was lost, and the chain is being rebuilt.
    ///
    /// Module Specification #002 requires recovery from a device change without
    /// stopping playback. This state is what "without stopping" looks like from
    /// the outside: the transport has not stopped, it is repairing itself.
    Recovering,
    /// An unrecoverable fault. Requires an explicit reset.
    Error,
}

impl PlaybackState {
    /// Returns `true` if audio is being rendered.
    #[must_use]
    pub const fn is_rendering(self) -> bool {
        matches!(self, Self::Playing)
    }

    /// Returns `true` if the transport is working toward playing again without
    /// needing the user to intervene.
    ///
    /// True while seeking, buffering or recovering. The interface uses this to
    /// show activity rather than a stopped transport, so a performer is not
    /// misled into thinking the deck has died.
    #[must_use]
    pub const fn is_transient(self) -> bool {
        matches!(self, Self::Seeking | Self::Buffering | Self::Recovering)
    }

    /// Returns `true` if a track is loaded.
    #[must_use]
    pub const fn has_track(self) -> bool {
        !matches!(self, Self::Stopped | Self::Loading | Self::Error)
    }
}

impl fmt::Display for PlaybackState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Stopped => "stopped",
            Self::Loading => "loading",
            Self::Ready => "ready",
            Self::Playing => "playing",
            Self::Paused => "paused",
            Self::Seeking => "seeking",
            Self::Buffering => "buffering",
            Self::Recovering => "recovering",
            Self::Error => "error",
        })
    }
}

/// What the user asked for.
///
/// Kept separate from [`PlaybackState`] so that "the user wants this playing,
/// but it is currently buffering" is a representable and inspectable condition
/// rather than a private boolean. When a transient state resolves, the transport
/// returns to the intent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PlaybackIntent {
    /// The user has stopped playback.
    Stopped,
    /// The user wants audio.
    Playing,
    /// The user has paused.
    Paused,
}

impl fmt::Display for PlaybackIntent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Stopped => "stopped",
            Self::Playing => "playing",
            Self::Paused => "paused",
        })
    }
}

/// Something that happens to the transport.
///
/// Events come from two places: the user, through the command queue, and the
/// engine, when a decoder or a device reports something. Both go through the
/// same table, so a device failure during a seek has a defined outcome rather
/// than an emergent one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum TransportEvent {
    /// Begin loading a track.
    Load,
    /// The track opened and its first buffers are ready.
    LoadSucceeded,
    /// The track could not be opened.
    LoadFailed,
    /// Release the loaded track.
    Unload,
    /// The user pressed play.
    Play,
    /// The user pressed pause.
    Pause,
    /// The user pressed stop.
    Stop,
    /// A seek has begun.
    SeekRequested,
    /// The destination is buffered and playback can continue.
    SeekCompleted,
    /// The decoder could not keep up.
    BufferExhausted,
    /// Enough audio is available to continue.
    BufferRefilled,
    /// The output device disappeared or changed.
    DeviceLost,
    /// A working output device is available again.
    DeviceRestored,
    /// An unrecoverable fault.
    Fault,
    /// Clear an error and return to a known state.
    Reset,
}

impl fmt::Display for TransportEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Load => "load",
            Self::LoadSucceeded => "load succeeded",
            Self::LoadFailed => "load failed",
            Self::Unload => "unload",
            Self::Play => "play",
            Self::Pause => "pause",
            Self::Stop => "stop",
            Self::SeekRequested => "seek requested",
            Self::SeekCompleted => "seek completed",
            Self::BufferExhausted => "buffer exhausted",
            Self::BufferRefilled => "buffer refilled",
            Self::DeviceLost => "device lost",
            Self::DeviceRestored => "device restored",
            Self::Fault => "fault",
            Self::Reset => "reset",
        })
    }
}

/// A transition that is not defined.
///
/// Returned rather than ignored. A rejected transition means either a defect or
/// a race the caller should know about — pressing play on a deck with no track,
/// for instance — and silently dropping it would hide both.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidTransition {
    /// The state the transport was in.
    pub from: PlaybackState,
    /// The event that could not be applied.
    pub event: TransportEvent,
}

impl fmt::Display for InvalidTransition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "cannot apply {} while {}", self.event, self.from)
    }
}

impl core::error::Error for InvalidTransition {}

/// Computes the next state.
///
/// A total function of the current state, the event and the user's intent. Given
/// the same three inputs it always produces the same result, which is what makes
/// the whole table testable by exhaustion rather than by example.
///
/// # Errors
///
/// Returns [`InvalidTransition`] when the event has no meaning in the current
/// state.
#[allow(
    clippy::match_same_arms,
    reason = "the arms are a transition table, grouped by source state so that a \
              reader can check one state's behaviour without searching. Merging \
              arms that happen to share a destination would scatter each state \
              across the table and make the omissions — which are the interesting \
              part — impossible to see."
)]
pub fn next_state(
    from: PlaybackState,
    event: TransportEvent,
    intent: PlaybackIntent,
) -> Result<PlaybackState, InvalidTransition> {
    use PlaybackState as S;
    use TransportEvent as E;

    let reject = Err(InvalidTransition { from, event });

    // A fault is always possible, and a stop is always honoured except from a
    // fault, which must be cleared explicitly. Handling both here keeps the
    // table below free of fifteen repetitions.
    match event {
        E::Fault => return Ok(S::Error),
        E::Stop if from != S::Error => return Ok(S::Stopped),
        _ => {}
    }

    let next = match (from, event) {
        // ---- Stopped: nothing is loaded. ----
        (S::Stopped, E::Load) => S::Loading,
        (S::Stopped, E::Unload | E::Reset) => S::Stopped,

        // ---- Loading. ----
        (S::Loading, E::LoadSucceeded) => S::Ready,
        (S::Loading, E::LoadFailed) => S::Error,
        (S::Loading, E::Unload) => S::Stopped,
        (S::Loading, E::Load) => S::Loading,

        // ---- Ready: a track is cued. ----
        (S::Ready, E::Play) => S::Playing,
        (S::Ready, E::Pause) => S::Paused,
        (S::Ready, E::SeekRequested) => S::Seeking,
        (S::Ready, E::Load) => S::Loading,
        (S::Ready, E::Unload) => S::Stopped,
        (S::Ready, E::DeviceLost) => S::Recovering,

        // ---- Playing. ----
        (S::Playing, E::Pause) => S::Paused,
        (S::Playing, E::Play) => S::Playing,
        (S::Playing, E::SeekRequested) => S::Seeking,
        (S::Playing, E::BufferExhausted) => S::Buffering,
        (S::Playing, E::DeviceLost) => S::Recovering,
        (S::Playing, E::Load) => S::Loading,

        // ---- Paused. ----
        (S::Paused, E::Play) => S::Playing,
        (S::Paused, E::Pause) => S::Paused,
        (S::Paused, E::SeekRequested) => S::Seeking,
        (S::Paused, E::DeviceLost) => S::Recovering,
        (S::Paused, E::Load) => S::Loading,
        (S::Paused, E::Unload) => S::Stopped,

        // ---- Seeking: transient, resolves to the intent. ----
        (S::Seeking, E::SeekCompleted) => resume(intent),
        (S::Seeking, E::SeekRequested) => S::Seeking,
        (S::Seeking, E::BufferExhausted) => S::Buffering,
        (S::Seeking, E::DeviceLost) => S::Recovering,
        // The user may change their mind mid-seek. The intent changes; the
        // transport keeps seeking and lands in the new intent when it arrives.
        (S::Seeking, E::Play | E::Pause) => S::Seeking,

        // ---- Buffering: transient, resolves to the intent. ----
        (S::Buffering, E::BufferRefilled) => resume(intent),
        (S::Buffering, E::SeekRequested) => S::Seeking,
        (S::Buffering, E::DeviceLost) => S::Recovering,
        (S::Buffering, E::Play | E::Pause) => S::Buffering,
        (S::Buffering, E::Load) => S::Loading,

        // ---- Recovering: transient, resolves to the intent. ----
        //
        // Module Specification #002 requires recovery from a device change
        // without stopping playback, so a restored device returns straight to
        // whatever the user wanted rather than to a stopped transport.
        (S::Recovering, E::DeviceRestored) => resume(intent),
        (S::Recovering, E::DeviceLost) => S::Recovering,
        (S::Recovering, E::Play | E::Pause) => S::Recovering,
        (S::Recovering, E::BufferExhausted | E::BufferRefilled) => S::Recovering,

        // ---- Error: only an explicit reset or unload leaves. ----
        (S::Error, E::Reset | E::Unload) => S::Stopped,

        _ => return reject,
    };

    Ok(next)
}

/// The state a transient condition resolves into.
///
/// A transport with no intent to play lands in [`PlaybackState::Ready`] rather
/// than [`PlaybackState::Stopped`], because the track is still loaded and cued.
const fn resume(intent: PlaybackIntent) -> PlaybackState {
    match intent {
        PlaybackIntent::Playing => PlaybackState::Playing,
        PlaybackIntent::Paused => PlaybackState::Paused,
        PlaybackIntent::Stopped => PlaybackState::Ready,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL_STATES: [PlaybackState; 9] = [
        PlaybackState::Stopped,
        PlaybackState::Loading,
        PlaybackState::Ready,
        PlaybackState::Playing,
        PlaybackState::Paused,
        PlaybackState::Seeking,
        PlaybackState::Buffering,
        PlaybackState::Recovering,
        PlaybackState::Error,
    ];

    const ALL_EVENTS: [TransportEvent; 15] = [
        TransportEvent::Load,
        TransportEvent::LoadSucceeded,
        TransportEvent::LoadFailed,
        TransportEvent::Unload,
        TransportEvent::Play,
        TransportEvent::Pause,
        TransportEvent::Stop,
        TransportEvent::SeekRequested,
        TransportEvent::SeekCompleted,
        TransportEvent::BufferExhausted,
        TransportEvent::BufferRefilled,
        TransportEvent::DeviceLost,
        TransportEvent::DeviceRestored,
        TransportEvent::Fault,
        TransportEvent::Reset,
    ];

    const ALL_INTENTS: [PlaybackIntent; 3] = [
        PlaybackIntent::Stopped,
        PlaybackIntent::Playing,
        PlaybackIntent::Paused,
    ];

    #[test]
    fn every_state_and_event_combination_has_a_defined_outcome() {
        // 9 × 15 × 3 = 405 combinations. The point of an explicit table is that
        // none of them is undefined behaviour: each either produces a state or
        // is rejected with a value naming what was refused. This test does not
        // assert *which*; it asserts that the function is total and never
        // panics, which is what "no hidden transitions" means operationally.
        for state in ALL_STATES {
            for event in ALL_EVENTS {
                for intent in ALL_INTENTS {
                    match next_state(state, event, intent) {
                        Ok(next) => assert!(ALL_STATES.contains(&next)),
                        Err(rejection) => {
                            assert_eq!(rejection.from, state);
                            assert_eq!(rejection.event, event);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn a_fault_is_always_reachable() {
        // Whatever the transport is doing, an unrecoverable fault must be
        // representable. A state that could not fail would hide failures.
        for state in ALL_STATES {
            for intent in ALL_INTENTS {
                assert_eq!(
                    next_state(state, TransportEvent::Fault, intent),
                    Ok(PlaybackState::Error),
                    "fault must be accepted while {state}"
                );
            }
        }
    }

    #[test]
    fn stop_is_honoured_everywhere_except_from_an_error() {
        for state in ALL_STATES {
            for intent in ALL_INTENTS {
                let result = next_state(state, TransportEvent::Stop, intent);
                if state == PlaybackState::Error {
                    assert!(result.is_err(), "an error must be cleared explicitly");
                } else {
                    assert_eq!(
                        result,
                        Ok(PlaybackState::Stopped),
                        "stop must always work while {state}"
                    );
                }
            }
        }
    }

    #[test]
    fn an_error_requires_an_explicit_reset() {
        for event in ALL_EVENTS {
            let result = next_state(PlaybackState::Error, event, PlaybackIntent::Playing);
            match event {
                TransportEvent::Reset | TransportEvent::Unload => {
                    assert_eq!(result, Ok(PlaybackState::Stopped));
                }
                TransportEvent::Fault => assert_eq!(result, Ok(PlaybackState::Error)),
                _ => assert!(result.is_err(), "{event} must not clear an error"),
            }
        }
    }

    #[test]
    fn playing_cannot_start_without_a_track() {
        assert!(next_state(
            PlaybackState::Stopped,
            TransportEvent::Play,
            PlaybackIntent::Playing
        )
        .is_err());
    }

    #[test]
    fn transient_states_resolve_to_the_users_intent() {
        let cases = [
            (PlaybackState::Seeking, TransportEvent::SeekCompleted),
            (PlaybackState::Buffering, TransportEvent::BufferRefilled),
            (PlaybackState::Recovering, TransportEvent::DeviceRestored),
        ];
        for (state, event) in cases {
            assert_eq!(
                next_state(state, event, PlaybackIntent::Playing),
                Ok(PlaybackState::Playing),
                "{state} should resume playing"
            );
            assert_eq!(
                next_state(state, event, PlaybackIntent::Paused),
                Ok(PlaybackState::Paused),
                "{state} should resume paused"
            );
            assert_eq!(
                next_state(state, event, PlaybackIntent::Stopped),
                Ok(PlaybackState::Ready),
                "{state} with no intent to play should return to a cued deck"
            );
        }
    }

    #[test]
    fn a_device_lost_during_a_seek_recovers_rather_than_stopping() {
        // The combination that a scattering of booleans would get wrong.
        let recovering = next_state(
            PlaybackState::Seeking,
            TransportEvent::DeviceLost,
            PlaybackIntent::Playing,
        );
        assert_eq!(recovering, Ok(PlaybackState::Recovering));
        assert_eq!(
            next_state(
                PlaybackState::Recovering,
                TransportEvent::DeviceRestored,
                PlaybackIntent::Playing
            ),
            Ok(PlaybackState::Playing),
            "the music must resume, not stop"
        );
    }

    #[test]
    fn changing_intent_mid_transient_does_not_abort_it() {
        // A user who presses pause while buffering should end up paused when the
        // buffer refills, not have the buffering cancelled.
        assert_eq!(
            next_state(
                PlaybackState::Buffering,
                TransportEvent::Pause,
                PlaybackIntent::Playing
            ),
            Ok(PlaybackState::Buffering)
        );
        assert_eq!(
            next_state(
                PlaybackState::Buffering,
                TransportEvent::BufferRefilled,
                PlaybackIntent::Paused
            ),
            Ok(PlaybackState::Paused)
        );
    }

    #[test]
    fn a_failed_load_produces_an_error_rather_than_a_silent_stop() {
        assert_eq!(
            next_state(
                PlaybackState::Loading,
                TransportEvent::LoadFailed,
                PlaybackIntent::Playing
            ),
            Ok(PlaybackState::Error)
        );
    }

    #[test]
    fn loading_a_new_track_is_possible_from_any_loaded_state() {
        for state in [
            PlaybackState::Ready,
            PlaybackState::Playing,
            PlaybackState::Paused,
            PlaybackState::Stopped,
        ] {
            assert_eq!(
                next_state(state, TransportEvent::Load, PlaybackIntent::Playing),
                Ok(PlaybackState::Loading),
                "loading over {state} must be allowed"
            );
        }
    }

    #[test]
    fn state_predicates_agree_with_the_states() {
        assert!(PlaybackState::Playing.is_rendering());
        assert!(!PlaybackState::Buffering.is_rendering());

        for state in [
            PlaybackState::Seeking,
            PlaybackState::Buffering,
            PlaybackState::Recovering,
        ] {
            assert!(state.is_transient(), "{state} is transient");
        }
        assert!(!PlaybackState::Paused.is_transient());

        assert!(!PlaybackState::Stopped.has_track());
        assert!(!PlaybackState::Loading.has_track());
        assert!(!PlaybackState::Error.has_track());
        assert!(PlaybackState::Ready.has_track());
        assert!(PlaybackState::Playing.has_track());
    }

    #[test]
    fn rejections_read_as_a_sentence() {
        let rejection = InvalidTransition {
            from: PlaybackState::Stopped,
            event: TransportEvent::Play,
        };
        assert_eq!(rejection.to_string(), "cannot apply play while stopped");
    }
}
