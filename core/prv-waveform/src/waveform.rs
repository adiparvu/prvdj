use core::fmt;

use crate::tile::{GenerationVersion, Tile};

/// Samples per tile at each resolution, finest first.
///
/// # How the ladder is chosen
///
/// Each step is a power of two and an exact multiple of the one below it, so
/// every coarse tile boundary lands on a fine tile boundary. That alignment is
/// what lets a zoom change resolution without the waveform appearing to shift
/// sideways — a visual artefact that is small, constant, and maddening.
///
/// The five bands correspond to the resolutions Module Specification #003 names.
/// At 48 kHz the finest tile is 1.3 milliseconds, fine enough that a transient
/// is a visible spike at maximum zoom; the coarsest is 1.4 seconds, so a
/// ten-minute track is about four hundred tiles at overview scale.
pub const RESOLUTION_LADDER: [u32; 5] = [64, 256, 1_024, 8_192, 65_536];

/// The largest number of tiles a single level will hold.
///
/// At the finest resolution this is about six hours of audio at 48 kHz. The
/// bound exists so that a corrupted duration cannot ask for an unbounded
/// allocation.
const MAX_TILES_PER_LEVEL_U64: u64 = 16_000_000;

/// Failures from waveform construction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum WaveformError {
    /// A channel count of zero.
    ZeroChannels,
    /// The chunk supplied a different number of channels than the builder was
    /// created with.
    ChannelCountMismatch {
        /// Channels the builder expects.
        expected: usize,
        /// Channels the chunk supplied.
        supplied: usize,
    },
    /// The channels in the chunk had different lengths.
    ///
    /// A frame is one sample across all channels, so a chunk with a long left
    /// and a short right is not a chunk of audio; it is a bug in whatever
    /// produced it, and continuing would silently misalign the two channels.
    RaggedChunk,
    /// The waveform would exceed the engine's allocation bound.
    TooLarge,
}

impl fmt::Display for WaveformError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroChannels => f.write_str("a waveform must have at least one channel"),
            Self::ChannelCountMismatch { expected, supplied } => {
                write!(f, "expected {expected} channels, got {supplied}")
            }
            Self::RaggedChunk => f.write_str("channels in a chunk must be the same length"),
            Self::TooLarge => f.write_str("waveform exceeds the allocation bound"),
        }
    }
}

impl core::error::Error for WaveformError {}

/// One resolution band.
#[derive(Debug, Clone, PartialEq)]
pub struct WaveformLevel {
    samples_per_tile: u32,
    channels: usize,
    tiles_per_channel: usize,
    /// Channel-major: channel `c` occupies `c * tiles_per_channel ..`.
    tiles: Vec<Tile>,
}

impl WaveformLevel {
    /// Samples summarised by one tile.
    #[must_use]
    pub const fn samples_per_tile(&self) -> u32 {
        self.samples_per_tile
    }

    /// Tiles in each channel.
    #[must_use]
    pub const fn tiles_per_channel(&self) -> usize {
        self.tiles_per_channel
    }

    /// Number of channels.
    #[must_use]
    pub const fn channels(&self) -> usize {
        self.channels
    }

    /// One channel's tiles.
    #[must_use]
    pub fn channel(&self, channel: usize) -> Option<&[Tile]> {
        let start = channel.checked_mul(self.tiles_per_channel)?;
        let end = start.checked_add(self.tiles_per_channel)?;
        self.tiles.get(start..end)
    }

    /// One tile.
    #[must_use]
    pub fn tile(&self, channel: usize, index: usize) -> Option<Tile> {
        self.channel(channel)?.get(index).copied()
    }
}

/// A complete waveform: every resolution, for every channel.
#[derive(Debug, Clone, PartialEq)]
pub struct Waveform {
    channels: usize,
    total_frames: u64,
    version: GenerationVersion,
    levels: Vec<WaveformLevel>,
}

impl Waveform {
    /// Number of channels.
    #[must_use]
    pub const fn channels(&self) -> usize {
        self.channels
    }

    /// Total frames summarised.
    #[must_use]
    pub const fn total_frames(&self) -> u64 {
        self.total_frames
    }

    /// The generation version these tiles were produced at.
    #[must_use]
    pub const fn version(&self) -> GenerationVersion {
        self.version
    }

