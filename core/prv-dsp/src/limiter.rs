//! The last thing in the signal path.
//!
//! # Why a limiter is the one processor that may not be approximately right
//!
//! Everything else in the chain shapes the sound. This one makes a promise: that
//! nothing leaves above the ceiling. A limiter that is *usually* under is not a
//! limiter — it is a compressor with a marketing name, and the file it produces
//! clips on some listeners' players and not others, which is the defect that
//! gets reported from the field and cannot be reproduced at the desk.
//!
//! So the guarantee here is structural rather than statistical, and there is a
//! test that hunts for a violation over material designed to produce one.
//!
//! # It limits the peak a converter will actually produce
//!
//! Sample values are not what a listener hears. Between two samples the
//! reconstructed waveform can rise above both of them, and a file whose samples
//! all sit at −0.1 dBFS can leave a converter at +0.7 dBTP. Master Prompt #3C
//! requires a true-peak figure in the export report, and it would be strange to
//! report a number the master chain had not been limiting against — so the
//! detector runs on a four-times oversampled estimate, the same reconstruction
//! `prv-analysis` uses to measure it.
//!
//! # How the gain gets down in time without a step
//!
//! A limiter that drops its gain the instant it sees a peak puts a
//! discontinuity into the *gain*, which is audible as a click even though the
//! sample it protects is now under the ceiling. The usual fixes trade one defect
//! for another: smoothing the gain lets peaks through, and hard clipping the
//! remainder is the thing being avoided.
//!
//! What this does instead is look ahead by [`Limiter::LOOK_AHEAD_FRAMES`] and,
//! at every sample, choose the shallowest straight line that reaches every
//! required gain in the window on time. The result is guaranteed to be at or
//! below what each sample needs when that sample arrives, and it moves in
//! straight lines rather than steps. The cost is a fixed delay, reported
//! through [`Processor::latency_frames`] so the graph compensates it.

use prv_rt::AudioBuffer;

use crate::processor::{PrepareConfig, ProcessContext, Processor};

/// Widens a count to a real number.
///
/// Every count converted here is a tap index, a distance within the look-ahead
/// window, or a sample rate. The largest is the rate, bounded by the engine at
/// 768 000 — well inside the range where `f32` represents every integer exactly
/// — so the precision-loss warning describes a case that cannot arise.
#[allow(
    clippy::cast_precision_loss,
    reason = "counts here are bounded far below 2^24, where f32 is exact"
)]
#[inline]
fn count_to_f32(count: usize) -> f32 {
    count as f32
}

/// Widens a count for the filter design, which is done in double precision.
#[allow(
    clippy::cast_precision_loss,
    reason = "filter lengths are single digits; f64 is exact far beyond them"
)]
#[inline]
fn count_to_f64(count: usize) -> f64 {
    count as f64
}

/// Narrows a designed coefficient to the precision the signal path works in.
///
/// The filter is designed in double precision because transcendental functions
/// are, and stored in single because the audio is. Every coefficient is well
/// inside one in magnitude, so this narrows precision and cannot overflow.
#[allow(
    clippy::cast_possible_truncation,
    reason = "coefficients are bounded by one; this narrows precision, not range"
)]
#[inline]
fn coefficient_to_f32(value: f64) -> f32 {
    value as f32
}

/// Taps per phase in the oversampling detector.
///
/// Seventeen, which is eight samples of support either side of the point being
/// reconstructed. Enough for the band-limited estimate to be worth trusting near
/// a transient, short enough that the detector is a few dozen operations per
/// sample rather than a few hundred.
const DETECTOR_TAPS: usize = 17;

/// How many points are reconstructed between one sample and the next.
///
/// Four. The published guidance for measuring true peak at 48 kHz, and the
/// factor `prv-analysis` uses, so the number the master chain limits against and
/// the number the export report shows come from the same reconstruction.
const OVERSAMPLING: usize = 4;

/// Half the detector's support, in samples.
///
/// Also the detector's own delay: a symmetric filter reconstructs the point at
/// the centre of its window, so the estimate that arrives when sample *n* is
/// read describes the sample `DETECTOR_HALF` earlier. The audio path is delayed
/// by this
/// much on top of the look-ahead to line the two up. Getting that wrong leaves
/// a hole of exactly this many samples in the protection, which is the kind of
/// defect that shows up as "it clips sometimes".
#[allow(
    clippy::integer_division,
    reason = "the filter length is odd by construction, so this is exact"
)]
const DETECTOR_HALF: usize = DETECTOR_TAPS / 2;

