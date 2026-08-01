use core::fmt;

use prv_time::{
    Frames, MusicalTime, SampleRate, Tempo, TimeSignature, TransportClock, TransportSnapshot,
};

use crate::looping::LoopRegion;
use crate::state::{next_state, InvalidTransition, PlaybackIntent, PlaybackState, TransportEvent};

/// Failures a transport operation can report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum TransportError {
    /// The event has no meaning in the current state.
    InvalidTransition(InvalidTransition),
    /// A loop region whose end is not after its start.
    InvalidLoopRegion,
}

impl fmt::Display for TransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidTransition(inner) => write!(f, "{inner}"),
            Self::InvalidLoopRegion => f.write_str("a loop must end after it starts"),
        }
    }
}

impl core::error::Error for TransportError {}

impl From<InvalidTransition> for TransportError {
    fn from(inner: InvalidTransition) -> Self {
        Self::InvalidTransition(inner)
    }
}

/// One deck's transport.
///
/// Owns the authoritative clock for that deck, its playback state, the user's
/// intent, its loop region and its slip position.
///
/// # Realtime safety
///
/// `Copy`, allocation-free and panic-free. [`Self::advance`] and
/// [`Self::frames_until_wrap`] are called from the audio callback and obey the
/// contract in ADR-0002.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Transport {
    clock: TransportClock,
    state: PlaybackState,
    intent: PlaybackIntent,
    loop_region: LoopRegion,
    slip: bool,
    slip_position: Frames,
}

impl Transport {
    /// Creates a stopped transport at position zero.
    #[must_use]
    pub const fn new(sample_rate: SampleRate, tempo: Tempo, signature: TimeSignature) -> Self {
        Self {
            clock: TransportClock::new(sample_rate, tempo, signature),
            state: PlaybackState::Stopped,
            intent: PlaybackIntent::Stopped,
            loop_region: LoopRegion::none(),
            slip: false,
            slip_position: Frames::ZERO,
        }
    }

    /// The current playback state.
    #[must_use]
    pub const fn state(self) -> PlaybackState {
        self.state
    }

    /// What the user asked for.
    #[must_use]
    pub const fn intent(self) -> PlaybackIntent {
        self.intent
    }

    /// The clock.
    #[must_use]
    pub const fn clock(self) -> TransportClock {
        self.clock
    }

    /// The current position.
    #[must_use]
    pub const fn position(self) -> Frames {
        self.clock.position()
    }

    /// The loop region.
    #[must_use]
    pub const fn loop_region(self) -> LoopRegion {
        self.loop_region
    }

    /// Whether slip mode is engaged.
    #[must_use]
    pub const fn is_slipping(self) -> bool {
        self.slip
    }

    /// Where playback would be if the loop had not been engaged.
    ///
    /// Meaningful only while slip mode is on.
    #[must_use]
    pub const fn slip_position(self) -> Frames {
        self.slip_position
    }

    /// Applies an event.
    ///
    /// Updates the user's intent for the three events that express one, then
    /// computes the new state. Intent is updated first so that a transient state
    /// resolving in the same instant resolves into the *new* intent.
    ///
    /// # Errors
    ///
    /// Returns [`TransportError::InvalidTransition`] if the event has no meaning
    /// in the current state. The transport is left unchanged.
    pub fn apply(&mut self, event: TransportEvent) -> Result<PlaybackState, TransportError> {
        let intent = match event {
            TransportEvent::Play => PlaybackIntent::Playing,
            TransportEvent::Pause => PlaybackIntent::Paused,
            TransportEvent::Stop | TransportEvent::Unload | TransportEvent::LoadFailed => {
                PlaybackIntent::Stopped
            }
            _ => self.intent,
        };

        let next = next_state(self.state, event, intent)?;

        self.intent = intent;
        self.state = next;

        if next == PlaybackState::Stopped {
            self.clock.seek(Frames::ZERO);
            self.slip_position = Frames::ZERO;
        }

        Ok(next)
    }