    /// Whether the tiles were produced by the current algorithm.
    ///
    /// A waveform that is not current must be regenerated before it is drawn.
    /// Master Prompt #20 requires an analysis change to invalidate exactly what
    /// it affects; this is how the waveform participates in that.
    #[must_use]
    pub const fn is_current(&self) -> bool {
        self.version.is_current()
    }

    /// Every resolution, finest first.
    pub fn levels(&self) -> impl Iterator<Item = &WaveformLevel> {
        self.levels.iter()
    }

    /// The level at an index in the ladder.
    #[must_use]
    pub fn level(&self, index: usize) -> Option<&WaveformLevel> {
        self.levels.get(index)
    }

    /// Chooses the resolution to draw a given zoom at.
    ///
    /// Returns the coarsest level whose tiles are no wider than one pixel, so
    /// that each pixel is backed by at least one measured tile.
    ///
    /// Never returns a level coarser than the pixel span. Module Specification
    /// #003 forbids upscaling coarse data, and the reason is not aesthetic: an
    /// upscaled peak is a claim about a transient that was never measured, and a
    /// DJ cutting on a transient that is not there cuts in the wrong place.
    #[must_use]
    pub fn level_for(&self, samples_per_pixel: f64) -> Option<&WaveformLevel> {
        if !samples_per_pixel.is_finite() || samples_per_pixel <= 0.0 {
            return self.levels.first();
        }
        let mut chosen = self.levels.first();
        for level in &self.levels {
            if f64::from(level.samples_per_tile) <= samples_per_pixel {
                chosen = Some(level);
            } else {
                break;
            }
        }
        chosen
    }
}

/// Accumulator for one tile that is not yet complete.
#[derive(Debug, Clone, Copy)]
struct PartialTile {
    min: f32,
    max: f32,
    sum_of_squares: f64,
    counted: u32,
}

impl PartialTile {
    const fn new() -> Self {
        Self {
            min: f32::INFINITY,
            max: f32::NEG_INFINITY,
            sum_of_squares: 0.0,
            counted: 0,
        }
    }

    fn push(&mut self, sample: f32) {
        if !sample.is_finite() {
            // A damaged frame is skipped rather than allowed to poison the tile.
            return;
        }
        if sample < self.min {
            self.min = sample;
        }
        if sample > self.max {
            self.max = sample;
        }
        self.sum_of_squares += f64::from(sample) * f64::from(sample);
        self.counted += 1;
    }

    fn finish(self) -> Tile {
        if self.counted == 0 || !self.min.is_finite() || !self.max.is_finite() {
            return Tile::SILENT;
        }
        #[allow(
            clippy::cast_precision_loss,
            clippy::cast_possible_truncation,
            reason = "tile spans are bounded by the ladder and energy is a display value"
        )]
        let energy = (self.sum_of_squares / f64::from(self.counted)).sqrt() as f32;
        Tile {
            min: self.min,
            max: self.max,
            energy,
        }
    }
}

/// One level under construction.
#[derive(Debug)]
struct LevelBuilder {
    samples_per_tile: u32,
    tiles: Vec<Vec<Tile>>,
    partials: Vec<PartialTile>,
    filled: u32,
}

impl LevelBuilder {
    fn new(samples_per_tile: u32, channels: usize) -> Self {
        Self {
            samples_per_tile,
            tiles: (0..channels).map(|_| Vec::new()).collect(),
            partials: vec![PartialTile::new(); channels],
            filled: 0,
        }
    }

    /// Feeds one frame's worth of samples, one per channel.
    fn push_frame(&mut self, frame: &[f32]) {
        for (channel, sample) in frame.iter().enumerate() {
            if let Some(partial) = self.partials.get_mut(channel) {
                partial.push(*sample);
            }
        }
        self.filled += 1;
        if self.filled >= self.samples_per_tile {
            self.flush();
        }
    }

    /// Completes the tile in progress.
    fn flush(&mut self) {
        if self.filled == 0 {
            return;
        }
        for (channel, partial) in self.partials.iter_mut().enumerate() {
            let tile = partial.finish();
            if let Some(column) = self.tiles.get_mut(channel) {
                column.push(tile);
            }
            *partial = PartialTile::new();
        }
        self.filled = 0;
    }

    fn finish(mut self) -> WaveformLevel {
        self.flush();
        let channels = self.tiles.len();
        let tiles_per_channel = self.tiles.first().map_or(0, Vec::len);
        let mut flat = Vec::with_capacity(channels * tiles_per_channel);
        for column in self.tiles {
            flat.extend(column);
        }
        WaveformLevel {
            samples_per_tile: self.samples_per_tile,
            channels,
            tiles_per_channel,
            tiles: flat,
        }
    }
}

