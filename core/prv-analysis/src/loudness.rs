//! Loudness, to ITU-R BS.1770.
//!
//! # Why a standard rather than a level meter
//!
//! Master Prompt #3A requires the master chain to report loudness, and Master
//! Prompt #20 requires it as part of the track profile. Both need a number that
//! means the same thing for a 1994 house record and a 2024 one, because the use
//! is comparison: matching two tracks so a transition does not jump in level,
//! and telling a user their set is louder than a streaming platform will allow.
//!
//! Peak level cannot do that — modern masters all peak at the same place — and
//! plain root-mean-square cannot either, because the ear is far less sensitive
//! to a 40 Hz rumble than to a 3 kHz snare at the same energy. BS.1770 exists
//! precisely to give one number that tracks perceived loudness, and it is what
//! every platform a user might deliver to has standardised on.
//!
//! # What is implemented
//!
//! - **K-weighting**: a high-shelf approximating the acoustic effect of a head,
//!   then a high-pass removing content too low to contribute to loudness.
//! - **Gated integrated loudness**: mean-square over 400-millisecond blocks,
//!   with an absolute gate at −70 LUFS and a relative gate 10 LU below the
//!   ungated mean. The gates are the part that makes the measure usable on
//!   music: without them a track with a long quiet intro measures quieter than
//!   it sounds, because silence is averaged in as though it were music.
//! - **Loudness range**: the spread between the tenth and ninety-fifth
//!   percentiles of short-term loudness, which is what distinguishes a
//!   dynamic recording from a flat one.
//! - **True peak**: peak measured after four-times oversampling, because a
//!   signal that never exceeds full scale between samples can still exceed it
//!   between them, and a converter reconstructing it will clip.

use prv_time::SampleRate;

use crate::error::AnalysisError;
use crate::num::{count_to_f64, round_to_count};

/// The absolute gate, in LUFS.
///
/// Content below this is not music being measured; it is silence, room tone or
/// a fade tail, and averaging it in would drag every measurement down.
const ABSOLUTE_GATE_LUFS: f64 = -70.0;

/// The relative gate, in loudness units below the ungated mean.
const RELATIVE_GATE_LU: f64 = -10.0;

/// The length of a measurement block, in seconds.
const BLOCK_SECONDS: f64 = 0.4;

/// The overlap between successive blocks, as a fraction.
///
/// Seventy-five per cent, which the standard specifies. The overlap is what
/// keeps the gating decision from depending on where the block boundaries
/// happened to fall.
const BLOCK_OVERLAP: f64 = 0.75;

/// The length of a short-term window, in seconds.
const SHORT_TERM_SECONDS: f64 = 3.0;

/// The offset that turns mean square into LUFS.
///
/// From the standard. A full-scale sine reads −3.01 LUFS with it, which is the
/// definition the number is anchored to.
const LUFS_OFFSET: f64 = -0.691;

/// The oversampling factor used for true-peak measurement.
const TRUE_PEAK_OVERSAMPLING: usize = 4;

/// The lowest loudness reported, in LUFS.
///
/// Digital silence has no loudness at all, and the logarithm of zero is not a
/// number. Reporting a floor rather than an infinity keeps every consumer from
/// having to special-case it, and this value is far below anything audible.
pub const SILENCE_LUFS: f64 = -120.0;

/// A measured loudness.
#[derive(Debug, Clone, PartialEq)]
pub struct Loudness {
    integrated: f64,
    range: f64,
    peak: f64,
    true_peak: f64,
    short_term: Vec<f32>,
    short_term_hop: usize,
}

impl Loudness {
    /// The gated integrated loudness of the whole signal, in LUFS.
    #[must_use]
    pub const fn integrated(&self) -> f64 {
        self.integrated
    }

    /// The loudness range, in loudness units.
    ///
    /// A club master runs around 3 to 5; a well-recorded live album runs 10 or
    /// more. It is the number that says whether a track will survive being
    /// played next to another without the quiet parts disappearing.
    #[must_use]
    pub const fn range(&self) -> f64 {
        self.range
    }

