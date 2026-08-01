use crate::frames::Frames;
use crate::musical_time::{MusicalTime, Ticks, TICKS_PER_BEAT};
use crate::sample_rate::SampleRate;
use crate::signature::TimeSignature;
use crate::tempo::Tempo;

/// Microseconds in a second, used in the frame/tick conversions.
const MICROS_PER_SECOND: i128 = 1_000_000;

/// Divides two integers, rounding to the nearest value and away from zero on a
/// tie.
///
/// Used for both directions of the frame/tick conversion. Rounding to nearest
/// rather than truncating halves the worst-case error and, more importantly,
/// makes the conversion symmetric: a tick position converted to frames and back
/// returns the original tick whenever one tick spans at least one frame.
///
/// `denominator` is always positive at every call site in this module.
const fn div_round_nearest(numerator: i128, denominator: i128) -> i128 {
    // Guard kept for total-function discipline even though no call site can
    // pass zero: the sample rate and tempo are both validated non-zero.
    if denominator == 0 {
        return 0;
    }
    #[allow(
        clippy::integer_division,
        reason = "rounding is applied explicitly by the bias term"
    )]
    if numerator >= 0 {
        (numerator + denominator / 2) / denominator
    } else {
        (numerator - denominator / 2) / denominator
    }
}

/// An immutable view of transport state at one instant.
///
/// The audio thread publishes one of these per processed block; every other
/// subsystem — the timeline playhead, waveform overlays, meters, diagnostics —
/// reads position from here rather than computing it independently. That is what
/// makes the playhead, the waveform and the beat grid agree by construction
/// rather than by luck (Module Specification #002, #003).
///
/// The type is `Copy` and free of indirection so that it can be published
/// through a wait-free triple buffer without allocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransportSnapshot {
    /// Absolute playback position in sample frames. Authoritative.
    pub position: Frames,
    /// Playback position in musical ticks, derived exactly from `position`.
    pub ticks: Ticks,
    /// Playback position decomposed into bar, beat and tick.
    pub musical: MusicalTime,
    /// Tempo in effect at this instant.
    pub tempo: Tempo,
    /// Time signature in effect at this instant.
    pub signature: TimeSignature,
    /// Sample rate in effect at this instant.
    pub sample_rate: SampleRate,
}

/// The authoritative transport clock.
///
/// # The single-clock rule
///
/// Module Specification #002 requires exactly one authoritative clock, and that
/// no other module create its own. This type is that clock. Nothing else in the
/// core can construct musical position from wall-clock time, because nothing
/// else has the conversion.
///
/// # How drift is prevented
///
/// Position is an exact integer count of frames. Musical position is computed
/// from it on demand with exact integer arithmetic. Nothing is accumulated in
/// floating point, so there is nothing to drift.
///
/// # Tempo changes do not move the music
///
/// The clock keeps an anchor: the frame position and tick position at which the
/// current tempo took effect. Musical position is measured forward from that
/// anchor. Changing tempo therefore re-anchors at the current instant rather
/// than reinterpreting the whole timeline, so the bar the user is currently
/// hearing does not jump underneath them.
///
/// This is a single-segment tempo map. Extending it to the multiple tempo
/// regions required by Master Prompt #3A means storing a list of anchors and
/// binary-searching it; the arithmetic below does not change.
///
/// # Realtime safety
///
/// Every operation is allocation-free, branch-predictable and panic-free, as
/// required by ADR-0002. [`Self::advance`] is called from the audio callback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransportClock {
    sample_rate: SampleRate,
    tempo: Tempo,
    signature: TimeSignature,
    position: Frames,
    anchor_frames: Frames,
    anchor_ticks: Ticks,
}

impl TransportClock {
    /// Creates a clock at position zero.
    #[must_use]
    pub const fn new(sample_rate: SampleRate, tempo: Tempo, signature: TimeSignature) -> Self {
        Self {
            sample_rate,
            tempo,
            signature,
            position: Frames::ZERO,
            anchor_frames: Frames::ZERO,
            anchor_ticks: Ticks::ZERO,
        }
    }

    /// Advances the clock by a number of frames.
    ///
    /// Called once per audio block with the exact number of frames rendered.
    /// Because the count is integral and simply added, the clock cannot drift
    /// however many blocks are processed.
    pub fn advance(&mut self, frames: u32) {
        self.position += Frames::from(frames);
    }

