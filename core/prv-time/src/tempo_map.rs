use core::fmt;

use crate::conversion::{frames_from_ticks, ticks_from_frames};
use crate::error::TimeError;
use crate::frames::Frames;
use crate::musical_time::Ticks;
use crate::sample_rate::SampleRate;
use crate::tempo::Tempo;

/// The largest number of tempo segments a map will hold.
///
/// Ten thousand is far beyond any hand-authored arrangement and comfortably
/// covers a beat grid corrected bar by bar across a long live recording, which
/// is the demanding case Master Prompt #20 describes. The bound exists so that a
/// corrupted project file cannot ask for an unbounded allocation.
const MAX_SEGMENTS: usize = 10_000;

/// One region of constant tempo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TempoSegment {
    /// Musical position at which this tempo takes effect.
    pub start_ticks: Ticks,
    /// Frame position of `start_ticks`, derived from every preceding segment.
    pub start_frames: Frames,
    /// The tempo in force from here until the next segment.
    pub tempo: Tempo,
}

/// A piecewise-constant tempo track.
///
/// # What this is for
///
/// Master Prompt #3A requires beat grids with multiple tempo regions and live
/// tempo changes; Master Prompt #20 requires tempo drift, human timing variation
/// and manual correction to be representable. A single tempo cannot express any
/// of that. Recorded material speeds up and slows down, and a grid that assumes
/// otherwise walks away from the music within a minute.
///
/// # Not for the audio thread
///
/// This structure allocates. It is a domain object, used for timeline arithmetic
/// off the audio thread. The audio thread holds
/// [`TransportClock`](crate::TransportClock), which carries one segment at a
/// time and receives changes as scheduled commands.
///
/// That split is deliberate. Putting the map on the audio thread would mean
/// either a heap-allocated structure in the render path, which ADR-0002 forbids,
/// or a fixed-capacity array sized for the worst case, which wastes memory in
/// every ordinary project. Scheduling segment changes as commands costs nothing
/// and keeps both sides simple.
///
/// # Invariants
///
/// - There is always at least one segment.
/// - The first segment starts at tick zero and frame zero.
/// - Segments are ordered by `start_ticks`, strictly ascending.
/// - `start_frames` is always consistent with every preceding segment, so a
///   conversion never has to re-derive it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TempoMap {
    sample_rate: SampleRate,
    segments: Vec<TempoSegment>,
}

impl TempoMap {
    /// Creates a map with a single tempo from the origin.
    #[must_use]
    pub fn new(sample_rate: SampleRate, tempo: Tempo) -> Self {
        Self {
            sample_rate,
            segments: vec![TempoSegment {
                start_ticks: Ticks::ZERO,
                start_frames: Frames::ZERO,
                tempo,
            }],
        }
    }

    /// The sample rate the frame positions are expressed in.
    #[must_use]
    pub const fn sample_rate(&self) -> SampleRate {
        self.sample_rate
    }

    /// Every segment, in ascending order.
    pub fn segments(&self) -> impl Iterator<Item = &TempoSegment> {
        self.segments.iter()
    }

    /// Number of segments.
    #[must_use]
    pub fn segment_count(&self) -> usize {
        self.segments.len()
    }

    /// Returns the segment governing a musical position.
    #[must_use]
    pub fn segment_at_ticks(&self, ticks: Ticks) -> TempoSegment {
        let index = self.index_at_ticks(ticks);
        self.segments
            .get(index)
            .copied()
            // Unreachable: the map always holds at least one segment and the
            // index is derived from its length. Falling back to the origin
            // keeps this total rather than introducing a panic path.
            .unwrap_or(TempoSegment {
                start_ticks: Ticks::ZERO,
                start_frames: Frames::ZERO,
                tempo: Tempo::BPM_120,
            })
    }

    /// Returns the tempo in force at a musical position.
    #[must_use]
    pub fn tempo_at_ticks(&self, ticks: Ticks) -> Tempo {
        self.segment_at_ticks(ticks).tempo
    }

    /// Returns the tempo in force at a frame position.
    #[must_use]
    pub fn tempo_at_frames(&self, frames: Frames) -> Tempo {
        let index = self.index_at_frames(frames);
        self.segments
            .get(index)
            .map_or(Tempo::BPM_120, |segment| segment.tempo)
    }

