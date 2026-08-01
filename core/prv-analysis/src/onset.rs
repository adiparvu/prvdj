//! Onsets: where something in the music starts.
//!
//! # What is being measured
//!
//! Everything rhythmic downstream — tempo, beats, downbeats, the cue points a
//! DJ actually uses — rests on one curve: a number per analysis frame saying
//! how much the spectrum just changed. Master Prompt #20 requires beat
//! detection accurate enough to drive a professional beat grid, and the quality
//! of that grid is decided here rather than in the beat tracker. A tracker
//! cannot recover a beat that the novelty curve did not show.
//!
//! # Why not amplitude
//!
//! The naive detector watches the signal envelope for a rise. It works on a
//! drum machine and fails on everything else. A bassline note change, a chord
//! stab under a sustained pad, a hi-hat over a loud kick — all are onsets, and
//! none of them necessarily raises the overall level. Modern masters compress
//! to within a few decibels of full scale from beginning to end, so envelope
//! detection is being asked to find events in a signal deliberately engineered
//! to have none.
//!
//! Spectral flux asks a better question: how much energy appeared in bins that
//! did not have it a moment ago. A new note is new energy at new frequencies
//! even when the total level is unchanged, so it registers.
//!
//! # The three refinements that matter
//!
//! **Half-wave rectification.** Only increases count. Energy leaving a bin is a
//! note ending, and note endings are not onsets — including them would put a
//! spurious peak after every event, which the tempo estimator would read as
//! double time.
//!
//! **Logarithmic compression.** Applied before differencing, so the curve
//! measures *relative* change. Without it a quiet intro contributes almost
//! nothing to the tempo estimate compared with a loud drop, and a track that
//! changes dynamics is analysed as though only its loudest section existed.
//!
//! **Local mean subtraction.** The curve is compared against its own recent
//! average rather than a fixed threshold, so a dense passage does not drown a
//! sparse one. This is the difference between a beat grid that survives a
//! breakdown and one that loses the beat at the quiet part and never recovers.

use prv_time::SampleRate;

use crate::error::AnalysisError;
use crate::num::{count_to_f64, narrow, ratio, round_to_count};
use crate::spectrum::{SpectrumFrame, Stft};

/// The compression constant applied before differencing.
///
/// `log(1 + γ·x)` behaves like `γ·x` for small values and like `log x` for
/// large ones. With γ at 1000 the knee sits around −60 dBFS: everything above
/// it is compared logarithmically, so a change is measured as a ratio, and
/// everything below it — noise floor, dither, tape hiss — is compressed toward
/// zero rather than amplified into apparent events.
const COMPRESSION: f64 = 1_000.0;

/// The half-width of the local mean window, in seconds.
///
/// A hundred milliseconds either side. Long enough to average across a bar's
/// worth of texture at any usable tempo, short enough to follow a drop within
/// one beat rather than a phrase.
const LOCAL_MEAN_SECONDS: f64 = 0.1;

/// The upper edge of the band whose energy is tracked for downbeat work.
///
/// Downbeats are carried by the kick and the bass note that lands with it.
/// Everything above roughly 200 Hz is snare, hats and melody, which land on
/// every beat and therefore say nothing about which beat is the first.
const LOW_BAND_HZ: f64 = 200.0;

/// How far above the curve's own background a peak must stand to count.
///
/// One and a half standard deviations. Chosen so that the detector reports the
/// events a listener would call events, rather than every ripple: at this
/// setting a four-to-the-floor record gives its kicks and its snares and not
/// the hi-hat between them, which is what a cue-point suggestion should offer.
/// Beat tracking does not use this threshold at all — it consumes the whole
/// curve — so raising or lowering it changes what is *suggested*, never what is
/// *measured*.
const PEAK_SIGMAS: f64 = 1.5;