    /// Moves the clock to an absolute frame position.
    pub fn seek(&mut self, position: Frames) {
        self.position = position;
    }

    /// Moves the clock to an absolute musical position.
    ///
    /// The frame position is rounded to the nearest frame, which is the closest
    /// representable point to the requested musical instant.
    pub fn seek_musical(&mut self, position: MusicalTime) {
        let ticks = self.signature.musical_to_ticks(position);
        self.position = self.ticks_to_frames(ticks);
    }

    /// Returns the current position in frames. Authoritative.
    #[must_use]
    pub const fn position(self) -> Frames {
        self.position
    }

    /// Returns the current position in musical ticks.
    #[must_use]
    pub fn ticks(self) -> Ticks {
        self.frames_to_ticks(self.position)
    }

    /// Returns the current position decomposed into bar, beat and tick.
    #[must_use]
    pub fn musical_position(self) -> MusicalTime {
        self.signature.ticks_to_musical(self.ticks())
    }

    /// Returns the current position in seconds.
    ///
    /// Lossy, for display only.
    #[must_use]
    pub fn seconds(self) -> f64 {
        self.position.as_seconds(self.sample_rate)
    }

    /// Returns the tempo currently in effect.
    #[must_use]
    pub const fn tempo(self) -> Tempo {
        self.tempo
    }

    /// Returns the time signature currently in effect.
    #[must_use]
    pub const fn signature(self) -> TimeSignature {
        self.signature
    }

    /// Returns the sample rate currently in effect.
    #[must_use]
    pub const fn sample_rate(self) -> SampleRate {
        self.sample_rate
    }

    /// Changes the tempo from the current instant onward.
    ///
    /// Re-anchors so that the musical position the listener is currently hearing
    /// is preserved. Musical time before this instant keeps its original
    /// interpretation.
    pub fn set_tempo(&mut self, tempo: Tempo) {
        self.anchor_ticks = self.ticks();
        self.anchor_frames = self.position;
        self.tempo = tempo;
    }

    /// Changes the time signature.
    ///
    /// Affects only the decomposition of ticks into bars and beats; it does not
    /// move playback position.
    pub fn set_signature(&mut self, signature: TimeSignature) {
        self.signature = signature;
    }

    /// Changes the sample rate, preserving musical position.
    ///
    /// Called when the audio device changes underneath a running transport.
    /// Module Specification #002 requires recovery from a sample-rate change
    /// without stopping playback; preserving musical position rather than frame
    /// position is what makes the music continue from where the listener heard
    /// it, instead of jumping.
    pub fn set_sample_rate(&mut self, sample_rate: SampleRate) {
        let ticks = self.ticks();
        self.sample_rate = sample_rate;
        self.anchor_frames = Frames::ZERO;
        self.anchor_ticks = Ticks::ZERO;
        self.position = self.ticks_to_frames(ticks);
        self.anchor_frames = self.position;
        self.anchor_ticks = ticks;
    }

    /// Converts an absolute frame position to an absolute tick position.
    ///
    /// Exact integer arithmetic with 128-bit intermediates: the result is
    /// bit-identical on every platform.
    #[must_use]
    pub fn frames_to_ticks(self, position: Frames) -> Ticks {
        let elapsed = i128::from(position.get() - self.anchor_frames.get());
        let numerator = elapsed
            .saturating_mul(MICROS_PER_SECOND)
            .saturating_mul(i128::from(TICKS_PER_BEAT));
        let denominator = i128::from(self.sample_rate.hz_u64())
            .saturating_mul(i128::from(self.tempo.micros_per_beat()));
        let ticks = div_round_nearest(numerator, denominator);
        Ticks::new(saturate_to_i64(
            ticks.saturating_add(i128::from(self.anchor_ticks.get())),
        ))
    }