    /// The largest absolute sample value, in decibels relative to full scale.
    #[must_use]
    pub const fn peak_dbfs(&self) -> f64 {
        self.peak
    }

    /// The true peak, in decibels relative to full scale.
    ///
    /// Measured after oversampling, so it accounts for what the waveform does
    /// *between* samples. A track can peak at exactly 0 dBFS by sample and
    /// still reconstruct above 0 in a converter — which is audible as
    /// distortion on some hardware and not on others, making it the kind of
    /// defect that is reported from the field and cannot be reproduced.
    #[must_use]
    pub const fn true_peak_dbfs(&self) -> f64 {
        self.true_peak
    }

    /// The short-term loudness curve, one value every [`Loudness::short_term_hop`]
    /// samples.
    ///
    /// This is what a loudness graph over the timeline draws, and what the
    /// energy curve of Master Prompt #20 is derived from.
    #[must_use]
    pub fn short_term(&self) -> &[f32] {
        &self.short_term
    }

    /// The spacing of the short-term curve, in samples.
    #[must_use]
    pub const fn short_term_hop(&self) -> usize {
        self.short_term_hop
    }

    /// The gain, in decibels, that would bring this signal to a target loudness.
    ///
    /// The one operation every consumer of a loudness measurement actually
    /// performs — matching two tracks, or matching a set to a platform's target
    /// — so it lives with the measurement rather than being re-derived by each
    /// caller with its own sign convention.
    #[must_use]
    pub fn gain_to_reach(&self, target_lufs: f64) -> f64 {
        if self.integrated <= SILENCE_LUFS {
            return 0.0;
        }
        target_lufs - self.integrated
    }
}

/// Measures the loudness of a mono signal.
///
/// # Errors
///
/// Returns [`AnalysisError::NotEnoughAudio`] for a signal shorter than one
/// measurement block, which cannot produce a gated measurement at all.
pub fn measure(samples: &[f32], sample_rate: SampleRate) -> Result<Loudness, AnalysisError> {
    let block_length = round_to_count(BLOCK_SECONDS * f64::from(sample_rate.hz()));
    if samples.len() < block_length {
        return Err(AnalysisError::NotEnoughAudio {
            frames: samples.len(),
            minimum: block_length,
        });
    }

    let weighted = k_weight(samples, sample_rate);

    let hop = round_to_count(count_to_f64(block_length) * (1.0 - BLOCK_OVERLAP)).max(1);
    let block_powers = mean_squares(&weighted, block_length, hop);
    let integrated = gated_loudness(&block_powers);

    let short_term_length = round_to_count(SHORT_TERM_SECONDS * f64::from(sample_rate.hz()));
    let short_term_hop = round_to_count(f64::from(sample_rate.hz()) * 0.1).max(1);
    let short_term_powers = mean_squares(&weighted, short_term_length.max(1), short_term_hop);
    let short_term: Vec<f32> = short_term_powers
        .iter()
        .map(|&power| crate::num::narrow(loudness_of(power)))
        .collect();

    Ok(Loudness {
        integrated,
        range: loudness_range(&short_term_powers),
        peak: peak_dbfs(samples),
        true_peak: true_peak_dbfs(samples),
        short_term,
        short_term_hop,
    })
}

/// Applies the two K-weighting filters.
///
/// The coefficients are those of the standard, specified at 48 kHz. At other
/// rates they are re-derived from the same filter design rather than reused,
/// because reusing coefficients across sample rates moves the corner
/// frequencies — at 44.1 kHz the shelf would sit 8 per cent low, which is a
/// small but systematic error in every measurement the product makes.
fn k_weight(samples: &[f32], sample_rate: SampleRate) -> Vec<f64> {
    let rate = f64::from(sample_rate.hz());

    // Stage one: a high shelf at about 1.7 kHz with 4 dB of lift, standing in
    // for the acoustic effect of a head in a diffuse field.
    let shelf = high_shelf(rate);
    // Stage two: a high-pass at about 38 Hz.
    let highpass = high_pass(rate);

    let mut output: Vec<f64> = samples.iter().map(|&s| f64::from(s)).collect();
    shelf.apply(&mut output);
    highpass.apply(&mut output);
    output
}