/// The smallest mean logarithmic rise per bin that counts as an event.
///
/// About 0.4 decibels averaged across the whole spectrum. Sustained material
/// with no events produces around a hundredth of this, and a percussive attack
/// produces a hundred times it, so the floor sits in a wide empty gap rather
/// than on a boundary that real music straddles.
const MINIMUM_RISE: f64 = 0.05;

/// A novelty curve and the low-band energy that accompanies it.
///
/// The two travel together because they are produced by one pass over the
/// spectrum and consumed by one stage — the beat tracker uses novelty for beat
/// positions and low-band energy for which of those beats begins the bar.
/// Separating them would mean transforming the track twice.
#[derive(Debug, Clone)]
pub struct NoveltyCurve {
    values: Vec<f32>,
    low_band: Vec<f32>,
    hop: usize,
    sample_rate: SampleRate,
}

impl NoveltyCurve {
    /// Computes the novelty curve for a mono signal.
    ///
    /// # Errors
    ///
    /// Propagates transform errors. A signal too short to produce a single
    /// analysis frame yields an empty curve rather than an error; the stages
    /// that need a minimum duration enforce their own.
    pub fn compute(
        stft: &mut Stft,
        samples: &[f32],
        sample_rate: SampleRate,
    ) -> Result<Self, AnalysisError> {
        let frames = stft.frame_count(samples.len());
        let mut previous = vec![0.0_f64; stft.bins()];
        let mut values = Vec::with_capacity(frames);
        let mut low_band = Vec::with_capacity(frames);
        let mut low_band_bins = 0_usize;

        stft.analyse(samples, sample_rate, |frame: &SpectrumFrame<'_>| {
            if frame.index() == 0 {
                low_band_bins = frame.bin_for_frequency(LOW_BAND_HZ).saturating_add(1);
            }

            let mut flux = 0.0_f64;
            let mut low_energy = 0.0_f64;
            for (bin, (&magnitude, stored)) in frame
                .magnitude()
                .iter()
                .zip(previous.iter_mut())
                .enumerate()
            {
                let compressed = (1.0 + COMPRESSION * magnitude).ln();
                let rise = compressed - *stored;
                if rise > 0.0 {
                    flux += rise;
                }
                *stored = compressed;
                if bin < low_band_bins {
                    low_energy += magnitude * magnitude;
                }
            }

            // Divided by the bin count, so the value is the mean logarithmic
            // rise per bin rather than a sum whose scale depends on the window
            // size. That makes the number comparable between analyses run at
            // different resolutions, and — because it is a logarithm — roughly
            // comparable between tracks, which is what lets the onset threshold
            // have a floor with a physical meaning rather than a tuned one.
            let bins = frame.bins();
            values.push(narrow(if bins == 0 {
                0.0
            } else {
                flux / count_to_f64(bins)
            }));
            low_band.push(narrow(low_energy.sqrt()));
        })?;

