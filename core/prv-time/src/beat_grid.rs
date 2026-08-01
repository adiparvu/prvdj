use core::fmt;

use crate::frames::Frames;
use crate::musical_time::{MusicalTime, Ticks, TICKS_PER_BEAT};
use crate::sample_rate::SampleRate;
use crate::signature::TimeSignature;
use crate::tempo::Tempo;
use crate::tempo_map::TempoMap;

/// Default phrase length, in bars.
///
/// Eight is the near-universal phrase in dance music: intros, build-ups and
/// drops almost always begin on an eight-bar boundary. A grid that could snap
/// only to bars would be technically correct and musically useless, because a
/// transition landing on bar 5 of an 8-bar phrase is heard as a mistake even
/// though it is perfectly on the beat.
pub const DEFAULT_PHRASE_BARS: u32 = 8;

/// Maximum corrective steps taken by a directional snap.
///
/// One step is always sufficient: the frame-to-tick conversion errs by at most
/// half a tick, which is less than one snap unit for every resolution. Two makes
/// the bound obviously safe while keeping the loop provably terminating.
const CORRECTION_STEPS: u32 = 2;

/// What a position is snapped to.
///
/// Ordered from coarsest to finest. Master Prompt #21 requires beat, bar, phrase
/// and sample resolutions; `Division` covers the loop and edit resolutions a
/// performer selects — halves, triplets, sixteenths.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SnapResolution {
    /// No snapping. The position is returned unchanged.
    Sample,
    /// The nearest tick.
    Tick,
    /// A subdivision of the beat: 1 is a beat, 2 an eighth, 4 a sixteenth,
    /// 3 a triplet. Zero is treated as 1.
    Division(u32),
    /// The nearest beat.
    Beat,
    /// The nearest bar line.
    Bar,
    /// The nearest phrase boundary.
    Phrase,
}

/// The musical grid a track or project is measured against.
///
/// # What a beat grid is
///
/// A tempo map says how fast the music is. A beat grid says *where* it is: the
/// frame at which the first downbeat falls. Detection gets the tempo right far
/// more often than it gets the downbeat right, and a grid one beat out of phase
/// is worse than no grid at all — every transition lands on the wrong part of the
/// bar. So the origin is a first-class, separately adjustable value, exactly as
/// Master Prompt #3A requires of editable markers and manual correction.
///
/// # Not for the audio thread
///
/// Holds a [`TempoMap`], which allocates. This is timeline arithmetic, performed
/// off the audio thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BeatGrid {
    origin: Frames,
    tempo_map: TempoMap,
    signature: TimeSignature,
    phrase_bars: u32,
}

impl BeatGrid {
    /// Creates a grid with a constant tempo, its first downbeat at `origin`.
    #[must_use]
    pub fn new(
        sample_rate: SampleRate,
        tempo: Tempo,
        signature: TimeSignature,
        origin: Frames,
    ) -> Self {
        Self {
            origin,
            tempo_map: TempoMap::new(sample_rate, tempo),
            signature,
            phrase_bars: DEFAULT_PHRASE_BARS,
        }
    }

    /// Creates a grid from an existing tempo map.
    #[must_use]
    pub fn with_tempo_map(tempo_map: TempoMap, signature: TimeSignature, origin: Frames) -> Self {
        Self {
            origin,
            tempo_map,
            signature,
            phrase_bars: DEFAULT_PHRASE_BARS,
        }
    }

    /// The frame at which the first downbeat falls.
    #[must_use]
    pub const fn origin(&self) -> Frames {
        self.origin
    }

    /// Moves the first downbeat to an absolute frame position.
    pub fn set_origin(&mut self, origin: Frames) {
        self.origin = origin;
    }

    /// Shifts the whole grid by a number of frames.
    ///
    /// The nudge a user reaches for when the tempo is right but the downbeat is
    /// a few milliseconds early or late — the most common manual correction
    /// there is.
    pub fn nudge(&mut self, delta: Frames) {
        self.origin += delta;
    }