    /// Moves the position.
    ///
    /// Does not change state: seeking while paused leaves the transport paused.
    /// A caller that wants the transient [`PlaybackState::Seeking`] state sends
    /// [`TransportEvent::SeekRequested`] as well; whether a seek is instant or
    /// needs buffering is a property of the source, not of the transport.
    pub fn seek(&mut self, position: Frames) {
        self.clock.seek(position);
        self.slip_position = position;
    }

    /// Moves the position to a musical instant.
    pub fn seek_musical(&mut self, position: MusicalTime) {
        self.clock.seek_musical(position);
        self.slip_position = self.clock.position();
    }

    /// Sets the loop region, preserving whether the loop is engaged.
    ///
    /// # Errors
    ///
    /// Returns [`TransportError::InvalidLoopRegion`] if `end` is not after
    /// `start`.
    pub fn set_loop(&mut self, start: Frames, end: Frames) -> Result<(), TransportError> {
        let region = LoopRegion::new(start, end).ok_or(TransportError::InvalidLoopRegion)?;
        self.loop_region = if self.loop_region.is_enabled() {
            region.enabled()
        } else {
            region
        };
        Ok(())
    }

    /// Engages the loop.
    ///
    /// Engaging a loop while the playhead is already past it does not move the
    /// playhead: the loop takes effect the next time the playhead reaches the
    /// end, which is what a performer expects when they arm a loop ahead of the
    /// music.
    pub fn enable_loop(&mut self) {
        self.loop_region = self.loop_region.enabled();
    }

    /// Disengages the loop.
    ///
    /// If slip mode is on, playback jumps to where it would have been had the
    /// loop never been engaged — the whole point of slip.
    pub fn disable_loop(&mut self) {
        self.loop_region = self.loop_region.disabled();
        if self.slip {
            self.clock.seek(self.slip_position);
        }
    }

    /// Turns slip mode on or off.
    ///
    /// While on, a shadow position advances as though nothing were looping, so
    /// that leaving the loop returns to the arrangement in time rather than
    /// wherever the loop happened to stop. Turning slip on resets the shadow to
    /// the current position.
    pub fn set_slip(&mut self, slip: bool) {
        if slip && !self.slip {
            self.slip_position = self.clock.position();
        }
        self.slip = slip;
    }

    /// How many frames may be rendered before the loop wraps.
    ///
    /// The graph renders in chunks of this size so that a wrap always falls on a
    /// block boundary. Rendering across a wrap and correcting afterwards would
    /// produce a discontinuity in the middle of a buffer, which is a click.
    ///
    /// Returns `requested` when no wrap is pending.
    #[must_use]
    pub fn frames_until_wrap(self, requested: u32) -> u32 {
        if !self.should_loop() {
            return requested;
        }
        let remaining = self
            .loop_region
            .end()
            .get()
            .saturating_sub(self.clock.position().get());
        if remaining <= 0 {
            return requested;
        }
        let remaining = u32::try_from(remaining).unwrap_or(u32::MAX);
        requested.min(remaining)
    }

    /// Advances by a number of frames, wrapping at the loop end.
    ///
    /// Called once per rendered chunk. When slip mode is on, the shadow position
    /// advances by the same amount without wrapping.
    pub fn advance(&mut self, frames: u32) {
        self.clock.advance(frames);
        if self.slip {
            self.slip_position += Frames::from(frames);
        }
        if self.should_loop() {
            let wrapped = self.loop_region.wrap(self.clock.position());
            if wrapped != self.clock.position() {
                self.clock.seek(wrapped);
            }
        }
    }

    /// Whether the loop should act on the current position.
    ///
    /// The loop engages only once the playhead has entered it. A loop armed
    /// ahead of the playhead does not drag the music backwards.
    fn should_loop(self) -> bool {
        self.loop_region.is_enabled() && self.clock.position() >= self.loop_region.start()
    }

