//! How loud it is, while it is playing.
//!
//! # One derivation of the weighting, two places that use it
//!
//! `prv-analysis` measures loudness offline, to the letter of the broadcast
//! standard, and `prv-export` reports what it found. This measures the same
//! quantity live, on the audio thread, so a user watching a meter and a user
//! reading an export report are looking at the same number.
//!
//! They can only be the same number if the filter is the same filter.
//! [`k_weighting`] is that filter, and it lives here — in the crate that owns
//! filters — precisely so there is one derivation rather than two that agree
//! until somebody improves one of them. An earlier version of this project
//! derived the shelf generically and was 0.26 dB out at 1 kHz: consistently
//! wrong, and therefore invisible. That is the failure this arrangement exists
//! to prevent.
//!
//! # Why the numbers are not the same as the offline ones, and that is correct
//!
//! The offline measurement is *gated*: quiet passages below a relative
//! threshold are excluded, so a track's integrated loudness describes the parts
//! a listener would call the music. A live meter cannot gate, because gating is
//! a statement about a whole recording and there is no whole recording yet.
//!
//! So this reports momentary and short-term windows — the two the standard
//! defines for exactly this purpose — and does not pretend to report an
//! integrated figure. A meter that showed a running "integrated" value would be
//! showing a number whose definition it was not honouring.

use prv_rt::AudioBuffer;
use prv_time::SampleRate;

use crate::biquad::{Biquad, BiquadCoefficients};
use crate::processor::{PrepareConfig, ProcessContext, Processor};

/// The offset that anchors the scale.
///
/// From the standard. A full-scale sine at 1 kHz reads −3.01 LUFS with this in
/// place, which is the anchor every other number is relative to.
const SCALE_OFFSET_DB: f64 = -0.691;

/// How long one measurement block covers, in milliseconds.
const BLOCK_MS: f64 = 100.0;

/// Blocks in the momentary window — 400 ms.
const MOMENTARY_BLOCKS: usize = 4;

/// Blocks in the short-term window — three seconds.
const SHORT_TERM_BLOCKS: usize = 30;

/// The two filter sections of the K-weighting, derived for a sample rate.
///
/// The first is the high shelf that models the acoustic effect of a head in a
/// sound field; the second is the high-pass that removes content which moves air
/// without contributing to how loud something sounds.
///
/// The prototype's parameters are the standard's own, not a generic shelf with
/// the same corner frequency. A generic one comes within a quarter of a decibel
/// at 1 kHz, which sounds close and is not: it puts a full-scale 1 kHz sine at
/// −3.26 LUFS instead of −3.01, so every loudness number the product reported
/// would be wrong by the same amount, consistently, and therefore invisibly.
#[must_use]
pub fn k_weighting(sample_rate: SampleRate) -> [BiquadCoefficients; 2] {
    let rate = f64::from(sample_rate.hz());
    [high_shelf(rate), high_pass(rate)]
}

/// The shelf, from the standard's derivation.
fn high_shelf(rate: f64) -> BiquadCoefficients {
    const FREQUENCY: f64 = 1_681.974_450_955_533;
    const GAIN_DB: f64 = 3.999_843_853_973_347;
    const Q: f64 = 0.707_175_236_955_419_6;

    let k = (core::f64::consts::PI * FREQUENCY / rate).tan();
    let high_gain = 10.0_f64.powf(GAIN_DB / 20.0);
    // The band gain is the high gain raised to a power fixed by the prototype;
    // it is what makes the transition follow the analogue response rather than a
    // digital approximation of it.
    let band_gain = high_gain.powf(0.499_666_774_154_541_6);

    let denominator = k.mul_add(k, 1.0 + k / Q);
    BiquadCoefficients::from_normalised(
        (k.mul_add(k, band_gain * k / Q) + high_gain) / denominator,
        2.0 * k.mul_add(k, -high_gain) / denominator,
        (k.mul_add(k, -(band_gain * k / Q)) + high_gain) / denominator,
        2.0 * k.mul_add(k, -1.0) / denominator,
        k.mul_add(k, 1.0 - k / Q) / denominator,
    )
}

/// The high-pass, from the standard's derivation.
fn high_pass(rate: f64) -> BiquadCoefficients {
    const FREQUENCY: f64 = 38.135_470_876_024_44;
    const Q: f64 = 0.500_327_037_323_877_3;

    let k = (core::f64::consts::PI * FREQUENCY / rate).tan();
    let denominator = k.mul_add(k, 1.0 + k / Q);
    BiquadCoefficients::from_normalised(
        1.0,
        -2.0,
        1.0,
        2.0 * k.mul_add(k, -1.0) / denominator,
        k.mul_add(k, 1.0 - k / Q) / denominator,
    )
}

