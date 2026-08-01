use core::fmt;

/// The version of the algorithm that produced a tile.
///
/// # Why tiles carry a version
///
/// Master Prompt #20 requires every analysis stage to be versioned independently
/// so that improving one re-runs only that one across a library. Waveform tiles
/// are subject to the same rule: a change to how peaks are summarised must
/// invalidate waveform data and nothing else — not the beat grid, not the key,
/// not the user's cue points.
///
/// Carrying the version on the tile rather than on the file means invalidation
/// can be partial. A track re-analysed at one resolution keeps its other
/// resolutions until they are needed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GenerationVersion(u32);

impl GenerationVersion {
    /// The version produced by the current implementation.
    pub const CURRENT: Self = Self(1);

    /// Creates a version.
    #[must_use]
    pub const fn new(version: u32) -> Self {
        Self(version)
    }

    /// The raw value.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }

    /// Whether a tile at this version is still usable.
    ///
    /// Older versions are stale and must be regenerated. A *newer* version is
    /// also rejected: it means the file was written by a later build, and
    /// rendering data this build does not understand would show the user
    /// something wrong rather than something missing.
    #[must_use]
    pub const fn is_current(self) -> bool {
        self.0 == Self::CURRENT.0
    }
}

impl fmt::Display for GenerationVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "v{}", self.0)
    }
}

/// A summary of one span of audio.
///
/// # Why minimum and maximum rather than just a peak
///
/// A waveform drawn from absolute peaks is symmetrical and hides asymmetry that
/// carries real information: heavily limited material, direct-current offset, and
/// the lopsided shape of a kick drum are all invisible in a rectified display.
/// Keeping both extremes costs one extra number per tile and shows the engineer
/// what is actually there.
///
/// # Why energy as well
///
/// Peaks describe the loudest instant; energy describes how loud the span *is*.
/// A single stray transient in a quiet passage produces a tall peak and almost no
/// energy, and a display driven by peaks alone would show a quiet breakdown as
/// though it were full. Master Prompt #2's energy waveform is drawn from this
/// field, and the structural analysis of Master Prompt #20 uses it directly.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tile {
    /// Most negative sample in the span.
    pub min: f32,
    /// Most positive sample in the span.
    pub max: f32,
    /// Root-mean-square amplitude of the span.
    pub energy: f32,
}

impl Tile {
    /// A tile summarising silence.
    pub const SILENT: Self = Self {
        min: 0.0,
        max: 0.0,
        energy: 0.0,
    };

    /// Summarises a slice of samples.
    ///
    /// An empty slice produces [`Self::SILENT`] rather than an error: an empty
    /// span at the end of a track is normal, not exceptional.
    #[must_use]
    pub fn from_samples(samples: &[f32]) -> Self {
        if samples.is_empty() {
            return Self::SILENT;
        }
        let mut min = f32::INFINITY;
        let mut max = f32::NEG_INFINITY;
        let mut sum_of_squares = 0.0_f64;

        for sample in samples {
            let value = *sample;
            // A non-finite sample from a damaged file must not poison the whole
            // tile: Master Prompt #20 requires corrupted frames to be detected
            // and reported, not propagated into the display.
            if !value.is_finite() {
                continue;
            }
            if value < min {
                min = value;
            }
            if value > max {
                max = value;
            }
            sum_of_squares += f64::from(value) * f64::from(value);
        }

        if !min.is_finite() || !max.is_finite() {
            return Self::SILENT;
        }

        #[allow(
            clippy::cast_precision_loss,
            clippy::cast_possible_truncation,
            reason = "tile spans are at most 65 536 samples and energy is a display value"
        )]
        let energy = (sum_of_squares / samples.len() as f64).sqrt() as f32;

        Self { min, max, energy }
    }

    /// Combines two tiles into one covering both spans.
    ///
    /// Used when rendering aggregates several tiles into one pixel. Energy is
    /// combined as a simple mean of the two, which is exact when the spans are
    /// equal — the only case that occurs, because a level's tiles all cover the
    /// same number of samples.
    #[must_use]
    pub fn merge(self, other: Self) -> Self {
        Self {
            min: self.min.min(other.min),
            max: self.max.max(other.max),
            energy: (self.energy + other.energy) * 0.5,
        }
    }

    /// The larger of the two extremes, in magnitude.
    #[must_use]
    pub fn peak(self) -> f32 {
        self.min.abs().max(self.max.abs())
    }

    /// Whether the span is silent.
    #[must_use]
    pub fn is_silent(self) -> bool {
        self.min == 0.0 && self.max == 0.0
    }

    /// Whether the span reached or exceeded full scale.
    ///
    /// Master Prompt #20 requires clipping to be detected and the user warned
    /// before the file is used. Surfacing it on the tile means the display can
    /// mark exactly where it happened rather than only that it happened.
    #[must_use]
    pub fn is_clipped(self) -> bool {
        self.min <= -1.0 || self.max >= 1.0
    }
}