    /// Changes the tempo, preserving musical position.
    pub fn set_tempo(&mut self, tempo: Tempo) {
        self.clock.set_tempo(tempo);
    }

    /// Changes the time signature.
    pub fn set_signature(&mut self, signature: TimeSignature) {
        self.clock.set_signature(signature);
    }

    /// Changes the sample rate, preserving musical position.
    ///
    /// Called when the device changes underneath a running transport. Loop
    /// bounds and the slip position are expressed in frames, so they are
    /// rescaled to keep the same musical extent.
    pub fn set_sample_rate(&mut self, sample_rate: SampleRate) {
        let previous = self.clock.sample_rate();
        self.clock.set_sample_rate(sample_rate);
        if previous == sample_rate {
            return;
        }
        let rescale = |frames: Frames| -> Frames {
            let scaled = i128::from(frames.get())
                .saturating_mul(i128::from(sample_rate.hz_u64()))
                .checked_div(i128::from(previous.hz_u64()))
                .unwrap_or(0);
            Frames::new(i64::try_from(scaled).unwrap_or(i64::MAX))
        };
        let start = rescale(self.loop_region.start());
        let end = rescale(self.loop_region.end());
        if let Some(region) = LoopRegion::new(start, end) {
            self.loop_region = if self.loop_region.is_enabled() {
                region.enabled()
            } else {
                region
            };
        }
        self.slip_position = rescale(self.slip_position);
    }

    /// A snapshot for publication to other subsystems.
    #[must_use]
    pub fn snapshot(self) -> TransportSnapshot {
        self.clock.snapshot()
    }
}

