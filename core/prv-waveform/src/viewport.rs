use core::fmt;

use prv_time::Frames;

use crate::tile::Tile;
use crate::waveform::Waveform;

/// What one pixel column of the waveform display shows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Peak {
    /// Most negative sample in the column.
    pub min: f32,
    /// Most positive sample in the column.
    pub max: f32,
    /// Root-mean-square amplitude of the column.
    pub energy: f32,
}

impl Peak {
    /// A silent column.
    pub const SILENT: Self = Self {
        min: 0.0,
        max: 0.0,
        energy: 0.0,
    };

    /// The larger of the two extremes, in magnitude.
    #[must_use]
    pub fn magnitude(self) -> f32 {
        self.min.abs().max(self.max.abs())
    }
}

impl Default for Peak {
    fn default() -> Self {
        Self::SILENT
    }
}

impl From<Tile> for Peak {
    fn from(tile: Tile) -> Self {
        Self {
            min: tile.min,
            max: tile.max,
            energy: tile.energy,
        }
    }
}

/// The span of audio being displayed, and how wide it is on screen.
///
/// Module Specification #003 requires the renderer never to process regions
/// outside the viewport. This type is how that requirement is expressed: a
/// render call reads only the tiles the viewport covers, so scrolling costs the
/// same whether the track is one minute or ten.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Viewport {
    /// First frame shown.
    pub start: Frames,
    /// One past the last frame shown.
    pub end: Frames,
    /// Width in pixel columns.
    pub pixels: u32,
}

impl Viewport {
    /// Creates a viewport, returning `None` if it is degenerate.
    #[must_use]
    pub fn new(start: Frames, end: Frames, pixels: u32) -> Option<Self> {
        if end <= start || pixels == 0 {
            return None;
        }
        Some(Self { start, end, pixels })
    }

    /// Frames spanned.
    #[must_use]
    pub fn frame_span(self) -> i64 {
        self.end.get().saturating_sub(self.start.get())
    }

    /// Frames represented by one pixel column.
    #[must_use]
    #[allow(
        clippy::cast_precision_loss,
        reason = "a zoom factor is a display value; the widest span is far below the precision limit"
    )]
    pub fn samples_per_pixel(self) -> f64 {
        if self.pixels == 0 {
            return 0.0;
        }
        self.frame_span() as f64 / f64::from(self.pixels)
    }
}

impl fmt::Display for Viewport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}..{} across {} px",
            self.start.get(),
            self.end.get(),
            self.pixels
        )
    }
}

impl Waveform {
    /// Renders one channel of a viewport into a caller-supplied buffer.
    ///
    /// Returns the number of columns written, which is the smaller of the
    /// viewport width and the buffer length.
    ///
    /// # Why the caller supplies the buffer
    ///
    /// A timeline scrolling at 120 frames a second renders this many times a
    /// second. Returning a freshly allocated vector each time would allocate
    /// hundreds of times a second on the interaction path, which Master Prompt
    /// #4 forbids and which shows up as stutter under a finger. The caller keeps
    /// one buffer for the life of the view.
    ///
    /// # Columns beyond the audio
    ///
    /// A viewport may extend past the end of the track — a project is longer
    /// than the material in it, and a user may scroll past the end. Those
    /// columns are written as silence rather than being omitted, so the caller
    /// never has to reason about a partially filled buffer.
    pub fn render(&self, viewport: Viewport, channel: usize, out: &mut [Peak]) -> usize {
        let columns = (viewport.pixels as usize).min(out.len());
        if columns == 0 {
            return 0;
        }

        let Some(level) = self.level_for(viewport.samples_per_pixel()) else {
            return 0;
        };
        let Some(tiles) = level.channel(channel) else {
            // An unknown channel renders as silence rather than as nothing, so
            // a display with more lanes than the file has channels still draws.
            for column in out.iter_mut().take(columns) {
                *column = Peak::SILENT;
            }
            return columns;
        };

        let span = viewport.frame_span();
        let samples_per_tile = i64::from(level.samples_per_tile());

        for (index, column) in out.iter_mut().enumerate().take(columns) {
            // The frame range this column covers. Computed from the column
            // index rather than accumulated, so rounding cannot drift across
            // the width of the view.
            //
            // `index` is bounded by `columns`, itself bounded by the viewport
            // width in `u32`, so the conversion cannot fail.
            let position = i64::try_from(index).unwrap_or(i64::MAX);
            let numerator_start = span.saturating_mul(position);
            let numerator_end = span.saturating_mul(position.saturating_add(1));
            let column_start = viewport
                .start
                .get()
                .saturating_add(divide(numerator_start, i64::from(viewport.pixels)));
            let column_end = viewport
                .start
                .get()
                .saturating_add(divide(numerator_end, i64::from(viewport.pixels)));

            *column = summarise(tiles, column_start, column_end, samples_per_tile);
        }

        columns
    }
}