/// A look-ahead peak limiter.
///
/// Deliberately not configurable beyond its ceiling and release. A limiter with
/// an attack control is a limiter that can be set to fail, and the one setting
/// that matters — how loud is too loud — is the one the delivery target already
/// states.
#[derive(Debug)]
pub struct Limiter {
    ceiling: f32,
    release_frames: f32,

    /// Delayed audio, one ring per channel, each `LOOK_AHEAD_FRAMES` long.
    delay: Vec<Vec<f32>>,
    /// Required gain for each of the next `LOOK_AHEAD_FRAMES` samples.
    required: Vec<f32>,
    /// Detector history, one ring per channel.
    history: Vec<Vec<f32>>,
    /// Polyphase reconstruction coefficients, `OVERSAMPLING - 1` phases.
    phases: Vec<Vec<f32>>,

    write: usize,
    gain: f32,
    reduction: f32,
    prepared_channels: usize,
}

impl Default for Limiter {
    fn default() -> Self {
        Self::new()
    }
}

impl Limiter {
    /// How far ahead the detector looks, in frames.
    ///
    /// Sixty-four — about 1.3 milliseconds at 48 kHz. Long enough that the gain
    /// reaches a deep reduction in a straight line rather than a corner, short
    /// enough that the delay it costs the graph is inaudible and cheap to
    /// compensate.
    pub const LOOK_AHEAD_FRAMES: usize = 64;

    /// The delay the audio path takes, in frames.
    ///
    /// The look-ahead plus the detector's own delay. Reported through
    /// [`Processor::latency_frames`] so the graph compensates it.
    pub const DELAY_FRAMES: usize = Self::LOOK_AHEAD_FRAMES + DETECTOR_HALF;

    /// The default ceiling, in decibels relative to full scale.
    ///
    /// −1 dBTP, which is what every normalising delivery target in `prv-export`
    /// asks for, and for the reason recorded there: a lossy encoder's output
    /// overshoots its input.
    pub const DEFAULT_CEILING_DB: f32 = -1.0;

    /// The default release, in milliseconds.
    ///
    /// Fifty. Short enough that the level recovers between kicks rather than
    /// ducking the whole bar behind one transient, long enough not to modulate
    /// the bass — the audible cost of a fast release on dance material, where
    /// the loudest thing in the mix is also the lowest.
    pub const DEFAULT_RELEASE_MS: f32 = 50.0;

    /// The lowest ceiling accepted, in decibels.
    pub const MIN_CEILING_DB: f32 = -24.0;

    /// A limiter at the default ceiling and release.
    #[must_use]
    pub fn new() -> Self {
        Self {
            ceiling: db_to_linear(Self::DEFAULT_CEILING_DB),
            release_frames: 1.0,
            delay: Vec::new(),
            required: Vec::new(),
            history: Vec::new(),
            phases: Vec::new(),
            write: 0,
            gain: 1.0,
            reduction: 0.0,
            prepared_channels: 0,
        }
    }

    /// Sets the ceiling, in decibels relative to full scale.
    ///
    /// Clamped to at most 0 dB and at least [`Self::MIN_CEILING_DB`]. A ceiling
    /// above full scale would be a limiter that does not limit, and one far
    /// below is a fader with extra steps.
    pub fn set_ceiling_db(&mut self, decibels: f32) {
        let bounded = if decibels.is_nan() {
            Self::DEFAULT_CEILING_DB
        } else {
            decibels.clamp(Self::MIN_CEILING_DB, 0.0)
        };
        self.ceiling = db_to_linear(bounded);
    }

    /// The ceiling in force, linear.
    #[must_use]
    pub const fn ceiling(&self) -> f32 {
        self.ceiling
    }

    /// How much the limiter is holding back right now, in decibels.
    ///
    /// What a meter shows. Reported rather than inferred, because a user
    /// watching six decibels of reduction on a master is being told something
    /// about their mix that no other number says.
    #[must_use]
    pub fn reduction_db(&self) -> f32 {
        self.reduction
    }