/// A direct-form biquad, used only here and only off the audio thread.
///
/// `prv-dsp` has a realtime biquad, and this is deliberately not it: depending
/// on the signal path from the analysis crate would invert the dependency
/// direction of ADR-0001, and the realtime version carries denormal flushing
/// and parameter smoothing that a batch measurement neither needs nor wants.
#[derive(Debug, Clone, Copy)]
struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
}

impl Biquad {
    fn apply(self, signal: &mut [f64]) {
        let mut x1 = 0.0_f64;
        let mut x2 = 0.0_f64;
        let mut y1 = 0.0_f64;
        let mut y2 = 0.0_f64;
        for sample in signal.iter_mut() {
            let x0 = *sample;
            let y0 = self.b0 * x0 + self.b1 * x1 + self.b2 * x2 - self.a1 * y1 - self.a2 * y2;
            x2 = x1;
            x1 = x0;
            y2 = y1;
            y1 = y0;
            *sample = y0;
        }
    }
}

/// The two K-weighting sections, taken from `prv-dsp`.
///
/// # Why the derivation is not here
///
/// The realtime meter measures the same quantity live, and a user watching a
/// meter and a user reading an export report must be looking at the same
/// number. They can only be the same number if the filter is the same filter,
/// so there is one derivation — in the crate that owns filters — rather than
/// two that agree until somebody improves one of them.
///
/// The parameters are the standard's own prototype, not a generic shelf with
/// the same corner frequency. A generic one comes within a quarter of a decibel
/// at 1 kHz, which sounds close and is not: it puts a full-scale 1 kHz sine at
/// −3.26 LUFS instead of the −3.01 the whole scale is anchored to, so every
/// loudness number the product reported would be wrong by the same amount,
/// consistently, and therefore invisibly.
fn weighting_sections(rate: f64) -> [Biquad; 2] {
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "the rate came from a SampleRate and round-trips exactly"
    )]
    let sample_rate = SampleRate::new(rate as u32).unwrap_or(SampleRate::HZ_48000);
    let passthrough = prv_dsp::BiquadCoefficients::PASSTHROUGH;
    let stages = prv_dsp::k_weighting(sample_rate);
    [
        from_coefficients(stages.first().copied().unwrap_or(passthrough)),
        from_coefficients(stages.get(1).copied().unwrap_or(passthrough)),
    ]
}

/// Adopts a coefficient set designed elsewhere.
fn from_coefficients(coefficients: prv_dsp::BiquadCoefficients) -> Biquad {
    let [b0, b1, b2, a1, a2] = coefficients.as_array();
    Biquad { b0, b1, b2, a1, a2 }
}

/// The K-weighting high shelf.
fn high_shelf(rate: f64) -> Biquad {
    weighting_sections(rate).first().copied().unwrap_or(Biquad {
        b0: 1.0,
        b1: 0.0,
        b2: 0.0,
        a1: 0.0,
        a2: 0.0,
    })
}

/// The K-weighting high-pass.
fn high_pass(rate: f64) -> Biquad {
    weighting_sections(rate).get(1).copied().unwrap_or(Biquad {
        b0: 1.0,
        b1: 0.0,
        b2: 0.0,
        a1: 0.0,
        a2: 0.0,
    })
}