impl fmt::Display for Transport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} (intent {}) at {}",
            self.state,
            self.intent,
            self.clock.position()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn transport() -> Transport {
        Transport::new(
            SampleRate::HZ_48000,
            Tempo::BPM_120,
            TimeSignature::FOUR_FOUR,
        )
    }

    /// Drives a transport to a loaded, playing state.
    fn playing() -> Transport {
        let mut transport = transport();
        assert!(transport.apply(TransportEvent::Load).is_ok());
        assert!(transport.apply(TransportEvent::LoadSucceeded).is_ok());
        assert!(transport.apply(TransportEvent::Play).is_ok());
        transport
    }

    #[test]
    fn a_new_transport_is_stopped_at_the_origin() {
        let transport = transport();
        assert_eq!(transport.state(), PlaybackState::Stopped);
        assert_eq!(transport.intent(), PlaybackIntent::Stopped);
        assert_eq!(transport.position(), Frames::ZERO);
    }

    #[test]
    fn play_records_the_intent_as_well_as_the_state() {
        let transport = playing();
        assert_eq!(transport.state(), PlaybackState::Playing);
        assert_eq!(transport.intent(), PlaybackIntent::Playing);
    }

    #[test]
    fn a_rejected_event_leaves_the_transport_untouched() {
        let mut transport = transport();
        let before = transport;
        assert!(transport.apply(TransportEvent::Play).is_err());
        assert_eq!(transport, before, "a rejected event must change nothing");
    }

    #[test]
    fn stopping_returns_to_the_origin() {
        let mut transport = playing();
        transport.advance(48_000);
        assert_eq!(transport.position(), Frames::new(48_000));

        assert!(transport.apply(TransportEvent::Stop).is_ok());
        assert_eq!(transport.state(), PlaybackState::Stopped);
        assert_eq!(transport.position(), Frames::ZERO);
    }

    #[test]
    fn pausing_holds_the_position() {
        let mut transport = playing();
        transport.advance(24_000);
        assert!(transport.apply(TransportEvent::Pause).is_ok());
        assert_eq!(transport.state(), PlaybackState::Paused);
        assert_eq!(transport.position(), Frames::new(24_000));
    }

    #[test]
    fn a_buffer_underrun_resumes_where_it_left_off() {
        let mut transport = playing();
        transport.advance(10_000);
        assert!(transport.apply(TransportEvent::BufferExhausted).is_ok());
        assert_eq!(transport.state(), PlaybackState::Buffering);
        assert_eq!(transport.position(), Frames::new(10_000));

        assert!(transport.apply(TransportEvent::BufferRefilled).is_ok());
        assert_eq!(transport.state(), PlaybackState::Playing);
        assert_eq!(transport.position(), Frames::new(10_000));
    }

    #[test]
    fn a_device_change_does_not_stop_the_music() {
        // Module Specification #002's central recovery requirement.
        let mut transport = playing();
        transport.advance(96_000);
        let position = transport.position();

        assert!(transport.apply(TransportEvent::DeviceLost).is_ok());
        assert_eq!(transport.state(), PlaybackState::Recovering);
        assert_eq!(transport.position(), position, "position must survive");

        assert!(transport.apply(TransportEvent::DeviceRestored).is_ok());
        assert_eq!(transport.state(), PlaybackState::Playing);
        assert_eq!(transport.position(), position);
    }

    #[test]
    fn a_loop_wraps_exactly_at_its_end() {
        let mut transport = playing();
        assert!(transport
            .set_loop(Frames::new(1_000), Frames::new(1_400))
            .is_ok());
        transport.enable_loop();
        transport.seek(Frames::new(1_000));

        // The chunk is clipped so the wrap lands on a block boundary.
        assert_eq!(transport.frames_until_wrap(512), 400);
        transport.advance(400);
        assert_eq!(transport.position(), Frames::new(1_000), "wrapped to start");
    }

    #[test]
    fn a_loop_repeats_indefinitely_without_drifting() {
        let mut transport = playing();
        assert!(transport
            .set_loop(Frames::new(0), Frames::new(1_000))
            .is_ok());
        transport.enable_loop();

        // Ten thousand passes: any per-wrap rounding would show up as a
        // position that is no longer a multiple of the block size.
        for _ in 0..10_000 {
            let chunk = transport.frames_until_wrap(256);
            transport.advance(chunk);
        }
        assert!(
            transport.position() < Frames::new(1_000),
            "the playhead must stay inside the loop"
        );
        assert_eq!(
            transport.position().get() % 256,
            0,
            "phase must be preserved across every wrap"
        );
    }

    #[test]
    fn a_loop_shorter_than_a_block_still_keeps_phase() {
        // A beat roll can be shorter than the audio block. Jumping to the start
        // on every wrap would lose the remainder each time and slide the roll
        // out of time.
        let mut transport = playing();
        assert!(transport.set_loop(Frames::new(0), Frames::new(100)).is_ok());
        transport.enable_loop();

        transport.advance(512);
        assert_eq!(transport.position(), Frames::new(12));
    }

    #[test]
    fn a_loop_armed_ahead_of_the_playhead_does_not_drag_it_back() {
        let mut transport = playing();
        assert!(transport
            .set_loop(Frames::new(10_000), Frames::new(20_000))
            .is_ok());
        transport.enable_loop();

        // The playhead is before the loop, so it advances normally.
        assert_eq!(transport.frames_until_wrap(512), 512);
        transport.advance(512);
        assert_eq!(transport.position(), Frames::new(512));
    }

    #[test]
    fn slip_mode_returns_to_the_arrangement() {
        // The defining behaviour of slip: the music underneath keeps running.
        let mut transport = playing();
        assert!(transport
            .set_loop(Frames::new(0), Frames::new(1_000))
            .is_ok());
        transport.set_slip(true);
        transport.enable_loop();

        // Chunks are clipped at the loop boundary, so the number of frames
        // actually rendered is not simply the requested size times the
        // iterations. Accumulating it is the point: the shadow position must
        // equal every frame of audio that passed, whatever the chunking.
        let mut rendered = 0_i64;
        for _ in 0..10 {
            let chunk = transport.frames_until_wrap(400);
            transport.advance(chunk);
            rendered += i64::from(chunk);
        }
        assert!(rendered > 3_000, "the test must actually render some audio");
        assert_eq!(
            transport.slip_position(),
            Frames::new(rendered),
            "the shadow position must not wrap"
        );
        assert!(transport.position() < Frames::new(1_000));

        transport.disable_loop();
        assert_eq!(
            transport.position(),
            Frames::new(rendered),
            "leaving a slipped loop must land where the arrangement is"
        );
    }

    #[test]
    fn without_slip_leaving_a_loop_stays_put() {
        let mut transport = playing();
        assert!(transport
            .set_loop(Frames::new(0), Frames::new(1_000))
            .is_ok());
        transport.enable_loop();
        transport.advance(400);

        transport.disable_loop();
        assert_eq!(transport.position(), Frames::new(400));
    }

    #[test]
    fn an_invalid_loop_region_is_rejected() {
        let mut transport = playing();
        assert_eq!(
            transport.set_loop(Frames::new(500), Frames::new(500)),
            Err(TransportError::InvalidLoopRegion)
        );
        assert_eq!(
            transport.set_loop(Frames::new(500), Frames::new(100)),
            Err(TransportError::InvalidLoopRegion)
        );
    }

    #[test]
    fn setting_a_new_loop_keeps_it_engaged() {
        let mut transport = playing();
        assert!(transport
            .set_loop(Frames::new(0), Frames::new(1_000))
            .is_ok());
        transport.enable_loop();
        assert!(transport.loop_region().is_enabled());

        assert!(transport
            .set_loop(Frames::new(2_000), Frames::new(3_000))
            .is_ok());
        assert!(
            transport.loop_region().is_enabled(),
            "halving a loop must not disengage it"
        );
    }

    #[test]
    fn a_sample_rate_change_rescales_the_loop_and_preserves_the_music() {
        let mut transport = playing();
        assert!(transport
            .set_loop(Frames::new(24_000), Frames::new(48_000))
            .is_ok());
        transport.enable_loop();
        transport.seek(Frames::new(24_000));
        let musical_before = transport.clock().musical_position();

        transport.set_sample_rate(SampleRate::HZ_96000);

        assert_eq!(transport.clock().musical_position(), musical_before);
        assert_eq!(transport.loop_region().start(), Frames::new(48_000));
        assert_eq!(transport.loop_region().end(), Frames::new(96_000));
        assert!(transport.loop_region().is_enabled());
    }

    #[test]
    fn seeking_does_not_change_state() {
        let mut transport = playing();
        assert!(transport.apply(TransportEvent::Pause).is_ok());
        transport.seek(Frames::new(50_000));
        assert_eq!(transport.state(), PlaybackState::Paused);
        assert_eq!(transport.position(), Frames::new(50_000));
    }

    #[test]
    fn a_pause_during_a_seek_lands_paused() {
        let mut transport = playing();
        assert!(transport.apply(TransportEvent::SeekRequested).is_ok());
        assert_eq!(transport.state(), PlaybackState::Seeking);

        assert!(transport.apply(TransportEvent::Pause).is_ok());
        assert_eq!(
            transport.state(),
            PlaybackState::Seeking,
            "the seek continues"
        );
        assert_eq!(transport.intent(), PlaybackIntent::Paused);

        assert!(transport.apply(TransportEvent::SeekCompleted).is_ok());
        assert_eq!(transport.state(), PlaybackState::Paused);
    }

    #[test]
    fn a_fault_requires_an_explicit_reset() {
        let mut transport = playing();
        assert!(transport.apply(TransportEvent::Fault).is_ok());
        assert_eq!(transport.state(), PlaybackState::Error);
        assert!(transport.apply(TransportEvent::Play).is_err());
        assert!(transport.apply(TransportEvent::Reset).is_ok());
        assert_eq!(transport.state(), PlaybackState::Stopped);
    }
}