    /// The tempo map.
    #[must_use]
    pub const fn tempo_map(&self) -> &TempoMap {
        &self.tempo_map
    }

    /// The tempo map, for editing.
    pub fn tempo_map_mut(&mut self) -> &mut TempoMap {
        &mut self.tempo_map
    }

    /// The time signature.
    #[must_use]
    pub const fn signature(&self) -> TimeSignature {
        self.signature
    }

    /// Sets the time signature.
    pub fn set_signature(&mut self, signature: TimeSignature) {
        self.signature = signature;
    }

    /// Phrase length in bars.
    #[must_use]
    pub const fn phrase_bars(&self) -> u32 {
        self.phrase_bars
    }

    /// Sets the phrase length in bars. Zero is treated as one.
    pub fn set_phrase_bars(&mut self, bars: u32) {
        self.phrase_bars = bars.max(1);
    }

    /// Converts a frame position to musical ticks relative to the grid origin.
    #[must_use]
    pub fn ticks_at(&self, frames: Frames) -> Ticks {
        self.tempo_map.ticks_at(frames - self.origin)
    }

    /// Converts musical ticks to an absolute frame position.
    #[must_use]
    pub fn frames_at(&self, ticks: Ticks) -> Frames {
        self.tempo_map.frames_at(ticks) + self.origin
    }

    /// Converts a frame position to bar, beat and tick.
    #[must_use]
    pub fn musical_at(&self, frames: Frames) -> MusicalTime {
        self.signature.ticks_to_musical(self.ticks_at(frames))
    }

    /// Converts a bar, beat and tick position to frames.
    #[must_use]
    pub fn frames_at_musical(&self, position: MusicalTime) -> Frames {
        self.frames_at(self.signature.musical_to_ticks(position))
    }

    /// Ticks per unit of the given resolution, or `None` for [`SnapResolution::Sample`].
    #[must_use]
    fn ticks_per_unit(&self, resolution: SnapResolution) -> Option<i64> {
        let per_beat = i64::from(TICKS_PER_BEAT);
        let per_bar = i64::from(self.signature.ticks_per_bar());
        Some(match resolution {
            SnapResolution::Sample => return None,
            SnapResolution::Tick => 1,
            SnapResolution::Division(division) => {
                let division = i64::from(division.max(1));
                // Truncation is intentional and bounded: a division that does
                // not divide the beat exactly lands between ticks, and the
                // nearest whole tick below is the closest representable point.
                // A division finer than one tick is meaningless, so the result
                // is clamped to a tick rather than collapsing to zero.
                #[allow(
                    clippy::integer_division,
                    reason = "truncation to the nearest representable tick is the intended behaviour"
                )]
                let per_division = per_beat / division;
                per_division.max(1)
            }
            SnapResolution::Beat => per_beat,
            SnapResolution::Bar => per_bar,
            SnapResolution::Phrase => per_bar.saturating_mul(i64::from(self.phrase_bars.max(1))),
        })
    }

    /// Snaps a frame position to the nearest point of the given resolution.
    ///
    /// Snapping happens in musical time, not in frames. Snapping in frames would
    /// drift away from the music the moment the tempo changed, which defeats the
    /// purpose.
    #[must_use]
    pub fn snap(&self, frames: Frames, resolution: SnapResolution) -> Frames {
        let Some(unit) = self.ticks_per_unit(resolution) else {
            return frames;
        };
        let ticks = self.ticks_at(frames).get();
        let snapped = round_to_multiple(ticks, unit);
        self.frames_at(Ticks::new(snapped))
    }

    /// Snaps to the nearest point at or before `frames`.
    ///
    /// The result is guaranteed not to lie after `frames`. See
    /// [`Self::snap_forward`] for why that guarantee needs enforcing.
    #[must_use]
    pub fn snap_back(&self, frames: Frames, resolution: SnapResolution) -> Frames {
        let Some(unit) = self.ticks_per_unit(resolution) else {
            return frames;
        };
        let mut target = floor_to_multiple(self.ticks_at(frames).get(), unit);
        let mut result = self.frames_at(Ticks::new(target));
        for _ in 0..CORRECTION_STEPS {
            if result <= frames {
                break;
            }
            target = target.saturating_sub(unit);
            result = self.frames_at(Ticks::new(target));
        }
        result
    }

    /// Snaps to the nearest point at or after `frames`.
    ///
    /// # Why the result is verified in frames
    ///
    /// At ordinary tempi one tick spans several frames, so converting a frame
    /// position into ticks rounds. A position one frame past a beat rounds back
    /// *to* that beat in tick space, and snapping "forward" from there would
    /// return a position slightly before where the caller started.
    ///
    /// For a nearest-point snap that is harmless. For a directional snap it is
    /// not: a loop whose end snapped backwards past its own start would be
    /// silently empty. So the result is checked against the input in frames —
    /// the authoritative unit — and stepped if the rounding landed on the wrong
    /// side. At most one step is ever needed; the bound exists so the loop is
    /// provably terminating.
    #[must_use]
    pub fn snap_forward(&self, frames: Frames, resolution: SnapResolution) -> Frames {
        let Some(unit) = self.ticks_per_unit(resolution) else {
            return frames;
        };
        let mut target = floor_to_multiple(self.ticks_at(frames).get(), unit);
        let mut result = self.frames_at(Ticks::new(target));
        for _ in 0..CORRECTION_STEPS {
            if result >= frames {
                break;
            }
            target = target.saturating_add(unit);
            result = self.frames_at(Ticks::new(target));
        }
        result
    }

    /// Returns `true` if a frame position falls exactly on the given resolution.
    #[must_use]
    pub fn is_aligned(&self, frames: Frames, resolution: SnapResolution) -> bool {
        let Some(unit) = self.ticks_per_unit(resolution) else {
            return true;
        };
        self.ticks_at(frames).get().rem_euclid(unit) == 0
    }
}