        Ok(Self {
            values,
            low_band,
            hop: stft.hop(),
            sample_rate,
        })
    }

    /// The raw novelty values, one per analysis frame.
    #[must_use]
    pub fn values(&self) -> &[f32] {
        &self.values
    }

    /// The low-band energy, one per analysis frame.
    #[must_use]
    pub fn low_band(&self) -> &[f32] {
        &self.low_band
    }

    /// The number of frames in the curve.
    #[must_use]
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// Whether the curve is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// The hop the curve was computed with, in samples.
    #[must_use]
    pub const fn hop(&self) -> usize {
        self.hop
    }

    /// The sample rate the curve was computed at.
    #[must_use]
    pub const fn sample_rate(&self) -> SampleRate {
        self.sample_rate
    }

    /// The curve's own sample rate, in values per second.
    #[must_use]
    pub fn frame_rate(&self) -> f64 {
        f64::from(self.sample_rate.hz()) / count_to_f64(self.hop)
    }

    /// The sample offset a curve index corresponds to.
    ///
    /// The analysis is centred, so this is exactly `index × hop`. The window
    /// size does not enter: the frame is centred on this sample, not started
    /// from it.
    #[must_use]
    pub const fn sample_at(&self, index: usize) -> usize {
        index * self.hop
    }

    /// The curve index nearest a sample offset.
    #[must_use]
    pub fn index_at(&self, sample: usize) -> usize {
        round_to_count(ratio(sample, self.hop)).min(self.len().saturating_sub(1))
    }

    /// Returns the curve with its local mean subtracted and rectified.
    ///
    /// This is the form every rhythmic stage consumes. Producing it on demand
    /// rather than storing it keeps the raw curve available, which matters
    /// because the raw values are what an explanation shows a user when it says
    /// how strong an onset was.
    #[must_use]
    pub fn enhanced(&self) -> Vec<f32> {
        let half_width = round_to_count(LOCAL_MEAN_SECONDS * self.frame_rate()).max(1);
        let mut output = Vec::with_capacity(self.values.len());

        // A running sum rather than a window recomputed per position. The
        // difference is linear against quadratic; on a ten-minute track with a
        // 0.1-second window that is fifty thousand additions instead of half a
        // billion.
        let mut sum = 0.0_f64;
        let mut count = 0_usize;
        let mut lower = 0_usize;
        let mut upper = 0_usize;

        for index in 0..self.values.len() {
            let want_lower = index.saturating_sub(half_width);
            let want_upper = (index + half_width + 1).min(self.values.len());

            while upper < want_upper {
                if let Some(&value) = self.values.get(upper) {
                    sum += f64::from(value);
                    count += 1;
                }
                upper += 1;
            }
            while lower < want_lower {
                if let Some(&value) = self.values.get(lower) {
                    sum -= f64::from(value);
                    count = count.saturating_sub(1);
                }
                lower += 1;
            }

            let mean = if count == 0 {
                0.0
            } else {
                sum / count_to_f64(count)
            };
            let value = self.values.get(index).copied().unwrap_or(0.0);
            output.push(narrow((f64::from(value) - mean).max(0.0)));
        }

        output
    }

    /// Picks discrete onsets from the curve.
    ///
    /// An onset is a local maximum of the enhanced curve that clears two
    /// independent thresholds and is not within `minimum_separation_seconds` of
    /// a stronger one.
    ///
    /// The first threshold is **statistical**: the value must stand out from
    /// this track's own background by [`PEAK_SIGMAS`] standard deviations. That
    /// is what makes the detector work equally on a sparse dub record and a
    /// dense drum and bass one, where an absolute threshold would find either
    /// nothing or everything.
    ///
    /// The second is **physical**: the value must represent at least
    /// [`MINIMUM_RISE`] of mean logarithmic increase per bin. A statistical
    /// threshold alone has no floor — on a sustained pad with no events at all
    /// it faithfully reports the loudest ripples in the noise, because
    /// something is always 1.5 deviations above the mean. The floor is what
    /// lets the detector say "nothing happened here", and the logarithmic
    /// compression is what makes such a floor meaningful across masters that
    /// differ by twenty decibels.
    #[must_use]
    pub fn onsets(&self, minimum_separation_seconds: f64) -> Vec<Onset> {
        let enhanced = self.enhanced();
        if enhanced.is_empty() {
            return Vec::new();
        }

        let count = count_to_f64(enhanced.len());
        let mean = enhanced.iter().map(|&v| f64::from(v)).sum::<f64>() / count;
        let variance = enhanced
            .iter()
            .map(|&v| {
                let centred = f64::from(v) - mean;
                centred * centred
            })
            .sum::<f64>()
            / count;
        let threshold = (mean + PEAK_SIGMAS * variance.sqrt()).max(MINIMUM_RISE);
        let separation = round_to_count(minimum_separation_seconds * self.frame_rate()).max(1);

        let mut picked: Vec<Onset> = Vec::new();
        for index in 0..enhanced.len() {
            // The first and last frames are candidates like any other. The
            // analysis is centred, so frame zero is centred on sample zero, and
            // a track that opens on a downbeat has a real event there. Skipping
            // the endpoints — which the usual three-point peak test does
            // implicitly — would lose exactly that downbeat.
            let before = index
                .checked_sub(1)
                .and_then(|previous| enhanced.get(previous))
                .copied()
                .unwrap_or(0.0);
            let after = enhanced.get(index + 1).copied().unwrap_or(0.0);
            let Some(&value) = enhanced.get(index) else {
                continue;
            };
            if f64::from(value) < threshold || value < before || value < after {
                continue;
            }
            // Ties between neighbouring equal values are broken toward the
            // earlier one, which is the correct choice for an attack: the event
            // began at the first frame that showed it.
            if let Some(last) = picked.last_mut() {
                if index.saturating_sub(last.frame) < separation {
                    if value > last.strength {
                        last.frame = index;
                        last.sample = self.sample_at(index);
                        last.strength = value;
                    }
                    continue;
                }
            }
            picked.push(Onset {
                frame: index,
                sample: self.sample_at(index),
                strength: value,
            });
        }

        picked
    }
}