    /// Converts a frame position to a musical position.
    #[must_use]
    pub fn ticks_at(&self, frames: Frames) -> Ticks {
        let index = self.index_at_frames(frames);
        let Some(segment) = self.segments.get(index) else {
            return Ticks::ZERO;
        };
        let elapsed = frames.get().saturating_sub(segment.start_frames.get());
        let within = ticks_from_frames(elapsed, self.sample_rate, segment.tempo);
        Ticks::new(segment.start_ticks.get().saturating_add(within))
    }

    /// Converts a musical position to a frame position.
    #[must_use]
    pub fn frames_at(&self, ticks: Ticks) -> Frames {
        let index = self.index_at_ticks(ticks);
        let Some(segment) = self.segments.get(index) else {
            return Frames::ZERO;
        };
        let elapsed = ticks.get().saturating_sub(segment.start_ticks.get());
        let within = frames_from_ticks(elapsed, self.sample_rate, segment.tempo);
        Frames::new(segment.start_frames.get().saturating_add(within))
    }

    /// Sets the tempo from a musical position onward.
    ///
    /// Replaces the segment if one already begins exactly there, otherwise
    /// inserts a new one. Positions before `at` keep their existing
    /// interpretation, so correcting a tempo late in a track does not move the
    /// beat grid at its start — which is what makes bar-by-bar correction of a
    /// drifting recording practical.
    ///
    /// # Errors
    ///
    /// Returns [`TimeError::ConversionOverflow`] if `at` is negative or the map
    /// already holds the maximum number of segments.
    pub fn set_tempo_at(&mut self, at: Ticks, tempo: Tempo) -> Result<(), TimeError> {
        if at.get() < 0 {
            return Err(TimeError::ConversionOverflow);
        }

        match self
            .segments
            .binary_search_by_key(&at.get(), |segment| segment.start_ticks.get())
        {
            Ok(index) => {
                if let Some(segment) = self.segments.get_mut(index) {
                    segment.tempo = tempo;
                }
            }
            Err(index) => {
                if self.segments.len() >= MAX_SEGMENTS {
                    return Err(TimeError::ConversionOverflow);
                }
                self.segments.insert(
                    index,
                    TempoSegment {
                        start_ticks: at,
                        // Recomputed immediately below.
                        start_frames: Frames::ZERO,
                        tempo,
                    },
                );
            }
        }

        self.recompute_frames();
        Ok(())
    }

    /// Removes the tempo change beginning at a musical position.
    ///
    /// The segment at the origin cannot be removed; a map always has a tempo.
    /// Returns `true` if a segment was removed.
    pub fn remove_tempo_at(&mut self, at: Ticks) -> bool {
        if at.get() == 0 {
            return false;
        }
        let Ok(index) = self
            .segments
            .binary_search_by_key(&at.get(), |segment| segment.start_ticks.get())
        else {
            return false;
        };
        self.segments.remove(index);
        self.recompute_frames();
        true
    }

    /// Changes the sample rate, preserving every musical position.
    ///
    /// Frame positions are recomputed; musical positions are untouched. This is
    /// what a device sample-rate change means for the timeline: the music has
    /// not moved, only its representation in frames.
    pub fn set_sample_rate(&mut self, sample_rate: SampleRate) {
        self.sample_rate = sample_rate;
        self.recompute_frames();
    }

    /// Rebuilds `start_frames` for every segment from the tempo of the segment
    /// before it.
    ///
    /// Called after any structural change. Frame positions are cached rather
    /// than derived on every lookup because lookups are far more frequent than
    /// edits, and because caching keeps conversion cost independent of how many
    /// tempo changes precede the position being converted.
    fn recompute_frames(&mut self) {
        let mut previous: Option<TempoSegment> = None;
        for segment in &mut self.segments {
            let start_frames = match previous {
                None => Frames::ZERO,
                Some(before) => {
                    let elapsed = segment
                        .start_ticks
                        .get()
                        .saturating_sub(before.start_ticks.get());
                    let span = frames_from_ticks(elapsed, self.sample_rate, before.tempo);
                    Frames::new(before.start_frames.get().saturating_add(span))
                }
            };
            segment.start_frames = start_frames;
            previous = Some(*segment);
        }
    }