    /// The largest reconstructed magnitude around one sample.
    ///
    /// Reads the detector history, which holds the most recent
    /// [`DETECTOR_TAPS`] samples, and reconstructs the points between the two in
    /// the middle. Allocation-free and branch-bounded.
    fn true_peak_at(&self, channel: usize, centre: usize) -> f32 {
        let Some(history) = self.history.get(channel) else {
            return 0.0;
        };
        let taps = history.len();
        if taps < DETECTOR_TAPS {
            return 0.0;
        }

        // The sample itself is one of the reconstructed points, and needs no
        // filter: phase zero of a windowed sinc is an impulse.
        let mut peak = history
            .get((centre + DETECTOR_HALF) % taps)
            .map_or(0.0, |sample| sample.abs());

        for phase in &self.phases {
            let mut sum = 0.0;
            for (tap, coefficient) in phase.iter().enumerate() {
                let index = (centre + tap) % taps;
                sum += history.get(index).copied().unwrap_or(0.0) * coefficient;
            }
            peak = peak.max(sum.abs());
        }
        peak
    }
}

impl Processor for Limiter {
    fn name(&self) -> &'static str {
        "limiter"
    }

    fn prepare(&mut self, config: &PrepareConfig) {
        let channels = config.channels.max(1);
        self.prepared_channels = channels;

        self.delay = vec![vec![0.0; Self::DELAY_FRAMES]; channels];
        self.history = vec![vec![0.0; DETECTOR_TAPS]; channels];
        self.required = vec![1.0; Self::DELAY_FRAMES];
        self.phases = build_phases();
        self.write = 0;
        self.gain = 1.0;
        self.reduction = 0.0;

        let rate = count_to_f32(config.sample_rate.hz() as usize);
        self.release_frames = (Self::DEFAULT_RELEASE_MS / 1000.0 * rate).max(1.0);
    }

    fn process(&mut self, ctx: &ProcessContext, buffer: &mut AudioBuffer) {
        let frames = ctx.frames.min(buffer.frames());
        let channels = buffer.channels().min(self.prepared_channels);
        if channels == 0 || self.required.is_empty() {
            return;
        }

        // One ring length for the audio and the requirements alike. Two lengths
        // was the first version, and the eight-sample difference between them
        // filed every requirement in the wrong slot — a hole in the protection
        // that reads as "it clips sometimes". One length removes the class.
        let window = Self::DELAY_FRAMES;

        // How far the gain may rise in one sample while releasing. Linear in
        // decibels would be more conventional; linear in amplitude is what keeps
        // the recovery from being audible as a swell on sustained material.
        let release_step = 1.0 / self.release_frames;

        for frame in 0..frames {
            let slot = self.write % window;

            // 1. Take the incoming sample into the detector.
            let mut described_peak = 0.0_f32;
            for channel in 0..channels {
                let sample = buffer
                    .channel(channel)
                    .and_then(|data| data.get(frame))
                    .copied()
                    .unwrap_or(0.0);

                if let Some(history) = self.history.get_mut(channel) {
                    let taps = history.len();
                    if let Some(entry) = history.get_mut(self.write % taps) {
                        *entry = sample;
                    }
                }
                described_peak = described_peak
                    .max(self.true_peak_at(channel, (self.write + 1) % DETECTOR_TAPS));
            }

            // 2. File the estimate against the sample it describes, which is
            //    the one read `DETECTOR_HALF` iterations ago — a symmetric
            //    filter reconstructs the centre of its window, not its edge.
            let needed = if described_peak > self.ceiling {
                self.ceiling / described_peak
            } else {
                1.0
            };
            let described = (self.write + window - DETECTOR_HALF) % window;
            if let Some(entry) = self.required.get_mut(described) {
                *entry = needed;
            }

            // 3. The shallowest straight line that reaches every requirement in
            //    the window on time. `slot` is at distance one: the gain is
            //    updated and applied in this same iteration, so the sample
            //    leaving the delay right now is the first thing the line has to
            //    satisfy.
            let mut target_slope = release_step;
            for step in 0..window {
                let index = (slot + step) % window;
                let required = self.required.get(index).copied().unwrap_or(1.0);
                if required < self.gain {
                    let distance = count_to_f32(step + 1);
                    let slope = (required - self.gain) / distance;
                    if slope < target_slope {
                        target_slope = slope;
                    }
                }
            }
            self.gain = (self.gain + target_slope).clamp(0.0, 1.0);

            // 4. Apply to the sample leaving the delay, and put the new one in.
            //    The audio and the requirements share a slot, so this must read
            //    before step 5 overwrites it.
            for channel in 0..channels {
                let sample = buffer
                    .channel(channel)
                    .and_then(|data| data.get(frame))
                    .copied()
                    .unwrap_or(0.0);

                let delayed = self
                    .delay
                    .get(channel)
                    .and_then(|ring| ring.get(slot))
                    .copied()
                    .unwrap_or(0.0);

                if let Some(entry) = self
                    .delay
                    .get_mut(channel)
                    .and_then(|ring| ring.get_mut(slot))
                {
                    *entry = sample;
                }
                if let Some(entry) = buffer
                    .channel_mut(channel)
                    .and_then(|data| data.get_mut(frame))
                {
                    *entry = delayed * self.gain;
                }
            }

            // 5. The sample that just arrived has no estimate yet — it gets one
            //    in `DETECTOR_HALF` iterations, still a full look-ahead before
            //    it is due. Until then it constrains nothing, rather than
            //    carrying whatever occupied the slot a window ago.
            if let Some(entry) = self.required.get_mut(slot) {
                *entry = 1.0;
            }

            self.write = self.write.wrapping_add(1);
        }

        self.reduction = if self.gain > 0.0 {
            -linear_to_db(self.gain)
        } else {
            -linear_to_db(f32::EPSILON)
        };
    }

    fn reset(&mut self) {
        for ring in &mut self.delay {
            ring.fill(0.0);
        }
        for ring in &mut self.history {
            ring.fill(0.0);
        }
        self.required.fill(1.0);
        self.write = 0;
        self.gain = 1.0;
        self.reduction = 0.0;
    }

    fn latency_frames(&self) -> u32 {
        u32::try_from(Self::DELAY_FRAMES).unwrap_or(u32::MAX)
    }
}