/// The mean square of each overlapping window.
fn mean_squares(signal: &[f64], window: usize, hop: usize) -> Vec<f64> {
    if window == 0 || hop == 0 || signal.len() < window {
        return Vec::new();
    }
    // A running sum rather than a window recomputed per position: linear rather
    // than quadratic, which for a ten-minute track at 75 per cent overlap is
    // the difference between milliseconds and minutes.
    let mut result = Vec::new();
    let mut sum = 0.0_f64;
    for &value in signal.iter().take(window) {
        sum += value * value;
    }
    result.push(sum / count_to_f64(window));

    let mut start = 0_usize;
    while start + hop + window <= signal.len() {
        for offset in 0..hop {
            if let Some(&leaving) = signal.get(start + offset) {
                sum -= leaving * leaving;
            }
            if let Some(&entering) = signal.get(start + window + offset) {
                sum += entering * entering;
            }
        }
        start += hop;
        result.push((sum / count_to_f64(window)).max(0.0));
    }
    result
}

/// Converts a mean square to LUFS.
fn loudness_of(power: f64) -> f64 {
    if power <= 0.0 {
        return SILENCE_LUFS;
    }
    (LUFS_OFFSET + 10.0 * power.log10()).max(SILENCE_LUFS)
}

/// Converts LUFS back to a mean square.
fn power_of(loudness: f64) -> f64 {
    10.0_f64.powf((loudness - LUFS_OFFSET) / 10.0)
}

/// The two-stage gated mean of the block powers.
fn gated_loudness(powers: &[f64]) -> f64 {
    if powers.is_empty() {
        return SILENCE_LUFS;
    }

    let absolute = power_of(ABSOLUTE_GATE_LUFS);
    let above_absolute: Vec<f64> = powers
        .iter()
        .copied()
        .filter(|&power| power > absolute)
        .collect();
    if above_absolute.is_empty() {
        return SILENCE_LUFS;
    }

    let ungated_mean = above_absolute.iter().sum::<f64>() / count_to_f64(above_absolute.len());
    let relative = power_of(loudness_of(ungated_mean) + RELATIVE_GATE_LU);

    let retained: Vec<f64> = above_absolute
        .into_iter()
        .filter(|&power| power > relative)
        .collect();
    if retained.is_empty() {
        return SILENCE_LUFS;
    }

    loudness_of(retained.iter().sum::<f64>() / count_to_f64(retained.len()))
}

/// The spread between the tenth and ninety-fifth percentiles of short-term
/// loudness, above the relative gate.
fn loudness_range(powers: &[f64]) -> f64 {
    let absolute = power_of(ABSOLUTE_GATE_LUFS);
    let above: Vec<f64> = powers
        .iter()
        .copied()
        .filter(|&power| power > absolute)
        .collect();
    if above.len() < 2 {
        return 0.0;
    }

    let mean = above.iter().sum::<f64>() / count_to_f64(above.len());
    // The range uses a 20 LU relative gate rather than the 10 LU used for
    // integrated loudness, so that a genuine quiet passage counts toward the
    // spread instead of being excluded as though it were silence.
    let gate = power_of(loudness_of(mean) - 20.0);

    let mut retained: Vec<f64> = above.into_iter().filter(|&power| power > gate).collect();
    if retained.len() < 2 {
        return 0.0;
    }
    retained.sort_by(|a, b| a.partial_cmp(b).unwrap_or(core::cmp::Ordering::Equal));

    let low = percentile(&retained, 0.10);
    let high = percentile(&retained, 0.95);
    (loudness_of(high) - loudness_of(low)).max(0.0)
}

/// The value at a fraction through a sorted slice.
fn percentile(sorted: &[f64], fraction: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let position = fraction * count_to_f64(sorted.len().saturating_sub(1));
    let index = round_to_count(position).min(sorted.len().saturating_sub(1));
    sorted.get(index).copied().unwrap_or(0.0)
}

/// The largest absolute sample, in decibels relative to full scale.
fn peak_dbfs(samples: &[f32]) -> f64 {
    let peak = samples
        .iter()
        .map(|&sample| f64::from(sample).abs())
        .fold(0.0_f64, f64::max);
    to_dbfs(peak)
}