/// Widens a count to a real number.
///
/// Every count converted here is a block index or a frame count within a
/// hundred-millisecond block — exact in `f64` by many orders of magnitude.
#[allow(
    clippy::cast_precision_loss,
    reason = "counts here are bounded far below 2^53, where f64 is exact"
)]
#[inline]
fn count_to_f64(count: usize) -> f64 {
    count as f64
}

/// What one channel of the meter needs.
#[derive(Debug)]
struct ChannelState {
    shelf: Biquad,
    high_pass: Biquad,
}

/// Measures loudness while audio is playing.
///
/// Sits in the chain and changes nothing. Implementing
/// [`Processor`](crate::Processor) rather than inventing a second shape means it
/// is prepared, reset and latency-accounted like everything else, and that the
/// allocation gate already covers it.
#[derive(Debug)]
pub struct LoudnessMeter {
    channels: Vec<ChannelState>,
    block_frames: usize,
    frames_into_block: usize,
    sum_of_squares: f64,

    powers: Vec<f64>,
    written: usize,
    blocks_seen: u64,

    peak: f32,
}

impl Default for LoudnessMeter {
    fn default() -> Self {
        Self::new()
    }
}

impl LoudnessMeter {
    /// A meter that measures nothing until it is prepared.
    #[must_use]
    pub fn new() -> Self {
        Self {
            channels: Vec::new(),
            block_frames: 1,
            frames_into_block: 0,
            sum_of_squares: 0.0,
            powers: Vec::new(),
            written: 0,
            blocks_seen: 0,
            peak: 0.0,
        }
    }

    /// The loudness of the last 400 milliseconds, in LUFS.
    ///
    /// `None` until that much audio has been seen. An estimate from a fifth of a
    /// window is not a momentary loudness, and reporting one would put a number
    /// on a meter that means something else.
    #[must_use]
    pub fn momentary_lufs(&self) -> Option<f64> {
        self.window_lufs(MOMENTARY_BLOCKS)
    }

    /// The loudness of the last three seconds, in LUFS.
    #[must_use]
    pub fn short_term_lufs(&self) -> Option<f64> {
        self.window_lufs(SHORT_TERM_BLOCKS)
    }

    /// The largest sample magnitude seen since the last reset.
    ///
    /// The *sample* peak, not the true peak: reconstructing between samples
    /// costs a polyphase filter per channel per sample, and a live meter that
    /// spent that would be taking it from the audio. The true peak is what
    /// `prv-export` reports before delivery, where the cost is affordable and
    /// the answer matters.
    #[must_use]
    pub const fn sample_peak(&self) -> f32 {
        self.peak
    }

    /// The loudness over the most recent `blocks` blocks.
    fn window_lufs(&self, blocks: usize) -> Option<f64> {
        if self.blocks_seen < blocks as u64 || self.powers.is_empty() {
            return None;
        }
        let capacity = self.powers.len();
        let mut sum = 0.0;
        for step in 0..blocks {
            let index = (self.written + capacity - 1 - step) % capacity;
            sum += self.powers.get(index).copied().unwrap_or(0.0);
        }
        let mean = sum / count_to_f64(blocks);
        if mean <= 0.0 {
            return None;
        }
        Some(SCALE_OFFSET_DB + 10.0 * mean.log10())
    }

    /// Closes the current block and starts a new one.
    fn close_block(&mut self) {
        let frames = count_to_f64(self.block_frames.max(1));
        let power = self.sum_of_squares / frames;
        let capacity = self.powers.len().max(1);
        if let Some(slot) = self.powers.get_mut(self.written % capacity) {
            *slot = power;
        }
        self.written = (self.written + 1) % capacity;
        self.blocks_seen = self.blocks_seen.saturating_add(1);
        self.sum_of_squares = 0.0;
        self.frames_into_block = 0;
    }
}