/// A detected onset.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Onset {
    frame: usize,
    sample: usize,
    strength: f32,
}

impl Onset {
    /// The novelty-curve index the onset was found at.
    #[must_use]
    pub const fn frame(&self) -> usize {
        self.frame
    }

    /// The sample offset of the onset.
    #[must_use]
    pub const fn sample(&self) -> usize {
        self.sample
    }

    /// The enhanced novelty value at the onset.
    ///
    /// Relative to the rest of the track, not an absolute level. Useful for
    /// ranking candidate cue points against each other; meaningless compared
    /// across tracks.
    #[must_use]
    pub const fn strength(&self) -> f32 {
        self.strength
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::float_cmp,
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a test that cannot build its own fixture should fail loudly, and the signal \
                  generators here work in exact, bounded quantities"
    )]

    use super::*;
    use crate::testing::{click_track, mean as mean_of, samples, tone};

    #[test]
    fn a_click_track_produces_one_onset_per_click() {
        let rate = SampleRate::HZ_44100;
        // 120 BPM: half a second between clicks.
        let period = 22_050_usize;
        let signal = click_track(period, 16, rate);

        let mut stft = Stft::for_rhythm().expect("valid");
        let curve = NoveltyCurve::compute(&mut stft, &signal, rate).expect("valid");
        let onsets = curve.onsets(0.05);

        assert_eq!(
            onsets.len(),
            16,
            "expected one onset per click, got {}",
            onsets.len()
        );

        // Two separate claims, because the detector has one systematic error
        // and it is worth being explicit about which.
        //
        // A difference detector fires when a transient *enters* the analysis
        // window, so it is never late and is early by at most half a window.
        // That error is a constant offset on the whole grid, which is why the
        // rhythm window is short and why the beat tracker fits an origin across
        // every beat rather than trusting any one of them.
        for (index, onset) in onsets.iter().enumerate() {
            let expected = index * period;
            assert!(
                onset.sample() <= expected + stft.hop(),
                "click {index} was detected late, at {} rather than {expected}",
                onset.sample()
            );
            assert!(
                expected.saturating_sub(onset.sample()) <= stft.window_size() >> 1,
                "click {index} at {} is more than half a window before {expected}",
                onset.sample()
            );
        }

        // The spacings, which is what tempo estimation actually consumes, carry
        // no systematic error at all: a constant offset cancels in a
        // difference. The very first spacing is excluded, because the click at
        // sample zero has no room to be detected early and so does not share
        // the offset the rest do.
        for pair in onsets.windows(2).skip(1) {
            let (Some(first), Some(second)) = (pair.first(), pair.get(1)) else {
                continue;
            };
            let spacing = second.sample() - first.sample();
            assert!(
                spacing.abs_diff(period) <= stft.hop(),
                "spacing {spacing} differs from the period {period} by more than one hop"
            );
        }
    }

    #[test]
    fn silence_produces_no_onsets() {
        let rate = SampleRate::HZ_44100;
        let mut stft = Stft::for_rhythm().expect("valid");
        let curve = NoveltyCurve::compute(&mut stft, &vec![0.0_f32; 44_100], rate).expect("valid");
        assert!(curve.onsets(0.05).is_empty());
    }

    #[test]
    fn a_sustained_tone_has_no_onsets_in_its_interior() {
        // The property that separates a spectral detector from an envelope
        // detector: a sustained note is an event once, not continuously.
        //
        // The tone does have two real events — it starts abruptly and it stops
        // abruptly, and cutting a tone dead is audibly a click. Asserting "one
        // onset, at the start" would have been asserting something false about
        // the signal; what is actually claimed is that nothing happens *while
        // the tone sustains*.
        let rate = SampleRate::HZ_44100;
        let length = samples(4.0, rate);
        let signal = tone(220.0, length, 0.5, rate);

        let mut stft = Stft::for_rhythm().expect("valid");
        let curve = NoveltyCurve::compute(&mut stft, &signal, rate).expect("valid");
        let onsets = curve.onsets(0.05);

        let interior: Vec<usize> = onsets
            .iter()
            .map(Onset::sample)
            .filter(|&position| {
                position > samples(0.2, rate) && position < length - samples(0.2, rate)
            })
            .collect();
        assert!(
            interior.is_empty(),
            "a sustained tone produced onsets at {interior:?}"
        );
        assert!(
            onsets
                .iter()
                .any(|onset| onset.sample() <= samples(0.2, rate)),
            "the tone's own beginning was not detected"
        );
    }

    #[test]
    fn the_curve_survives_a_twenty_decibel_level_change() {
        // The reason for logarithmic compression. A quiet intro followed by a
        // loud drop must contribute onsets from both halves; without
        // compression the loud half dominates the mean and the quiet half falls
        // below the threshold entirely.
        let rate = SampleRate::HZ_44100;
        let period = 22_050_usize;
        let mut signal = click_track(period, 16, rate);
        for (index, sample) in signal.iter_mut().enumerate() {
            if index < period * 8 {
                *sample *= 0.1;
            }
        }

        let mut stft = Stft::for_rhythm().expect("valid");
        let curve = NoveltyCurve::compute(&mut stft, &signal, rate).expect("valid");
        let onsets = curve.onsets(0.05);

        let quiet = onsets
            .iter()
            .filter(|onset| onset.sample() < period * 8)
            .count();
        let loud = onsets
            .iter()
            .filter(|onset| onset.sample() >= period * 8)
            .count();
        assert!(
            quiet >= 7 && loud >= 7,
            "quiet half gave {quiet} onsets and loud half {loud}; the level change should not \
             have cost either of them"
        );
    }

    #[test]
    fn low_band_energy_follows_the_bass_and_ignores_the_top() {
        let rate = SampleRate::HZ_44100;
        let length = samples(1.0, rate);
        let low = tone(60.0, length, 0.5, rate);
        let high = tone(6_000.0, length, 0.5, rate);

        let mut stft = Stft::for_rhythm().expect("valid");
        let low_curve = NoveltyCurve::compute(&mut stft, &low, rate).expect("valid");
        let high_curve = NoveltyCurve::compute(&mut stft, &high, rate).expect("valid");

        let low_energy = mean_of(low_curve.low_band());
        let high_energy = mean_of(high_curve.low_band());
        assert!(
            low_energy > high_energy * 100.0,
            "the low band read {low_energy} for a 60 Hz tone and {high_energy} for 6 kHz; it is \
             not selective"
        );
    }

    #[test]
    fn index_and_sample_round_trip() {
        let rate = SampleRate::HZ_44100;
        let mut stft = Stft::for_rhythm().expect("valid");
        let curve = NoveltyCurve::compute(&mut stft, &vec![0.0_f32; 100_000], rate).expect("valid");
        for index in [0_usize, 1, 17, 50] {
            assert_eq!(curve.index_at(curve.sample_at(index)), index);
        }
    }
}