/// Rounds to the nearest multiple, with ties going away from zero.
fn round_to_multiple(value: i64, unit: i64) -> i64 {
    if unit <= 1 {
        return value;
    }
    let floored = floor_to_multiple(value, unit);
    let remainder = value.saturating_sub(floored);
    // `remainder` is in `0..unit` because `floor_to_multiple` floors.
    if remainder.saturating_mul(2) >= unit {
        floored.saturating_add(unit)
    } else {
        floored
    }
}

/// Rounds down to a multiple, flooring rather than truncating so that negative
/// positions behave correctly.
fn floor_to_multiple(value: i64, unit: i64) -> i64 {
    if unit <= 1 {
        return value;
    }
    value.div_euclid(unit).saturating_mul(unit)
}

impl fmt::Display for BeatGrid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "BeatGrid(origin {}, {}, {} bars per phrase)",
            self.origin, self.signature, self.phrase_bars
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid_at(bpm: f64) -> BeatGrid {
        let tempo = Tempo::from_bpm(bpm).unwrap_or(Tempo::BPM_120);
        BeatGrid::new(
            SampleRate::HZ_48000,
            tempo,
            TimeSignature::FOUR_FOUR,
            Frames::ZERO,
        )
    }

    #[test]
    fn the_origin_defines_the_first_downbeat() {
        let mut grid = grid_at(120.0);
        grid.set_origin(Frames::new(5_000));

        assert_eq!(grid.musical_at(Frames::new(5_000)), MusicalTime::ZERO);
        // One beat at 120 BPM and 48 kHz is 24 000 frames.
        assert_eq!(
            grid.musical_at(Frames::new(5_000 + 24_000)),
            MusicalTime::new(0, 1, 0)
        );
    }

    #[test]
    fn nudging_moves_the_whole_grid() {
        let mut grid = grid_at(120.0);
        grid.nudge(Frames::new(-120));
        assert_eq!(grid.origin(), Frames::new(-120));
        assert_eq!(grid.frames_at(Ticks::ZERO), Frames::new(-120));
    }

    #[test]
    fn snapping_to_a_beat_takes_the_nearer_side() {
        let grid = grid_at(120.0);
        // At 120 BPM and 48 kHz a beat is 24 000 frames; halves and quarters
        // are written out rather than divided so the tests contain no
        // arithmetic of their own.
        let beat = 24_000_i64;
        let half_beat = 12_000_i64;

        // Just past a beat snaps back to it.
        assert_eq!(
            grid.snap(Frames::new(beat + 100), SnapResolution::Beat),
            Frames::new(beat)
        );
        // Just before the next snaps forward to it.
        assert_eq!(
            grid.snap(Frames::new(2 * beat - 100), SnapResolution::Beat),
            Frames::new(2 * beat)
        );
        // Exactly halfway rounds away from zero, deterministically.
        assert_eq!(
            grid.snap(Frames::new(half_beat), SnapResolution::Beat),
            Frames::new(beat)
        );
    }

    #[test]
    fn snapping_to_a_bar_and_a_phrase() {
        let grid = grid_at(120.0);
        let beat = 24_000_i64;
        let bar = beat * 4;
        let phrase = bar * 8;

        assert_eq!(
            grid.snap(Frames::new(bar + 1_000), SnapResolution::Bar),
            Frames::new(bar)
        );
        assert_eq!(
            grid.snap(Frames::new(phrase + 1_000), SnapResolution::Phrase),
            Frames::new(phrase)
        );
        // A position three bars into a phrase snaps to the phrase start, not the
        // nearest bar — which is the whole point of phrase snapping.
        assert_eq!(
            grid.snap(Frames::new(bar * 3), SnapResolution::Phrase),
            Frames::new(0)
        );
        assert_eq!(
            grid.snap(Frames::new(bar * 5), SnapResolution::Phrase),
            Frames::new(phrase)
        );
    }

    #[test]
    fn phrase_length_is_configurable() {
        let mut grid = grid_at(120.0);
        assert_eq!(grid.phrase_bars(), DEFAULT_PHRASE_BARS);
        grid.set_phrase_bars(16);
        assert_eq!(grid.phrase_bars(), 16);
        grid.set_phrase_bars(0);
        assert_eq!(grid.phrase_bars(), 1, "zero is meaningless and becomes one");
    }

    #[test]
    fn divisions_cover_the_common_loop_lengths() {
        let grid = grid_at(120.0);
        // 24 000 frames to the beat: an eighth is 12 000, a sixteenth 6 000,
        // and a triplet 8 000 — all exact, because the tick resolution divides
        // by two, three and four without remainder.
        assert_eq!(
            grid.snap(Frames::new(12_000 + 200), SnapResolution::Division(2)),
            Frames::new(12_000)
        );
        assert_eq!(
            grid.snap(Frames::new(6_000 + 100), SnapResolution::Division(4)),
            Frames::new(6_000)
        );
        assert_eq!(
            grid.snap(Frames::new(8_000 + 100), SnapResolution::Division(3)),
            Frames::new(8_000)
        );
    }

    #[test]
    fn sample_resolution_does_not_snap() {
        let grid = grid_at(120.0);
        let awkward = Frames::new(12_345);
        assert_eq!(grid.snap(awkward, SnapResolution::Sample), awkward);
        assert!(grid.is_aligned(awkward, SnapResolution::Sample));
    }

    #[test]
    fn directional_snapping_never_crosses_the_position() {
        // Exhaustive over a whole beat rather than at a few sampled offsets.
        // The failure this guards against appears only at offsets small enough
        // that the frame-to-tick conversion rounds back across the boundary —
        // one frame past a beat, not a thousand — so sampling would have missed
        // it. A loop whose end snapped backwards past its own start would be
        // silently empty.
        let grid = grid_at(120.0);
        let beat = 24_000_i64;
        for offset in 1..beat {
            let position = Frames::new(beat + offset);
            let back = grid.snap_back(position, SnapResolution::Beat);
            let forward = grid.snap_forward(position, SnapResolution::Beat);
            assert!(
                back <= position,
                "snap_back moved forwards at offset {offset}"
            );
            assert!(
                forward >= position,
                "snap_forward moved backwards at offset {offset}"
            );
            assert_eq!(back, Frames::new(beat), "at offset {offset}");
            assert_eq!(forward, Frames::new(2 * beat), "at offset {offset}");
        }
    }

    #[test]
    fn directional_snapping_holds_at_every_resolution() {
        let grid = grid_at(128.0);
        for resolution in [
            SnapResolution::Tick,
            SnapResolution::Division(4),
            SnapResolution::Division(3),
            SnapResolution::Beat,
            SnapResolution::Bar,
            SnapResolution::Phrase,
        ] {
            for frame in (0..200_000_i64).step_by(997) {
                let position = Frames::new(frame);
                assert!(
                    grid.snap_back(position, resolution) <= position,
                    "snap_back crossed at {frame} for {resolution:?}"
                );
                assert!(
                    grid.snap_forward(position, resolution) >= position,
                    "snap_forward crossed at {frame} for {resolution:?}"
                );
            }
        }
    }

    #[test]
    fn snapping_an_aligned_position_leaves_it_alone() {
        let grid = grid_at(128.0);
        for resolution in [
            SnapResolution::Beat,
            SnapResolution::Bar,
            SnapResolution::Phrase,
            SnapResolution::Division(4),
        ] {
            let aligned = grid.snap(Frames::new(1_234_567), resolution);
            assert_eq!(
                grid.snap(aligned, resolution),
                aligned,
                "snapping must be idempotent"
            );
            assert!(grid.is_aligned(aligned, resolution));
            assert_eq!(
                grid.snap_back(aligned, resolution),
                aligned,
                "an aligned position must not move backwards"
            );
            assert_eq!(
                grid.snap_forward(aligned, resolution),
                aligned,
                "an aligned position must not move forwards"
            );
        }
    }

    #[test]
    fn snapping_follows_a_tempo_change() {
        // The reason snapping happens in musical time rather than in frames: a
        // grid that snapped by frame arithmetic would drift away from the music
        // the moment the tempo changed.
        let mut grid = grid_at(120.0);
        let faster = Tempo::from_bpm(240.0);
        let Ok(faster) = faster else { return };
        assert!(grid
            .tempo_map_mut()
            .set_tempo_at(Ticks::from_beats(4), faster)
            .is_ok());

        // Beats 0 to 4 are 24 000 frames apart; after that, 12 000.
        assert_eq!(grid.frames_at(Ticks::from_beats(4)), Frames::new(96_000));
        assert_eq!(grid.frames_at(Ticks::from_beats(5)), Frames::new(108_000));
        assert_eq!(
            grid.snap(Frames::new(108_000 + 500), SnapResolution::Beat),
            Frames::new(108_000)
        );
    }

    #[test]
    fn negative_positions_snap_correctly() {
        // Truncation towards zero would snap -100 to 0 and -25 000 to -24 000
        // inconsistently. Flooring gives the musically correct answer on both
        // sides of the origin.
        let grid = grid_at(120.0);
        let beat = 24_000_i64;
        assert_eq!(
            grid.snap(Frames::new(-100), SnapResolution::Beat),
            Frames::ZERO
        );
        assert_eq!(
            grid.snap(Frames::new(-beat - 100), SnapResolution::Beat),
            Frames::new(-beat)
        );
        assert_eq!(
            grid.snap_back(Frames::new(-100), SnapResolution::Beat),
            Frames::new(-beat)
        );
    }

    #[test]
    fn rounding_helpers_floor_rather_than_truncate() {
        assert_eq!(floor_to_multiple(10, 4), 8);
        assert_eq!(floor_to_multiple(-10, 4), -12);
        assert_eq!(floor_to_multiple(8, 4), 8);
        assert_eq!(round_to_multiple(10, 4), 12);
        assert_eq!(round_to_multiple(9, 4), 8);
        assert_eq!(round_to_multiple(-10, 4), -8);
        assert_eq!(round_to_multiple(5, 1), 5);
    }
}