/// Builds a waveform from audio supplied in arbitrary chunks.
///
/// # Why chunked
///
/// Master Prompt #7 requires every stage of the import pipeline to be
/// restartable, and Module Specification #003 requires generation to be
/// progressive. Both need the same thing: the ability to consume audio as it
/// arrives rather than requiring the whole file in memory. An eleven-million-
/// sample track does not need eleven million samples resident to be summarised.
///
/// Feeding the same audio in different chunk sizes produces identical tiles,
/// which is what makes an interrupted import resumable rather than restartable.
///
/// # Why every level is built from the audio
///
/// Coarse levels could be built by merging finer tiles. For minimum and maximum
/// that would be exact, but energy would not: the mean of means is only the true
/// mean when the spans are equal, and it drifts further at every rung. Building
/// each level directly from the samples costs one pass and keeps every level
/// exact.
#[derive(Debug)]
pub struct WaveformBuilder {
    channels: usize,
    total_frames: u64,
    levels: Vec<LevelBuilder>,
    frame_scratch: Vec<f32>,
}

impl WaveformBuilder {
    /// Creates a builder for a given channel count.
    ///
    /// # Errors
    ///
    /// Returns [`WaveformError::ZeroChannels`] if `channels` is zero.
    pub fn new(channels: usize) -> Result<Self, WaveformError> {
        if channels == 0 {
            return Err(WaveformError::ZeroChannels);
        }
        Ok(Self {
            channels,
            total_frames: 0,
            levels: RESOLUTION_LADDER
                .iter()
                .map(|samples| LevelBuilder::new(*samples, channels))
                .collect(),
            frame_scratch: vec![0.0; channels],
        })
    }

    /// Frames consumed so far.
    ///
    /// An interrupted import records this and resumes from it.
    #[must_use]
    pub const fn frames_consumed(&self) -> u64 {
        self.total_frames
    }