    /// Index of the segment governing a musical position.
    fn index_at_ticks(&self, ticks: Ticks) -> usize {
        match self
            .segments
            .binary_search_by_key(&ticks.get(), |segment| segment.start_ticks.get())
        {
            Ok(index) => index,
            // `Err(0)` means the position precedes the first segment, which can
            // only happen for a negative tick position; the first segment
            // governs it, extrapolating backwards.
            Err(index) => index.saturating_sub(1),
        }
    }

    /// Index of the segment governing a frame position.
    fn index_at_frames(&self, frames: Frames) -> usize {
        match self
            .segments
            .binary_search_by_key(&frames.get(), |segment| segment.start_frames.get())
        {
            Ok(index) => index,
            Err(index) => index.saturating_sub(1),
        }
    }
}

impl fmt::Display for TempoMap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TempoMap({} segments", self.segments.len())?;
        if let Some(first) = self.segments.first() {
            write!(f, ", from {}", first.tempo)?;
        }
        f.write_str(")")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::musical_time::TICKS_PER_BEAT;

    fn beats(count: i64) -> Ticks {
        Ticks::from_beats(count)
    }

    fn map_at(bpm: f64) -> TempoMap {
        let tempo = Tempo::from_bpm(bpm).unwrap_or(Tempo::BPM_120);
        TempoMap::new(SampleRate::HZ_48000, tempo)
    }

    #[test]
    fn a_new_map_has_one_segment_at_the_origin() {
        let map = map_at(120.0);
        assert_eq!(map.segment_count(), 1);
        let segment = map.segment_at_ticks(Ticks::ZERO);
        assert_eq!(segment.start_ticks, Ticks::ZERO);
        assert_eq!(segment.start_frames, Frames::ZERO);
    }

    #[test]
    fn a_constant_map_agrees_with_direct_conversion() {
        let map = map_at(128.0);
        // One beat at 128 BPM and 48 kHz is exactly 22 500 frames.
        assert_eq!(map.frames_at(beats(1)), Frames::new(22_500));
        assert_eq!(map.ticks_at(Frames::new(22_500)), beats(1));
    }

    #[test]
    fn a_tempo_change_only_affects_positions_after_it() {
        let mut map = map_at(120.0);
        // At 120 BPM a beat is 24 000 frames; at 240 it is 12 000.
        let faster = Tempo::from_bpm(240.0);
        assert!(faster.is_ok());
        let Ok(faster) = faster else { return };
        assert!(map.set_tempo_at(beats(4), faster).is_ok());

        // Before the change, unchanged.
        assert_eq!(map.frames_at(beats(2)), Frames::new(48_000));
        assert_eq!(map.frames_at(beats(4)), Frames::new(96_000));
        // After it, beats are half as long.
        assert_eq!(map.frames_at(beats(5)), Frames::new(96_000 + 12_000));
        assert_eq!(map.frames_at(beats(8)), Frames::new(96_000 + 48_000));
    }

    #[test]
    fn conversion_round_trips_across_segment_boundaries() {
        let mut map = map_at(120.0);
        for (beat, bpm) in [(4_i64, 140.0_f64), (12, 90.0), (20, 174.0)] {
            let tempo = Tempo::from_bpm(bpm);
            assert!(tempo.is_ok());
            if let Ok(tempo) = tempo {
                assert!(map.set_tempo_at(beats(beat), tempo).is_ok());
            }
        }
        assert_eq!(map.segment_count(), 4);

        for beat in 0..64_i64 {
            let ticks = beats(beat);
            let frames = map.frames_at(ticks);
            assert_eq!(
                map.ticks_at(frames),
                ticks,
                "round trip failed at beat {beat}"
            );
        }
    }

    #[test]
    fn positions_are_monotonic_across_segments() {
        // The property that matters most: time must never go backwards, however
        // the tempo changes. A non-monotonic map would let the playhead jump
        // backwards at a tempo boundary.
        let mut map = map_at(120.0);
        for (beat, bpm) in [(4_i64, 200.0_f64), (8, 60.0), (16, 174.0), (24, 90.0)] {
            let tempo = Tempo::from_bpm(bpm);
            if let Ok(tempo) = tempo {
                assert!(map.set_tempo_at(beats(beat), tempo).is_ok());
            }
        }

        let mut previous = Frames::new(i64::MIN);
        for tick in 0..(32 * i64::from(TICKS_PER_BEAT)) {
            let frames = map.frames_at(Ticks::new(tick));
            assert!(
                frames >= previous,
                "frame position went backwards at tick {tick}"
            );
            previous = frames;
        }
    }

    #[test]
    fn setting_a_tempo_at_an_existing_boundary_replaces_it() {
        let mut map = map_at(120.0);
        let first = Tempo::from_bpm(140.0);
        let second = Tempo::from_bpm(160.0);
        let (Ok(first), Ok(second)) = (first, second) else {
            return;
        };

        assert!(map.set_tempo_at(beats(4), first).is_ok());
        assert!(map.set_tempo_at(beats(4), second).is_ok());
        assert_eq!(
            map.segment_count(),
            2,
            "no duplicate segment may be created"
        );
        assert_eq!(map.tempo_at_ticks(beats(4)), second);
    }

    #[test]
    fn the_origin_segment_cannot_be_removed() {
        let mut map = map_at(120.0);
        assert!(!map.remove_tempo_at(Ticks::ZERO));
        assert_eq!(map.segment_count(), 1);
    }

    #[test]
    fn removing_a_segment_restores_the_preceding_tempo() {
        let mut map = map_at(120.0);
        let faster = Tempo::from_bpm(240.0);
        let Ok(faster) = faster else { return };
        assert!(map.set_tempo_at(beats(4), faster).is_ok());
        assert_eq!(map.frames_at(beats(8)), Frames::new(96_000 + 48_000));

        assert!(map.remove_tempo_at(beats(4)));
        assert_eq!(map.segment_count(), 1);
        assert_eq!(map.frames_at(beats(8)), Frames::new(192_000));
    }

    #[test]
    fn removing_a_position_with_no_segment_reports_it() {
        let mut map = map_at(120.0);
        assert!(!map.remove_tempo_at(beats(7)));
    }

    #[test]
    fn negative_tempo_positions_are_rejected() {
        let mut map = map_at(120.0);
        assert_eq!(
            map.set_tempo_at(Ticks::new(-1), Tempo::BPM_128),
            Err(TimeError::ConversionOverflow)
        );
    }

    #[test]
    fn changing_the_sample_rate_preserves_musical_positions() {
        let mut map = map_at(120.0);
        let faster = Tempo::from_bpm(240.0);
        let Ok(faster) = faster else { return };
        assert!(map.set_tempo_at(beats(4), faster).is_ok());

        let before: Vec<Ticks> = (0..16)
            .map(|beat| map.ticks_at(map.frames_at(beats(beat))))
            .collect();

        map.set_sample_rate(SampleRate::HZ_96000);

        // Twice the rate, twice the frames, same music.
        assert_eq!(map.frames_at(beats(4)), Frames::new(192_000));
        let after: Vec<Ticks> = (0..16)
            .map(|beat| map.ticks_at(map.frames_at(beats(beat))))
            .collect();
        assert_eq!(before, after);
    }

    #[test]
    fn tempo_lookup_by_frame_agrees_with_lookup_by_tick() {
        let mut map = map_at(120.0);
        let faster = Tempo::from_bpm(174.0);
        let Ok(faster) = faster else { return };
        assert!(map.set_tempo_at(beats(8), faster).is_ok());

        for beat in 0..24_i64 {
            let ticks = beats(beat);
            let frames = map.frames_at(ticks);
            assert_eq!(
                map.tempo_at_frames(frames),
                map.tempo_at_ticks(ticks),
                "tempo lookups disagreed at beat {beat}"
            );
        }
    }

    #[test]
    fn positions_before_the_origin_extrapolate_from_the_first_segment() {
        // Count-ins and transitions that begin before the arrangement.
        let map = map_at(120.0);
        assert_eq!(map.frames_at(beats(-2)), Frames::new(-48_000));
        assert_eq!(map.ticks_at(Frames::new(-48_000)), beats(-2));
    }

    #[test]
    fn a_map_full_of_segments_refuses_further_insertion() {
        let mut map = map_at(120.0);
        let mut inserted = 1_usize;
        let mut beat = 1_i64;
        while inserted < MAX_SEGMENTS {
            if map.set_tempo_at(beats(beat), Tempo::BPM_128).is_err() {
                break;
            }
            inserted += 1;
            beat += 1;
        }
        assert_eq!(map.segment_count(), MAX_SEGMENTS);
        assert!(
            map.set_tempo_at(beats(beat), Tempo::BPM_174).is_err(),
            "the segment bound must be enforced"
        );
    }
}