/// Merges every tile overlapping a frame range into one column.
fn summarise(tiles: &[Tile], start_frame: i64, end_frame: i64, samples_per_tile: i64) -> Peak {
    if samples_per_tile <= 0 || end_frame <= start_frame {
        return Peak::SILENT;
    }
    let first = start_frame.div_euclid(samples_per_tile).max(0);
    // The last tile the range touches. A range ending exactly on a boundary does
    // not touch the tile that begins there.
    let last = (end_frame - 1).div_euclid(samples_per_tile).max(0);

    let Ok(first_index) = usize::try_from(first) else {
        return Peak::SILENT;
    };
    let Ok(last_index) = usize::try_from(last) else {
        return Peak::SILENT;
    };
    if first_index >= tiles.len() {
        return Peak::SILENT;
    }
    let end_index = (last_index + 1).min(tiles.len());
    let Some(covered) = tiles.get(first_index..end_index) else {
        return Peak::SILENT;
    };

    // Folded over the whole slice rather than reduced pairwise. Reducing with
    // `merge` weighted the last tile by a half and the first by an eighth, so
    // the same span drawn in reverse produced a different energy — a display
    // that lies about the music, which is what the energy row exists to avoid.
    Tile::fold(covered).map_or(Peak::SILENT, Peak::from)
}