/// The largest reconstructed value, in decibels relative to full scale.
///
/// # Why interpolation has to be band-limited
///
/// The obvious implementation reads samples in pairs and interpolates linearly
/// between them. It cannot work, and the reason is worth stating because the
/// code looks correct: linear interpolation is monotonic between its endpoints,
/// so it never produces a value larger than a sample already present. Such an
/// implementation reports the sample peak under a different name, passes any
/// test that only checks it returns a plausible number, and tells the user
/// their master is safe when it is not.
///
/// The reconstruction a converter performs is band-limited, and a band-limited
/// curve through a set of samples *does* overshoot them. So the interpolation
/// here is a windowed-sinc polyphase filter — the same thing, four times over,
/// once per intermediate position.
///
/// The classic case is a sine near a quarter of the sample rate whose samples
/// straddle its crest: every sample reads well below full scale while the
/// waveform between them touches it.
fn true_peak_dbfs(samples: &[f32]) -> f64 {
    let filter = PolyphaseInterpolator::new();
    to_dbfs(filter.peak(samples))
}

/// The taps each phase of the interpolator uses.
///
/// Twelve. The stop-band rejection of a windowed sinc improves with length, and
/// twelve taps per phase puts it around 60 dB — far past the point where the
/// residual affects a peak reading, and short enough that measuring a track
/// costs one pass rather than being the slowest thing in the analysis.
const TAPS_PER_PHASE: usize = 12;

/// A four-times polyphase interpolator.
#[derive(Debug)]
struct PolyphaseInterpolator {
    /// One set of taps per intermediate phase. Phase zero is the original
    /// sample and is not filtered, so only three sets are held.
    phases: [[f64; TAPS_PER_PHASE]; TRUE_PEAK_OVERSAMPLING - 1],
}

impl PolyphaseInterpolator {
    fn new() -> Self {
        let length = TAPS_PER_PHASE * TRUE_PEAK_OVERSAMPLING;
        let centre = count_to_f64(length - 1) / 2.0;

        let mut prototype = vec![0.0_f64; length];
        for (index, tap) in prototype.iter_mut().enumerate() {
            let position = count_to_f64(index) - centre;
            let sinc = sinc(position / count_to_f64(TRUE_PEAK_OVERSAMPLING));
            // A Hann window on the sinc. Truncating it without a window would
            // leave the Gibbs ripple, which shows up as a peak reading that
            // wobbles with the signal's phase rather than following its
            // envelope.
            let window = 0.5
                * (1.0
                    - (core::f64::consts::TAU * count_to_f64(index) / count_to_f64(length - 1))
                        .cos());
            *tap = sinc * window;
        }

        let mut phases = [[0.0_f64; TAPS_PER_PHASE]; TRUE_PEAK_OVERSAMPLING - 1];
        for (offset, taps) in phases.iter_mut().enumerate() {
            let phase = offset + 1;
            let mut sum = 0.0_f64;
            for (index, tap) in taps.iter_mut().enumerate() {
                let source = index * TRUE_PEAK_OVERSAMPLING + phase;
                *tap = prototype.get(source).copied().unwrap_or(0.0);
                sum += *tap;
            }
            // Normalised so each phase passes a constant unchanged. Without
            // this the interpolated points sit slightly below the samples and
            // the measurement understates every peak by a fixed fraction.
            if sum.abs() > f64::EPSILON {
                for tap in taps.iter_mut() {
                    *tap /= sum;
                }
            }
        }

        Self { phases }
    }