    /// Converts an absolute tick position to an absolute frame position.
    ///
    /// Rounds to the nearest frame.
    ///
    /// # Round-trip guarantee
    ///
    /// A tick position converted to frames and back returns the original tick
    /// whenever one tick spans at least one frame. That holds for every
    /// supported sample rate up to roughly `sample_rate / 64` beats per minute —
    /// about 689 BPM at 44.1 kHz and 750 BPM at 48 kHz — which covers the entire
    /// musical range including double-time detection results.
    ///
    /// Above that threshold ticks are finer than frames, and no exact
    /// round-trip is possible in principle; the error is bounded by half a
    /// frame.
    #[must_use]
    pub fn ticks_to_frames(self, ticks: Ticks) -> Frames {
        let elapsed = i128::from(ticks.get() - self.anchor_ticks.get());
        let numerator = elapsed
            .saturating_mul(i128::from(self.sample_rate.hz_u64()))
            .saturating_mul(i128::from(self.tempo.micros_per_beat()));
        let denominator = MICROS_PER_SECOND.saturating_mul(i128::from(TICKS_PER_BEAT));
        let frames = div_round_nearest(numerator, denominator);
        Frames::new(saturate_to_i64(
            frames.saturating_add(i128::from(self.anchor_frames.get())),
        ))
    }

    /// Returns an immutable snapshot for publication to other subsystems.
    #[must_use]
    pub fn snapshot(self) -> TransportSnapshot {
        let ticks = self.ticks();
        TransportSnapshot {
            position: self.position,
            ticks,
            musical: self.signature.ticks_to_musical(ticks),
            tempo: self.tempo,
            signature: self.signature,
            sample_rate: self.sample_rate,
        }
    }
}