/// Integer division that floors, so negative viewport positions behave.
fn divide(numerator: i64, denominator: i64) -> i64 {
    if denominator == 0 {
        return 0;
    }
    numerator.div_euclid(denominator)
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::float_cmp,
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        reason = "summaries of exact inputs are exact; indices are far below any precision limit"
    )]

    use super::*;
    use crate::waveform::{WaveformBuilder, RESOLUTION_LADDER};

    fn waveform(frames: usize) -> Waveform {
        let samples: Vec<f32> = (0..frames)
            .map(|index| {
                if index % 5_000 == 2_500 {
                    0.9
                } else {
                    (index as f32 * 0.0005).sin() * 0.3
                }
            })
            .collect();
        let mut builder = WaveformBuilder::new(1).unwrap_or_else(|_| unreachable!());
        assert!(builder.append(&[&samples]).is_ok());
        builder.finish()
    }

    #[test]
    fn a_degenerate_viewport_is_rejected() {
        assert!(Viewport::new(Frames::new(0), Frames::new(0), 100).is_none());
        assert!(Viewport::new(Frames::new(100), Frames::new(50), 100).is_none());
        assert!(Viewport::new(Frames::new(0), Frames::new(100), 0).is_none());
        assert!(Viewport::new(Frames::new(0), Frames::new(100), 10).is_some());
    }

    #[test]
    fn zoom_is_frames_divided_by_pixels() {
        let Some(viewport) = Viewport::new(Frames::new(0), Frames::new(48_000), 1_000) else {
            unreachable!()
        };
        assert!((viewport.samples_per_pixel() - 48.0).abs() < 1e-9);
        assert_eq!(viewport.frame_span(), 48_000);
    }

    #[test]
    fn rendering_writes_exactly_the_requested_columns() {
        let waveform = waveform(100_000);
        let Some(viewport) = Viewport::new(Frames::new(0), Frames::new(100_000), 500) else {
            unreachable!()
        };
        let mut columns = vec![Peak::SILENT; 500];
        assert_eq!(waveform.render(viewport, 0, &mut columns), 500);
    }

    #[test]
    fn a_short_buffer_is_filled_rather_than_overrun() {
        let waveform = waveform(100_000);
        let Some(viewport) = Viewport::new(Frames::new(0), Frames::new(100_000), 500) else {
            unreachable!()
        };
        let mut columns = vec![Peak::SILENT; 100];
        assert_eq!(waveform.render(viewport, 0, &mut columns), 100);
    }

    #[test]
    fn at_one_tile_per_pixel_a_column_is_its_tile() {
        // The identity case: no aggregation, so each column must be exactly the
        // tile beneath it.
        let waveform = waveform(64_000);
        let finest = RESOLUTION_LADDER[0];
        let pixels = 100_u32;
        let span = i64::from(finest) * i64::from(pixels);
        let Some(viewport) = Viewport::new(Frames::new(0), Frames::new(span), pixels) else {
            unreachable!()
        };

        let mut columns = vec![Peak::SILENT; pixels as usize];
        assert_eq!(waveform.render(viewport, 0, &mut columns), pixels as usize);

        let Some(level) = waveform.level(0) else {
            unreachable!()
        };
        for (index, column) in columns.iter().enumerate() {
            let Some(tile) = level.tile(0, index) else {
                continue;
            };
            assert_eq!(column.min, tile.min, "column {index} minimum");
            assert_eq!(column.max, tile.max, "column {index} maximum");
        }
    }

    #[test]
    fn an_aggregated_column_reports_the_extremes_of_everything_it_covers() {
        // The property that matters when zoomed out: a transient inside a
        // column must still be visible. A column that averaged its tiles would
        // hide it, and the user would cut in the wrong place.
        let waveform = waveform(200_000);
        let Some(viewport) = Viewport::new(Frames::new(0), Frames::new(200_000), 20) else {
            unreachable!()
        };
        let mut columns = vec![Peak::SILENT; 20];
        assert_eq!(waveform.render(viewport, 0, &mut columns), 20);

        let loudest = columns
            .iter()
            .fold(0.0_f32, |peak, column| peak.max(column.magnitude()));
        assert!(
            loudest > 0.85,
            "the 0.9 transients must survive aggregation, got {loudest}"
        );
    }

    #[test]
    fn columns_beyond_the_audio_are_silent_rather_than_missing() {
        let waveform = waveform(10_000);
        let Some(viewport) = Viewport::new(Frames::new(0), Frames::new(100_000), 50) else {
            unreachable!()
        };
        let mut columns = vec![
            Peak {
                min: -1.0,
                max: 1.0,
                energy: 1.0,
            };
            50
        ];
        assert_eq!(waveform.render(viewport, 0, &mut columns), 50);

        // The last columns lie past the end of a 10 000-frame file.
        let Some(last) = columns.last() else {
            unreachable!()
        };
        assert_eq!(*last, Peak::SILENT);
    }

    #[test]
    fn a_viewport_starting_before_the_audio_is_handled() {
        let waveform = waveform(10_000);
        let Some(viewport) = Viewport::new(Frames::new(-5_000), Frames::new(5_000), 40) else {
            unreachable!()
        };
        let mut columns = vec![Peak::SILENT; 40];
        assert_eq!(waveform.render(viewport, 0, &mut columns), 40);
        // The second half covers real audio and must not be silent.
        let later = columns.get(30).copied().unwrap_or(Peak::SILENT);
        assert!(later.magnitude() > 0.0);
    }

    #[test]
    fn an_unknown_channel_renders_as_silence_rather_than_nothing() {
        let waveform = waveform(10_000);
        let Some(viewport) = Viewport::new(Frames::new(0), Frames::new(10_000), 10) else {
            unreachable!()
        };
        let mut columns = vec![
            Peak {
                min: -1.0,
                max: 1.0,
                energy: 1.0,
            };
            10
        ];
        assert_eq!(waveform.render(viewport, 7, &mut columns), 10);
        assert!(columns.iter().all(|column| *column == Peak::SILENT));
    }

    #[test]
    fn column_boundaries_do_not_drift_across_the_view() {
        // Column positions are computed from the index rather than accumulated,
        // so rounding cannot compound. With a span that does not divide evenly,
        // an accumulating implementation would end up a pixel or more out by the
        // right-hand edge.
        let waveform = waveform(100_000);
        let Some(viewport) = Viewport::new(Frames::new(0), Frames::new(99_997), 997) else {
            unreachable!()
        };
        let mut columns = vec![Peak::SILENT; 997];
        assert_eq!(waveform.render(viewport, 0, &mut columns), 997);

        // The final column must still cover the end of the span, not fall short.
        let Some(last) = columns.last() else {
            unreachable!()
        };
        assert!(last.magnitude() > 0.0, "the last column fell off the audio");
    }

    #[test]
    fn a_zero_length_buffer_writes_nothing() {
        let waveform = waveform(10_000);
        let Some(viewport) = Viewport::new(Frames::new(0), Frames::new(10_000), 10) else {
            unreachable!()
        };
        let mut columns: [Peak; 0] = [];
        assert_eq!(waveform.render(viewport, 0, &mut columns), 0);
    }

    #[test]
    fn a_peak_converts_from_a_tile_without_losing_anything() {
        let tile = Tile {
            min: -0.4,
            max: 0.6,
            energy: 0.3,
        };
        let peak = Peak::from(tile);
        assert_eq!(peak.min, tile.min);
        assert_eq!(peak.max, tile.max);
        assert_eq!(peak.energy, tile.energy);
        assert_eq!(peak.magnitude(), 0.6);
    }
}