impl Processor for LoudnessMeter {
    fn name(&self) -> &'static str {
        "loudness-meter"
    }

    fn prepare(&mut self, config: &PrepareConfig) {
        let stages = k_weighting(config.sample_rate);
        let channels = config.channels.max(1);
        self.channels = (0..channels)
            .map(|_| {
                let mut first = Biquad::new();
                let mut second = Biquad::new();
                if let Some(coefficients) = stages.first() {
                    first.set_coefficients(*coefficients);
                }
                if let Some(coefficients) = stages.get(1) {
                    second.set_coefficients(*coefficients);
                }
                ChannelState {
                    shelf: first,
                    high_pass: second,
                }
            })
            .collect();

        let rate = f64::from(config.sample_rate.hz());
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a hundred milliseconds at any accepted rate is a small count"
        )]
        let block = (rate * BLOCK_MS / 1000.0) as usize;
        self.block_frames = block.max(1);
        self.powers = vec![0.0; SHORT_TERM_BLOCKS];
        self.reset();
    }

    fn process(&mut self, ctx: &ProcessContext, buffer: &mut AudioBuffer) {
        let frames = ctx.frames.min(buffer.frames());
        let channels = buffer.channels().min(self.channels.len());
        if channels == 0 {
            return;
        }

        for frame in 0..frames {
            let mut weighted = 0.0_f64;
            for channel in 0..channels {
                let sample = buffer
                    .channel(channel)
                    .and_then(|data| data.get(frame))
                    .copied()
                    .unwrap_or(0.0);
                self.peak = self.peak.max(sample.abs());

                let Some(state) = self.channels.get_mut(channel) else {
                    continue;
                };
                // Both stages, in the standard's order. The meter reads the
                // audio and leaves it alone: the filtering happens on a copy of
                // the value, never in the buffer.
                let shelved = state.shelf.process(f64::from(sample));
                let filtered = state.high_pass.process(shelved);
                weighted += filtered * filtered;
            }

            self.sum_of_squares += weighted;
            self.frames_into_block += 1;
            if self.frames_into_block >= self.block_frames {
                self.close_block();
            }
        }
    }

    fn reset(&mut self) {
        for state in &mut self.channels {
            state.shelf.reset();
            state.high_pass.reset();
        }
        self.powers.fill(0.0);
        self.written = 0;
        self.blocks_seen = 0;
        self.frames_into_block = 0;
        self.sum_of_squares = 0.0;
        self.peak = 0.0;
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::integer_division,
        clippy::cast_sign_loss,
        clippy::float_cmp,
        reason = "test fixtures index their own buffers and compare exact constants"
    )]

    use super::*;

    const RATE: SampleRate = SampleRate::HZ_48000;
    const BLOCK: usize = 480;

    fn prepared(channels: usize) -> LoudnessMeter {
        let mut meter = LoudnessMeter::new();
        meter.prepare(&PrepareConfig::new(RATE, BLOCK as u32, channels));
        meter
    }

    /// Feeds a sine at a given amplitude for a number of seconds.
    fn feed(
        meter: &mut LoudnessMeter,
        channels: usize,
        frequency: f32,
        amplitude: f32,
        seconds: f32,
    ) {
        let mut buffer = AudioBuffer::new(channels, BLOCK).expect("a buffer");
        let context = ProcessContext::new(BLOCK, RATE);
        let blocks = (seconds * RATE.hz() as f32 / BLOCK as f32) as usize;
        let mut frame_index = 0_usize;

        for _ in 0..blocks {
            for channel in 0..channels {
                let data = buffer.channel_mut(channel).expect("a channel");
                for (offset, slot) in data.iter_mut().enumerate() {
                    let index = frame_index + offset;
                    let phase =
                        index as f32 / RATE.hz() as f32 * frequency * core::f32::consts::TAU;
                    *slot = phase.sin() * amplitude;
                }
            }
            meter.process(&context, &mut buffer);
            frame_index += BLOCK;
        }
    }

    #[test]
    fn a_full_scale_sine_reads_the_figure_the_scale_is_anchored_to() {
        // −3.01 LUFS. Every other number the product reports is relative to
        // this one, so if it is wrong they all are — consistently, and
        // therefore invisibly.
        let mut meter = prepared(1);
        feed(&mut meter, 1, 1000.0, 1.0, 2.0);

        let momentary = meter.momentary_lufs().expect("two seconds of audio");
        assert!(
            (momentary - -3.01).abs() < 0.1,
            "a full-scale 1 kHz sine read {momentary} LUFS, expected −3.01"
        );
    }

    #[test]
    fn halving_the_amplitude_costs_six_decibels() {
        let mut loud = prepared(1);
        feed(&mut loud, 1, 1000.0, 0.5, 2.0);
        let at_half = loud.momentary_lufs().expect("audio");

        let mut quiet = prepared(1);
        feed(&mut quiet, 1, 1000.0, 0.25, 2.0);
        let at_quarter = quiet.momentary_lufs().expect("audio");

        assert!(
            ((at_half - at_quarter) - 6.02).abs() < 0.1,
            "halving cost {} dB, expected 6.02",
            at_half - at_quarter
        );
    }

    #[test]
    fn the_weighting_makes_low_frequencies_count_for_less() {
        // The whole point of K-weighting: a 40 Hz tone at the same amplitude as
        // a 1 kHz one does not sound as loud, and must not measure as loud.
        let mut low = prepared(1);
        feed(&mut low, 1, 40.0, 0.5, 2.0);
        let at_low = low.momentary_lufs().expect("audio");

        let mut reference = prepared(1);
        feed(&mut reference, 1, 1000.0, 0.5, 2.0);
        let at_reference = reference.momentary_lufs().expect("audio");

        assert!(
            at_reference - at_low > 4.0,
            "40 Hz measured only {} dB below 1 kHz",
            at_reference - at_low
        );
    }

    #[test]
    fn nothing_is_reported_before_there_is_a_window_to_report() {
        // An estimate from a fifth of a window is not a momentary loudness, and
        // reporting one would put a number on a meter that means something else.
        let mut meter = prepared(2);
        assert_eq!(meter.momentary_lufs(), None);
        assert_eq!(meter.short_term_lufs(), None);

        feed(&mut meter, 2, 1000.0, 0.5, 0.5);
        assert!(meter.momentary_lufs().is_some(), "400 ms should be enough");
        assert_eq!(
            meter.short_term_lufs(),
            None,
            "half a second is not three seconds"
        );

        feed(&mut meter, 2, 1000.0, 0.5, 3.0);
        assert!(meter.short_term_lufs().is_some());
    }

    #[test]
    fn the_meter_changes_nothing() {
        // It sits in the chain. A meter that altered the audio would be the
        // worst kind of defect: one that only shows up when somebody switches
        // the display on.
        let mut meter = prepared(2);
        let mut buffer = AudioBuffer::new(2, BLOCK).expect("a buffer");
        for channel in 0..2 {
            let data = buffer.channel_mut(channel).expect("a channel");
            for (index, slot) in data.iter_mut().enumerate() {
                *slot = (index as f32 * 0.01).sin() * 0.7;
            }
        }
        let before: Vec<f32> = buffer.channel(0).expect("a channel").to_vec();

        meter.process(&ProcessContext::new(BLOCK, RATE), &mut buffer);

        let after = buffer.channel(0).expect("a channel");
        for (index, (was, is)) in before.iter().zip(after.iter()).enumerate() {
            assert!(
                (was - is).abs() < f32::EPSILON,
                "the meter changed sample {index} from {was} to {is}"
            );
        }
    }

    #[test]
    fn silence_reports_nothing_rather_than_minus_infinity() {
        // A meter showing an enormous negative number during a gap between
        // tracks is a meter the user learns to ignore.
        let mut meter = prepared(2);
        feed(&mut meter, 2, 1000.0, 0.0, 2.0);
        assert_eq!(meter.momentary_lufs(), None);
        assert_eq!(meter.sample_peak(), 0.0);
    }

    #[test]
    fn resetting_forgets_the_previous_track() {
        let mut meter = prepared(1);
        feed(&mut meter, 1, 1000.0, 1.0, 2.0);
        assert!(meter.momentary_lufs().is_some());
        assert!(meter.sample_peak() > 0.9);

        meter.reset();
        assert_eq!(meter.momentary_lufs(), None);
        assert_eq!(meter.sample_peak(), 0.0);
    }

    #[test]
    fn the_peak_is_the_largest_magnitude_since_the_reset() {
        let mut meter = prepared(1);
        feed(&mut meter, 1, 1000.0, 0.8, 0.5);
        let after_quiet = meter.sample_peak();
        assert!((after_quiet - 0.8).abs() < 0.01);

        feed(&mut meter, 1, 1000.0, 0.3, 0.5);
        assert!(
            (meter.sample_peak() - after_quiet).abs() < 1e-6,
            "the peak fell when the signal did"
        );
    }

    #[test]
    fn the_weighting_is_the_same_at_every_accepted_rate() {
        // Derived per rate rather than tabulated for one. A meter that was only
        // right at 48 kHz would be wrong on every interface that runs at 44.1.
        for rate in [
            SampleRate::HZ_44100,
            SampleRate::HZ_48000,
            SampleRate::HZ_96000,
        ] {
            let mut meter = LoudnessMeter::new();
            meter.prepare(&PrepareConfig::new(rate, BLOCK as u32, 1));

            let mut buffer = AudioBuffer::new(1, BLOCK).expect("a buffer");
            let context = ProcessContext::new(BLOCK, rate);
            let blocks = 2 * rate.hz() as usize / BLOCK;
            let mut frame_index = 0_usize;
            for _ in 0..blocks {
                let data = buffer.channel_mut(0).expect("a channel");
                for (offset, slot) in data.iter_mut().enumerate() {
                    let index = frame_index + offset;
                    let phase = index as f32 / rate.hz() as f32 * 1000.0 * core::f32::consts::TAU;
                    *slot = phase.sin();
                }
                meter.process(&context, &mut buffer);
                frame_index += BLOCK;
            }

            let momentary = meter.momentary_lufs().expect("two seconds of audio");
            assert!(
                (momentary - -3.01).abs() < 0.15,
                "at {} Hz a full-scale sine read {momentary} LUFS",
                rate.hz()
            );
        }
    }
}