    /// Feeds a planar chunk: one slice per channel, all the same length.
    ///
    /// # Errors
    ///
    /// Returns [`WaveformError::ChannelCountMismatch`] if the chunk has a
    /// different number of channels than the builder, [`WaveformError::RaggedChunk`]
    /// if the slices differ in length, or [`WaveformError::TooLarge`] if the
    /// result would exceed the allocation bound.
    pub fn append(&mut self, channels: &[&[f32]]) -> Result<(), WaveformError> {
        if channels.len() != self.channels {
            return Err(WaveformError::ChannelCountMismatch {
                expected: self.channels,
                supplied: channels.len(),
            });
        }
        let Some(first) = channels.first() else {
            return Err(WaveformError::ZeroChannels);
        };
        let frames = first.len();
        if channels.iter().any(|channel| channel.len() != frames) {
            return Err(WaveformError::RaggedChunk);
        }

        let projected = self
            .total_frames
            .checked_add(frames as u64)
            .ok_or(WaveformError::TooLarge)?;
        let finest = RESOLUTION_LADDER.first().copied().unwrap_or(64);
        // Truncation is intentional: this is a bound check, and a partial tile
        // at the end cannot push the count past the limit.
        #[allow(
            clippy::integer_division,
            reason = "bound check; the fractional tile cannot exceed the limit"
        )]
        let projected_tiles = projected / u64::from(finest);
        if projected_tiles > MAX_TILES_PER_LEVEL_U64 {
            return Err(WaveformError::TooLarge);
        }

        for index in 0..frames {
            for (channel_index, channel) in channels.iter().enumerate() {
                if let (Some(slot), Some(sample)) = (
                    self.frame_scratch.get_mut(channel_index),
                    channel.get(index),
                ) {
                    *slot = *sample;
                }
            }
            for level in &mut self.levels {
                level.push_frame(&self.frame_scratch);
            }
        }

        self.total_frames = projected;
        Ok(())
    }

    /// Completes the waveform, flushing any partial tiles.
    #[must_use]
    pub fn finish(self) -> Waveform {
        Waveform {
            channels: self.channels,
            total_frames: self.total_frames,
            version: GenerationVersion::CURRENT,
            levels: self.levels.into_iter().map(LevelBuilder::finish).collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::float_cmp,
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        reason = "tile summaries of exact inputs are exact; indices are far below any precision limit"
    )]

    use super::*;

    /// A deterministic test signal: a slow sine with a spike every 1000 frames.
    fn signal(frames: usize) -> Vec<f32> {
        (0..frames)
            .map(|index| {
                if index % 1_000 == 500 {
                    0.95
                } else {
                    (index as f32 * 0.001).sin() * 0.4
                }
            })
            .collect()
    }

    fn build(frames: usize, chunk: usize) -> Waveform {
        let samples = signal(frames);
        let mut builder = WaveformBuilder::new(1).unwrap_or_else(|_| unreachable!());
        let mut offset = 0;
        while offset < samples.len() {
            let end = (offset + chunk).min(samples.len());
            if let Some(slice) = samples.get(offset..end) {
                assert!(builder.append(&[slice]).is_ok());
            }
            offset = end;
        }
        builder.finish()
    }

    #[test]
    fn a_waveform_records_what_it_summarised() {
        let waveform = build(10_000, 512);
        assert_eq!(waveform.channels(), 1);
        assert_eq!(waveform.total_frames(), 10_000);
        assert!(waveform.is_current());
        assert_eq!(waveform.levels().count(), RESOLUTION_LADDER.len());
    }

    #[test]
    fn chunking_does_not_change_the_result() {
        // The property that makes an interrupted import resumable rather than
        // restartable: the same audio in different chunk sizes must produce
        // identical tiles.
        let reference = build(50_000, 50_000);
        for chunk in [1_usize, 7, 64, 512, 4_096, 49_999] {
            let chunked = build(50_000, chunk);
            assert_eq!(
                chunked, reference,
                "chunk size {chunk} produced a different waveform"
            );
        }
    }

    #[test]
    fn tiles_report_the_true_extremes_of_their_span() {
        let samples = signal(4_096);
        let mut builder = WaveformBuilder::new(1).unwrap_or_else(|_| unreachable!());
        assert!(builder.append(&[&samples]).is_ok());
        let waveform = builder.finish();

        let Some(finest) = waveform.level(0) else {
            unreachable!("the ladder always has a finest level")
        };
        let span = finest.samples_per_tile() as usize;

        for (index, tile) in finest.channel(0).unwrap_or(&[]).iter().enumerate().take(16) {
            let start = index * span;
            let end = (start + span).min(samples.len());
            let Some(slice) = samples.get(start..end) else {
                continue;
            };
            let expected = Tile::from_samples(slice);
            assert_eq!(tile.min, expected.min, "tile {index} minimum");
            assert_eq!(tile.max, expected.max, "tile {index} maximum");
            assert!((tile.energy - expected.energy).abs() < 1e-6);
        }
    }

    #[test]
    fn every_level_is_exact_rather_than_derived_from_the_one_below() {
        // Coarse tiles built by merging fine tiles would have exact extremes and
        // drifting energy. These are built from the audio, so both are exact.
        let samples = signal(65_536);
        let mut builder = WaveformBuilder::new(1).unwrap_or_else(|_| unreachable!());
        assert!(builder.append(&[&samples]).is_ok());
        let waveform = builder.finish();

        for level_index in 0..RESOLUTION_LADDER.len() {
            let Some(level) = waveform.level(level_index) else {
                continue;
            };
            let span = level.samples_per_tile() as usize;
            let Some(tile) = level.tile(0, 0) else {
                continue;
            };
            let Some(slice) = samples.get(0..span.min(samples.len())) else {
                continue;
            };
            let expected = Tile::from_samples(slice);
            assert_eq!(tile.min, expected.min, "level {level_index} minimum");
            assert_eq!(tile.max, expected.max, "level {level_index} maximum");
            assert!(
                (tile.energy - expected.energy).abs() < 1e-6,
                "level {level_index} energy drifted"
            );
        }
    }

    #[test]
    fn a_partial_tile_at_the_end_is_still_recorded() {
        // A track whose length is not a multiple of the tile span must not lose
        // its last fragment; that fragment is where the outro ends.
        let waveform = build(100, 100);
        let Some(finest) = waveform.level(0) else {
            unreachable!()
        };
        assert_eq!(
            finest.tiles_per_channel(),
            2,
            "100 frames at 64 per tile is one full tile and one partial"
        );
    }

    #[test]
    fn channels_are_summarised_independently() {
        let left = vec![0.5_f32; 512];
        let right = vec![-0.25_f32; 512];
        let mut builder = WaveformBuilder::new(2).unwrap_or_else(|_| unreachable!());
        assert!(builder.append(&[&left, &right]).is_ok());
        let waveform = builder.finish();

        let Some(level) = waveform.level(0) else {
            unreachable!()
        };
        assert_eq!(level.channels(), 2);
        assert_eq!(level.tile(0, 0).map(|t| t.max), Some(0.5));
        assert_eq!(level.tile(1, 0).map(|t| t.min), Some(-0.25));
    }

    #[test]
    fn a_ragged_chunk_is_rejected_rather_than_misaligning_the_channels() {
        let long = vec![0.0_f32; 100];
        let short = vec![0.0_f32; 50];
        let mut builder = WaveformBuilder::new(2).unwrap_or_else(|_| unreachable!());
        assert_eq!(
            builder.append(&[&long, &short]),
            Err(WaveformError::RaggedChunk)
        );
    }

    #[test]
    fn a_channel_count_mismatch_is_rejected() {
        let mono = vec![0.0_f32; 100];
        let mut builder = WaveformBuilder::new(2).unwrap_or_else(|_| unreachable!());
        assert_eq!(
            builder.append(&[&mono]),
            Err(WaveformError::ChannelCountMismatch {
                expected: 2,
                supplied: 1
            })
        );
    }

    #[test]
    fn zero_channels_is_rejected() {
        assert_eq!(
            WaveformBuilder::new(0).err(),
            Some(WaveformError::ZeroChannels)
        );
    }

    #[test]
    fn progress_is_reported_so_an_import_can_resume() {
        let samples = signal(1_000);
        let mut builder = WaveformBuilder::new(1).unwrap_or_else(|_| unreachable!());
        assert_eq!(builder.frames_consumed(), 0);
        if let Some(slice) = samples.get(0..400) {
            assert!(builder.append(&[slice]).is_ok());
        }
        assert_eq!(builder.frames_consumed(), 400);
    }

    #[test]
    fn the_resolution_ladder_is_strictly_ascending_and_aligned() {
        // Each level must be an exact multiple of the one below, so that a zoom
        // changing resolution does not appear to shift the waveform sideways.
        let mut previous = 0_u32;
        for samples in RESOLUTION_LADDER {
            assert!(samples > previous, "the ladder must ascend");
            if previous > 0 {
                assert_eq!(
                    samples % previous,
                    0,
                    "{samples} must be a multiple of {previous}"
                );
            }
            assert!(samples.is_power_of_two());
            previous = samples;
        }
    }

    #[test]
    fn level_selection_never_upscales() {
        let waveform = build(200_000, 4_096);

        // One tile per pixel exactly: the level whose span matches.
        for samples_per_pixel in [64.0_f64, 256.0, 1_024.0, 8_192.0, 65_536.0] {
            let Some(level) = waveform.level_for(samples_per_pixel) else {
                continue;
            };
            assert!(
                f64::from(level.samples_per_tile()) <= samples_per_pixel,
                "chose a {}-sample tile for {samples_per_pixel} samples per pixel",
                level.samples_per_tile()
            );
        }

        // Zoomed in beyond the finest level, the finest is used rather than
        // stretching a coarser one.
        let Some(level) = waveform.level_for(4.0) else {
            unreachable!()
        };
        assert_eq!(level.samples_per_tile(), RESOLUTION_LADDER[0]);
    }

    #[test]
    fn level_selection_picks_the_coarsest_that_still_fills_every_pixel() {
        let waveform = build(200_000, 4_096);
        let Some(level) = waveform.level_for(5_000.0) else {
            unreachable!()
        };
        assert_eq!(
            level.samples_per_tile(),
            1_024,
            "8192 would be wider than a pixel, so 1024 is the coarsest usable"
        );
    }

    #[test]
    fn a_degenerate_zoom_falls_back_to_the_finest_level() {
        let waveform = build(10_000, 1_000);
        for zoom in [0.0_f64, -1.0, f64::NAN, f64::INFINITY] {
            let Some(level) = waveform.level_for(zoom) else {
                continue;
            };
            assert!(level.samples_per_tile() >= RESOLUTION_LADDER[0]);
        }
    }

    #[test]
    fn out_of_range_lookups_return_none_rather_than_panicking() {
        let waveform = build(1_000, 1_000);
        assert!(waveform.level(99).is_none());
        let Some(level) = waveform.level(0) else {
            unreachable!()
        };
        assert!(level.channel(5).is_none());
        assert!(level.tile(0, 999_999).is_none());
        assert!(level.tile(5, 0).is_none());
    }
}