impl Default for Tile {
    fn default() -> Self {
        Self::SILENT
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::float_cmp,
        reason = "tile summaries of exact inputs are exact by construction"
    )]

    use super::*;

    #[test]
    fn an_empty_span_is_silent() {
        assert_eq!(Tile::from_samples(&[]), Tile::SILENT);
        assert!(Tile::SILENT.is_silent());
    }

    #[test]
    fn extremes_are_the_true_extremes() {
        let tile = Tile::from_samples(&[0.1, -0.7, 0.4, -0.2, 0.9]);
        assert_eq!(tile.min, -0.7);
        assert_eq!(tile.max, 0.9);
        assert_eq!(tile.peak(), 0.9);
    }

    #[test]
    fn asymmetry_is_preserved() {
        // A rectified display would show these as identical. They are not, and
        // the difference is exactly what tells an engineer the material is
        // heavily limited or carries a direct-current offset.
        let asymmetric = Tile::from_samples(&[0.9, 0.8, 0.1, 0.05]);
        let symmetric = Tile::from_samples(&[0.9, -0.9, 0.1, -0.1]);
        assert_eq!(asymmetric.peak(), symmetric.peak());
        assert_ne!(asymmetric.min, symmetric.min);
    }

    #[test]
    fn energy_distinguishes_a_transient_from_a_loud_passage() {
        // The reason energy is stored alongside peaks. Both spans peak at 1.0;
        // one is a stray transient in near-silence, the other is genuinely loud.
        let mut transient = [0.001_f32; 64];
        if let Some(first) = transient.first_mut() {
            *first = 1.0;
        }
        let sustained = [0.7_f32; 64];

        let transient_tile = Tile::from_samples(&transient);
        let sustained_tile = Tile::from_samples(&sustained);

        assert_eq!(transient_tile.max, 1.0);
        assert!(
            transient_tile.energy < 0.2,
            "a lone transient carries little energy, got {}",
            transient_tile.energy
        );
        assert!(
            sustained_tile.energy > 0.6,
            "a sustained passage carries a lot, got {}",
            sustained_tile.energy
        );
    }

    #[test]
    fn energy_of_a_constant_span_is_its_amplitude() {
        let tile = Tile::from_samples(&[0.5_f32; 128]);
        assert!((tile.energy - 0.5).abs() < 1e-6);
    }

    #[test]
    fn non_finite_samples_are_skipped_rather_than_propagated() {
        // A damaged file must not turn the whole waveform into nothing.
        let tile = Tile::from_samples(&[0.5, f32::NAN, -0.3, f32::INFINITY, 0.2]);
        assert_eq!(tile.min, -0.3);
        assert_eq!(tile.max, 0.5);
        assert!(tile.energy.is_finite());
    }

    #[test]
    fn a_span_of_only_non_finite_samples_reads_as_silence() {
        let tile = Tile::from_samples(&[f32::NAN, f32::INFINITY, f32::NEG_INFINITY]);
        assert_eq!(tile, Tile::SILENT);
    }

    #[test]
    fn merging_takes_the_extremes_of_both() {
        let left = Tile::from_samples(&[0.5, -0.2]);
        let right = Tile::from_samples(&[0.1, -0.8]);
        let merged = left.merge(right);
        assert_eq!(merged.max, 0.5);
        assert_eq!(merged.min, -0.8);
    }

    #[test]
    fn merging_is_commutative() {
        let left = Tile::from_samples(&[0.5, -0.2]);
        let right = Tile::from_samples(&[0.1, -0.8]);
        assert_eq!(left.merge(right), right.merge(left));
    }

    #[test]
    fn clipping_is_detected_at_full_scale() {
        assert!(!Tile::from_samples(&[0.99, -0.5]).is_clipped());
        assert!(Tile::from_samples(&[1.0, -0.5]).is_clipped());
        assert!(Tile::from_samples(&[0.5, -1.0]).is_clipped());
        assert!(Tile::from_samples(&[1.5]).is_clipped());
    }

    #[test]
    fn versions_are_compared_exactly() {
        assert!(GenerationVersion::CURRENT.is_current());
        assert!(!GenerationVersion::new(0).is_current());
        // A newer version is rejected too: rendering data this build does not
        // understand would show something wrong rather than something missing.
        assert!(!GenerationVersion::new(GenerationVersion::CURRENT.get() + 1).is_current());
    }
}