    /// The largest absolute value of the reconstructed signal.
    fn peak(&self, samples: &[f32]) -> f64 {
        let mut peak = samples
            .iter()
            .map(|&sample| f64::from(sample).abs())
            .fold(0.0_f64, f64::max);

        // The filter is centred, so the output at position `n` is built from
        // samples around `n - TAPS_PER_PHASE/2`.
        //
        // Only positions whose whole support lies inside the signal are
        // interpolated. Reading zeros past the ends would fabricate a step
        // edge, and a band-limited reconstruction of a step overshoots — so a
        // file beginning at a non-zero sample would be reported as clipping
        // because of where the analysis started rather than because of what is
        // in it. The raw sample peak above already covers the edges, so nothing
        // real is missed.
        let lead = TAPS_PER_PHASE >> 1;
        let last = samples.len() + lead;
        if last < TAPS_PER_PHASE {
            return peak;
        }
        for position in lead..=(last - TAPS_PER_PHASE) {
            for taps in &self.phases {
                let mut accumulator = 0.0_f64;
                for (index, &tap) in taps.iter().enumerate() {
                    let source = (position + index).checked_sub(lead);
                    let value = source
                        .and_then(|source| samples.get(source))
                        .map_or(0.0, |&sample| f64::from(sample));
                    accumulator += tap * value;
                }
                peak = peak.max(accumulator.abs());
            }
        }

        peak
    }
}

/// The normalised sinc function.
fn sinc(x: f64) -> f64 {
    if x.abs() < f64::EPSILON {
        return 1.0;
    }
    let scaled = core::f64::consts::PI * x;
    scaled.sin() / scaled
}