/// The reconstruction coefficients, one set per intermediate point.
///
/// A windowed sinc, sampled at each fractional offset. Built once in `prepare`
/// because building it is arithmetic on transcendental functions and the audio
/// thread does not do that.
fn build_phases() -> Vec<Vec<f32>> {
    let mut phases = Vec::with_capacity(OVERSAMPLING - 1);
    for point in 1..OVERSAMPLING {
        let offset = count_to_f64(point) / count_to_f64(OVERSAMPLING);
        let mut taps = Vec::with_capacity(DETECTOR_TAPS);
        for tap in 0..DETECTOR_TAPS {
            let distance = count_to_f64(tap) - count_to_f64(DETECTOR_HALF) - offset;
            taps.push(coefficient_to_f32(windowed_sinc(distance)));
        }
        phases.push(taps);
    }
    phases
}

/// One tap of a Blackman-windowed sinc.
fn windowed_sinc(distance: f64) -> f64 {
    let sinc = if distance.abs() < 1e-9 {
        1.0
    } else {
        let x = core::f64::consts::PI * distance;
        x.sin() / x
    };
    // Blackman, over the filter's support. Chosen over a rectangular window for
    // the usual reason: the sidelobes of an untapered sinc put energy from one
    // transient into the estimate of the next.
    let position = (distance + count_to_f64(DETECTOR_HALF)) / (count_to_f64(DETECTOR_TAPS) - 1.0);
    if !(0.0..=1.0).contains(&position) {
        return 0.0;
    }
    let angle = 2.0 * core::f64::consts::PI * position;
    let window = 0.42 - 0.5 * angle.cos() + 0.08 * (2.0 * angle).cos();
    sinc * window
}

/// Decibels to a linear amplitude.
fn db_to_linear(decibels: f32) -> f32 {
    10.0_f32.powf(decibels / 20.0)
}