/// Clamps a 128-bit intermediate into the 64-bit domain type.
///
/// Reaching either bound requires a position beyond any physically possible
/// session, but clamping explicitly is preferred to a silent wrap, which would
/// place the playhead somewhere arbitrary.
const fn saturate_to_i64(value: i128) -> i64 {
    if value > i64::MAX as i128 {
        i64::MAX
    } else if value < i64::MIN as i128 {
        i64::MIN
    } else {
        #[allow(
            clippy::cast_possible_truncation,
            reason = "both bounds checked immediately above; this branch is in range by construction"
        )]
        {
            value as i64
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clock_at(bpm: f64) -> TransportClock {
        let tempo = Tempo::from_bpm(bpm).unwrap_or(Tempo::BPM_120);
        TransportClock::new(SampleRate::HZ_48000, tempo, TimeSignature::FOUR_FOUR)
    }

    #[test]
    fn one_beat_at_128_bpm_is_22500_frames_at_48k() {
        // 60 / 128 seconds = 0.46875 s; at 48 000 Hz that is exactly 22 500
        // frames, so this tempo and rate combination has no rounding at all.
        let clock = clock_at(128.0);
        assert_eq!(clock.ticks_to_frames(Ticks::BEAT), Frames::new(22_500));
    }

    #[test]
    fn advancing_never_drifts() {
        // The property that matters: advancing in many small blocks must land
        // in exactly the same place as one large seek. Any floating-point
        // accumulation would fail this within a few thousand iterations.
        let mut clock = clock_at(128.0);
        let block = 128_u32;
        let blocks = 100_000_u32;
        for _ in 0..blocks {
            clock.advance(block);
        }
        assert_eq!(
            clock.position(),
            Frames::new(i64::from(block) * i64::from(blocks))
        );
    }

    #[test]
    fn six_hours_of_playback_has_zero_drift() {
        // Six hours at 48 kHz in 512-frame blocks: the length of a long
        // festival set, processed one buffer at a time.
        let mut clock = clock_at(124.0);
        let block = 512_u32;
        // 48 000 × 3600 × 6 = 1 036 800 000 frames, which is exactly
        // 2 025 000 blocks of 512. Stated as a product so the test contains no
        // division of its own.
        let blocks = 2_025_000_i64;
        for _ in 0..blocks {
            clock.advance(block);
        }
        let expected = Frames::new(blocks * i64::from(block));
        assert_eq!(expected.get(), 48_000 * 3_600 * 6);
        assert_eq!(clock.position(), expected);

        // And the musical position derived from it is exact, not approximate.
        let ticks_direct = clock.frames_to_ticks(expected);
        assert_eq!(clock.ticks(), ticks_direct);
    }

    #[test]
    fn tick_to_frame_round_trip_is_exact_across_the_musical_range() {
        for bpm in [
            20.0_f64, 60.0, 90.0, 120.0, 124.0, 128.0, 140.0, 174.0, 200.0, 300.0,
        ] {
            let clock = clock_at(bpm);
            for beats in 0..512_i64 {
                let ticks = Ticks::from_beats(beats);
                let frames = clock.ticks_to_frames(ticks);
                assert_eq!(
                    clock.frames_to_ticks(frames),
                    ticks,
                    "round trip failed at {bpm} BPM, beat {beats}"
                );
            }
        }
    }

    #[test]
    fn musical_position_tracks_the_beat_grid() {
        let mut clock = clock_at(120.0);
        // At 120 BPM and 48 kHz a beat is exactly 24 000 frames.
        clock.advance(24_000);
        assert_eq!(clock.musical_position(), MusicalTime::new(0, 1, 0));
        clock.advance(24_000 * 3);
        assert_eq!(clock.musical_position(), MusicalTime::new(1, 0, 0));
    }

    #[test]
    fn changing_tempo_preserves_the_current_musical_position() {
        let mut clock = clock_at(120.0);
        clock.advance(24_000 * 5); // Bar 1, beat 1.
        let before = clock.musical_position();
        assert_eq!(before, MusicalTime::new(1, 1, 0));

        let faster = Tempo::from_bpm(160.0);
        assert!(faster.is_ok());
        if let Ok(faster) = faster {
            clock.set_tempo(faster);
        }

        // The listener is in the same place in the music; only the rate at
        // which the next bar arrives has changed.
        assert_eq!(clock.musical_position(), before);
        assert_eq!(clock.position(), Frames::new(24_000 * 5));
    }

    #[test]
    fn tempo_change_advances_at_the_new_rate() {
        let mut clock = clock_at(120.0);
        let doubled = Tempo::from_bpm(240.0);
        assert!(doubled.is_ok());
        if let Ok(doubled) = doubled {
            clock.set_tempo(doubled);
        }
        // At 240 BPM a beat is 12 000 frames rather than 24 000.
        clock.advance(12_000);
        assert_eq!(clock.musical_position(), MusicalTime::new(0, 1, 0));
    }

    #[test]
    fn sample_rate_change_preserves_musical_position() {
        // Module Specification #002: recover from a device sample-rate change
        // without stopping playback. What must survive is where the listener is
        // in the music, not the raw frame count.
        let mut clock = clock_at(120.0);
        clock.advance(24_000 * 6);
        let before = clock.musical_position();

        clock.set_sample_rate(SampleRate::HZ_96000);

        assert_eq!(clock.musical_position(), before);
        assert_eq!(clock.sample_rate(), SampleRate::HZ_96000);
        // Twice the rate means twice the frames for the same musical instant.
        assert_eq!(clock.position(), Frames::new(24_000 * 6 * 2));
    }

    #[test]
    fn seek_musical_lands_on_the_requested_position() {
        let mut clock = clock_at(128.0);
        clock.seek_musical(MusicalTime::new(16, 2, 1_920));
        assert_eq!(clock.musical_position(), MusicalTime::new(16, 2, 1_920));
    }

    #[test]
    fn snapshot_is_self_consistent() {
        let mut clock = clock_at(128.0);
        clock.advance(22_500 * 9);
        let snapshot = clock.snapshot();
        assert_eq!(snapshot.position, clock.position());
        assert_eq!(snapshot.ticks, clock.ticks());
        assert_eq!(snapshot.musical, clock.musical_position());
        assert_eq!(snapshot.tempo, clock.tempo());
        assert_eq!(snapshot.sample_rate, clock.sample_rate());
    }

    #[test]
    fn negative_positions_are_representable() {
        // Count-ins and transitions that begin before the arrangement.
        let mut clock = clock_at(120.0);
        clock.seek(Frames::new(-24_000));
        assert_eq!(clock.musical_position(), MusicalTime::new(-1, 3, 0));
    }

    #[test]
    fn rounding_helper_is_symmetric_about_zero() {
        assert_eq!(div_round_nearest(10, 4), 3);
        assert_eq!(div_round_nearest(-10, 4), -3);
        assert_eq!(div_round_nearest(2, 4), 1);
        assert_eq!(div_round_nearest(-2, 4), -1);
        assert_eq!(div_round_nearest(0, 4), 0);
        assert_eq!(div_round_nearest(1, 0), 0);
    }
}