/// Converts a linear amplitude to decibels relative to full scale.
fn to_dbfs(amplitude: f64) -> f64 {
    if amplitude <= 0.0 {
        return SILENCE_LUFS;
    }
    (20.0 * amplitude.log10()).max(SILENCE_LUFS)
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
    use crate::testing::{samples, tone};

    #[test]
    fn a_full_scale_kilohertz_tone_reads_the_standard_value() {
        // The anchor the whole scale is defined against: a 1 kHz sine at full
        // scale is −3.01 LUFS. If this is wrong every other number the product
        // reports about loudness is wrong by the same amount, and nothing else
        // would reveal it.
        for rate in [SampleRate::HZ_44100, SampleRate::HZ_48000] {
            let signal = tone(1_000.0, samples(5.0, rate), 1.0, rate);
            let loudness = measure(&signal, rate).expect("five seconds is enough");
            assert!(
                (loudness.integrated() - (-3.01)).abs() < 0.2,
                "at {} Hz a full-scale 1 kHz tone read {} LUFS",
                rate.hz(),
                loudness.integrated()
            );
        }
    }

    #[test]
    fn halving_the_amplitude_costs_six_decibels() {
        let rate = SampleRate::HZ_48000;
        let loud = measure(&tone(1_000.0, samples(5.0, rate), 1.0, rate), rate).expect("valid");
        let quiet = measure(&tone(1_000.0, samples(5.0, rate), 0.5, rate), rate).expect("valid");
        let difference = loud.integrated() - quiet.integrated();
        assert!(
            (difference - 6.02).abs() < 0.1,
            "halving the amplitude changed the reading by {difference} dB"
        );
    }

    #[test]
    fn the_weighting_prefers_the_midrange_to_the_bass() {
        // The whole reason for K-weighting, stated as a contrast with the
        // alternative rather than as a bare number. Two tones of identical
        // energy, at 40 Hz and 1 kHz: an unweighted meter reports them as
        // exactly equal, and a listener does not hear them that way.
        let rate = SampleRate::HZ_48000;
        let bass = tone(40.0, samples(5.0, rate), 0.5, rate);
        let mid = tone(1_000.0, samples(5.0, rate), 0.5, rate);

        let unweighted = |signal: &[f32]| -> f64 {
            let sum: f64 = signal.iter().map(|&s| f64::from(s) * f64::from(s)).sum();
            10.0 * (sum / count_to_f64(signal.len())).log10()
        };
        assert!(
            (unweighted(&bass) - unweighted(&mid)).abs() < 0.1,
            "the two tones do not carry the same energy, so the comparison means nothing"
        );

        let low = measure(&bass, rate).expect("valid");
        let high = measure(&mid, rate).expect("valid");
        let difference = high.integrated() - low.integrated();
        assert!(
            difference > 4.0,
            "40 Hz read {} LUFS and 1 kHz read {} LUFS, a difference of {difference}; the \
             weighting is not working",
            low.integrated(),
            high.integrated()
        );
    }

    #[test]
    fn the_derived_coefficients_reproduce_the_standard_table() {
        // The strongest check available without a reference implementation: the
        // standard publishes its coefficients at 48 kHz, and the derivation
        // used here must land on them. Anything close but not equal — a generic
        // shelf with a Butterworth Q, say — moves a full-scale 1 kHz tone by a
        // quarter of a decibel, which is invisible in every test except this
        // one and wrong in every number the product reports.
        let rate = 48_000.0_f64;
        let shelf = high_shelf(rate);
        let highpass = high_pass(rate);

        let expected_shelf = Biquad {
            b0: 1.535_124_859_586_97,
            b1: -2.691_696_189_406_38,
            b2: 1.198_392_810_852_85,
            a1: -1.690_659_293_182_41,
            a2: 0.732_480_774_215_85,
        };
        let expected_highpass = Biquad {
            b0: 1.0,
            b1: -2.0,
            b2: 1.0,
            a1: -1.990_047_454_833_98,
            a2: 0.990_072_250_366_21,
        };

        for (actual, expected, name) in [
            (shelf, expected_shelf, "shelf"),
            (highpass, expected_highpass, "high-pass"),
        ] {
            for (got, want, coefficient) in [
                (actual.b0, expected.b0, "b0"),
                (actual.b1, expected.b1, "b1"),
                (actual.b2, expected.b2, "b2"),
                (actual.a1, expected.a1, "a1"),
                (actual.a2, expected.a2, "a2"),
            ] {
                assert!(
                    (got - want).abs() < 1e-9,
                    "{name} {coefficient}: derived {got}, standard {want}"
                );
            }
        }
    }

    #[test]
    fn silence_before_a_track_does_not_make_it_quieter() {
        // What the gates are for, and the single most visible difference
        // between a gated measurement and an average. A track with a long quiet
        // intro must measure the same as the same track without it; otherwise
        // matching two tracks by loudness produces a jump at every transition
        // into a record with an intro.
        let rate = SampleRate::HZ_48000;
        let music = tone(1_000.0, samples(10.0, rate), 0.5, rate);
        let mut with_intro = vec![0.0_f32; samples(10.0, rate)];
        with_intro.extend(music.iter().copied());

        let plain = measure(&music, rate).expect("valid");
        let padded = measure(&with_intro, rate).expect("valid");
        assert!(
            (plain.integrated() - padded.integrated()).abs() < 0.3,
            "ten seconds of leading silence moved the reading from {} to {} LUFS",
            plain.integrated(),
            padded.integrated()
        );
    }

    #[test]
    fn a_dynamic_signal_has_a_wider_range_than_a_flat_one() {
        let rate = SampleRate::HZ_48000;
        let flat = tone(1_000.0, samples(20.0, rate), 0.5, rate);

        let mut dynamic = Vec::new();
        for step in 0..4 {
            let amplitude = if step % 2 == 0 { 0.5 } else { 0.05 };
            dynamic.extend(tone(1_000.0, samples(5.0, rate), amplitude, rate));
        }

        let flat_range = measure(&flat, rate).expect("valid").range();
        let dynamic_range = measure(&dynamic, rate).expect("valid").range();
        assert!(
            flat_range < 1.0,
            "a constant tone has a loudness range of {flat_range}"
        );
        assert!(
            dynamic_range > 10.0,
            "a signal alternating by 20 dB has a range of only {dynamic_range}"
        );
    }

    #[test]
    fn the_true_peak_catches_what_the_sample_peak_misses() {
        // The case this exists for: a signal whose samples never exceed a
        // level while the waveform between them does. At a quarter of Nyquist
        // and the right phase, a sine's samples straddle its crest.
        let rate = SampleRate::HZ_48000;
        let frequency = f64::from(rate.hz()) / 4.0;
        let step = core::f64::consts::TAU * frequency / f64::from(rate.hz());
        let signal: Vec<f32> = (0..samples(1.0, rate))
            .map(|n| {
                crate::num::narrow(
                    0.99 * (step * count_to_f64(n) + core::f64::consts::FRAC_PI_4).sin(),
                )
            })
            .collect();

        let loudness = measure(&signal, rate).expect("valid");
        // The samples straddle the crest at ±45 degrees, so every one of them
        // reads 3 dB below the actual amplitude. The true peak must recover it.
        assert!(
            loudness.peak_dbfs() < -2.8,
            "the sample peak is {}, so the signal is not the case this tests",
            loudness.peak_dbfs()
        );
        assert!(
            loudness.true_peak_dbfs() > loudness.peak_dbfs() + 2.0,
            "true peak {} is barely above sample peak {}; the interpolation is not \
             band-limited",
            loudness.true_peak_dbfs(),
            loudness.peak_dbfs()
        );
        assert!(
            (loudness.true_peak_dbfs() - to_dbfs(0.99)).abs() < 0.5,
            "true peak {} does not recover the signal's actual amplitude of {} dBFS",
            loudness.true_peak_dbfs(),
            to_dbfs(0.99)
        );
    }

    #[test]
    fn the_true_peak_of_a_constant_is_the_constant() {
        // The property that catches an unnormalised polyphase filter: a signal
        // that does not move must not gain or lose level when interpolated.
        let signal = vec![0.5_f32; 4_000];
        let measured = true_peak_dbfs(&signal);
        assert!(
            (measured - to_dbfs(0.5)).abs() < 0.01,
            "a constant 0.5 measured {measured} dBFS rather than {}",
            to_dbfs(0.5)
        );
    }

    #[test]
    fn silence_reports_a_floor_rather_than_an_infinity() {
        let rate = SampleRate::HZ_48000;
        let loudness = measure(&vec![0.0_f32; samples(2.0, rate)], rate).expect("valid");
        assert_eq!(loudness.integrated(), SILENCE_LUFS);
        assert_eq!(loudness.peak_dbfs(), SILENCE_LUFS);
        assert_eq!(loudness.true_peak_dbfs(), SILENCE_LUFS);
        assert!(loudness.integrated().is_finite());
        // Nothing can be matched to a target from silence, and returning a huge
        // gain would be worse than returning none.
        assert_eq!(loudness.gain_to_reach(-14.0), 0.0);
    }

    #[test]
    fn the_gain_to_a_target_is_the_difference() {
        let rate = SampleRate::HZ_48000;
        let loudness = measure(&tone(1_000.0, samples(5.0, rate), 0.5, rate), rate).expect("valid");
        let gain = loudness.gain_to_reach(-14.0);
        assert!(
            (loudness.integrated() + gain - (-14.0)).abs() < 1e-9,
            "applying {gain} dB does not reach the target"
        );
    }

    #[test]
    fn material_shorter_than_a_block_is_refused() {
        let rate = SampleRate::HZ_48000;
        assert!(matches!(
            measure(&vec![0.0_f32; 100], rate),
            Err(AnalysisError::NotEnoughAudio { .. })
        ));
    }

    #[test]
    fn the_short_term_curve_follows_the_signal() {
        let rate = SampleRate::HZ_48000;
        let mut signal = tone(1_000.0, samples(8.0, rate), 0.5, rate);
        signal.extend(tone(1_000.0, samples(8.0, rate), 0.05, rate));

        let loudness = measure(&signal, rate).expect("valid");
        let curve = loudness.short_term();
        assert!(!curve.is_empty());

        let first = curve.first().copied().unwrap_or(0.0);
        let last = curve.last().copied().unwrap_or(0.0);
        assert!(
            f64::from(first) - f64::from(last) > 15.0,
            "the curve went from {first} to {last}, missing a 20 dB drop"
        );
    }
}