/// A linear amplitude to decibels.
fn linear_to_db(linear: f32) -> f32 {
    20.0 * linear.max(f32::EPSILON).log10()
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::float_cmp,
        clippy::cast_precision_loss,
        reason = "test fixtures index their own buffers and compare exact constants"
    )]

    use super::*;
    use prv_time::SampleRate;

    const RATE: u32 = 48_000;
    const BLOCK: usize = 128;

    fn prepared() -> Limiter {
        let mut limiter = Limiter::new();
        limiter.prepare(&PrepareConfig {
            sample_rate: SampleRate::new(RATE).expect("a valid rate"),
            max_block_frames: u32::try_from(BLOCK).expect("a sane block size"),
            channels: 2,
        });
        limiter
    }

    /// Runs a signal through in blocks and returns everything that came out.
    fn run(limiter: &mut Limiter, input: &[f32]) -> Vec<f32> {
        let context = ProcessContext::new(BLOCK, SampleRate::new(RATE).expect("rate"));
        let mut output = Vec::with_capacity(input.len());
        let mut buffer = AudioBuffer::new(2, BLOCK).expect("a buffer");

        for chunk in input.chunks(BLOCK) {
            buffer.clear();
            for channel in 0..2 {
                let data = buffer.channel_mut(channel).expect("a channel");
                for (slot, sample) in data.iter_mut().zip(chunk.iter()) {
                    *slot = *sample;
                }
            }
            limiter.process(&context, &mut buffer);
            let left = buffer.channel(0).expect("a channel");
            output.extend_from_slice(&left[..chunk.len()]);
        }
        output
    }

    /// A sine at a level, with a much louder short burst in the middle.
    fn material(level: f32, burst: f32) -> Vec<f32> {
        (0..BLOCK * 40)
            .map(|index| {
                let phase = count_to_f32(index) / count_to_f32(RATE as usize)
                    * 997.0
                    * core::f32::consts::TAU;
                let amplitude = if (2000..2100).contains(&index) {
                    burst
                } else {
                    level
                };
                phase.sin() * amplitude
            })
            .collect()
    }

    #[test]
    fn nothing_leaves_above_the_ceiling() {
        // The promise the processor exists to make. A limiter that is usually
        // under is a compressor with a marketing name, and the file it produces
        // clips on some players and not others.
        let mut limiter = prepared();
        let output = run(&mut limiter, &material(0.5, 3.0));

        // Exact in real arithmetic; in single precision the required gain is a
        // division and applying it is a multiplication, so a few units in the
        // last place are the honest tolerance. Anything larger would be a fudge
        // hiding a design that does not hold.
        let ceiling = limiter.ceiling();
        let tolerance = ceiling * (1.0 + f32::EPSILON * 4.0);
        let worst = output.iter().fold(0.0_f32, |worst, s| worst.max(s.abs()));
        assert!(
            worst <= tolerance,
            "a sample left at {worst}, above the ceiling of {ceiling}"
        );
    }

    #[test]
    fn a_signal_already_under_the_ceiling_comes_out_unchanged() {
        // Transparency is the other half of the promise. A limiter that touches
        // material it did not need to is one nobody leaves switched on.
        let mut limiter = prepared();
        let input = material(0.2, 0.2);
        let output = run(&mut limiter, &input);

        let delay = Limiter::DELAY_FRAMES;
        for (index, sample) in output.iter().enumerate().skip(delay) {
            let original = input[index - delay];
            assert!(
                (sample - original).abs() < 1e-6,
                "sample {index} changed from {original} to {sample}"
            );
        }
    }

    #[test]
    fn the_gain_moves_in_straight_lines_and_never_steps() {
        // A limiter that drops its gain the instant it sees a peak puts a
        // discontinuity into the gain, which is a click even though the sample
        // it protects is now under the ceiling.
        let mut limiter = prepared();
        let input = material(0.5, 4.0);
        let output = run(&mut limiter, &input);

        let delay = Limiter::DELAY_FRAMES;
        let mut worst_step = 0.0_f32;
        for index in delay..output.len() {
            let original = input[index - delay];
            if original.abs() < 0.05 {
                // Near a zero crossing the ratio is dominated by numerical
                // noise and says nothing about the gain.
                continue;
            }
            let applied = output[index] / original;
            let previous_original = input[index - delay - 1];
            if previous_original.abs() < 0.05 {
                continue;
            }
            let previous = output[index - 1] / previous_original;
            worst_step = worst_step.max((applied - previous).abs());
        }

        // One over the look-ahead is the steepest a straight line to a full
        // mute can be; a real signal never needs all of it.
        let steepest = 1.0 / count_to_f32(Limiter::DELAY_FRAMES);
        assert!(
            worst_step <= steepest * 1.5,
            "the gain moved by {worst_step} in one sample; a straight line to \
             silence would move by {steepest}"
        );
    }

    #[test]
    fn the_gain_is_already_down_when_the_peak_arrives() {
        // What the look-ahead is for. If the reduction started when the peak
        // did, the first millisecond of every transient would clip.
        let mut limiter = prepared();
        let mut input = vec![0.0_f32; BLOCK * 8];
        for (index, sample) in input.iter_mut().enumerate() {
            *sample = if index == BLOCK * 4 { 4.0 } else { 0.0 };
        }
        let output = run(&mut limiter, &input);

        let ceiling = limiter.ceiling();
        let worst = output.iter().fold(0.0_f32, |worst, s| worst.max(s.abs()));
        assert!(
            worst <= ceiling * (1.0 + f32::EPSILON * 4.0),
            "an isolated transient left at {worst}, above {ceiling}"
        );
    }

    #[test]
    fn it_limits_the_peak_a_converter_would_produce_not_the_sample_peak() {
        // A signal whose samples all sit under the ceiling and whose
        // reconstruction does not. If the detector read samples alone, this
        // would pass through untouched and clip on the way out of a converter.
        let mut limiter = prepared();

        // Half the sample rate, offset so every sample lands off the crest:
        // the samples are modest and the waveform between them is not.
        let input: Vec<f32> = (0..BLOCK * 8)
            .map(|index| {
                let phase = count_to_f32(index) * core::f32::consts::PI / 2.0 + 0.785;
                phase.sin() * 0.95
            })
            .collect();

        let sample_peak = input.iter().fold(0.0_f32, |worst, s| worst.max(s.abs()));
        let output = run(&mut limiter, &input);
        let out_peak = output.iter().fold(0.0_f32, |worst, s| worst.max(s.abs()));

        assert!(
            out_peak < sample_peak,
            "the detector did not act on a signal whose reconstruction exceeds \
             its samples: in {sample_peak}, out {out_peak}"
        );
    }

    #[test]
    fn the_delay_is_reported_so_the_graph_can_compensate_it() {
        let limiter = prepared();
        assert_eq!(
            limiter.latency_frames(),
            u32::try_from(Limiter::DELAY_FRAMES).expect("a sane delay"),
            "an uncompensated limiter delay moves the master against everything else"
        );
    }

    #[test]
    fn reduction_is_reported_and_is_zero_when_nothing_is_held_back() {
        let mut limiter = prepared();
        run(&mut limiter, &material(0.1, 0.1));
        assert!(
            limiter.reduction_db().abs() < 0.01,
            "quiet material reported {} dB of reduction",
            limiter.reduction_db()
        );

        let mut working = prepared();
        run(&mut working, &material(0.5, 4.0));
        assert!(
            working.reduction_db() >= 0.0,
            "reduction should be reported as a positive number of decibels"
        );
    }

    #[test]
    fn resetting_clears_the_tail() {
        // Master Prompt #15's "silence should remain silent": the last transient
        // of the previous track must not arrive under the first bar of the next.
        let mut limiter = prepared();
        run(&mut limiter, &material(0.5, 4.0));
        limiter.reset();

        let silence = vec![0.0_f32; BLOCK * 2];
        let output = run(&mut limiter, &silence);
        assert!(
            output.iter().all(|sample| *sample == 0.0),
            "audio survived a reset"
        );
    }

    #[test]
    fn a_ceiling_outside_what_a_limiter_can_mean_is_brought_back() {
        let mut limiter = prepared();

        limiter.set_ceiling_db(6.0);
        assert!(
            limiter.ceiling() <= 1.0,
            "a ceiling above full scale is a limiter that does not limit"
        );

        limiter.set_ceiling_db(-100.0);
        assert!(limiter.ceiling() >= db_to_linear(Limiter::MIN_CEILING_DB) - 1e-6);

        limiter.set_ceiling_db(f32::NAN);
        assert!((limiter.ceiling() - db_to_linear(Limiter::DEFAULT_CEILING_DB)).abs() < 1e-6);
    }

    #[test]
    fn the_default_ceiling_is_the_one_every_delivery_target_asks_for() {
        // −1 dBTP, and it agrees with `prv-export` rather than being chosen
        // again here. If the two ever disagreed, a master would be limited to
        // one figure and reported against another.
        let limiter = Limiter::new();
        assert!((limiter.ceiling() - db_to_linear(-1.0)).abs() < 1e-6);
    }

    #[test]
    fn silence_in_produces_silence_out() {
        let mut limiter = prepared();
        let output = run(&mut limiter, &vec![0.0_f32; BLOCK * 4]);
        assert!(output.iter().all(|sample| *sample == 0.0));
    }
}
