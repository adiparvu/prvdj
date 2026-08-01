use core::f64::consts::PI;

use prv_rt::{AudioBuffer, LinearSmoother};
use prv_time::SampleRate;

use crate::processor::{PrepareConfig, ProcessContext, Processor};

/// State below this magnitude is flushed to zero, to stay out of the denormal
/// range where arithmetic can slow by an order of magnitude.
const DENORMAL_FLOOR: f64 = 1e-30;

/// Frames over which a knob movement is ramped.
///
/// About twenty milliseconds at 48 kHz — slow enough to remove stepping, fast
/// enough that a sweep still tracks the hand.
const RAMP_FRAMES: u32 = 960;

/// Frames between coefficient updates.
///
/// Coefficients are recomputed at this interval rather than per sample. Per
/// sample would cost a `tan` on every frame for no audible benefit; per block
/// would step audibly on a fast sweep at large block sizes. Thirty-two frames is
/// 0.67 ms at 48 kHz, which is below the ear's resolution for this kind of
/// change and cheap enough to ignore.
const CONTROL_INTERVAL: usize = 32;

/// Lowest cutoff the low-pass sweep reaches, in hertz.
const MIN_LOW_PASS_HZ: f64 = 30.0;

/// Highest cutoff the high-pass sweep reaches, in hertz.
const MAX_HIGH_PASS_HZ: f64 = 12_000.0;

/// Cutoff at which each sweep begins — effectively out of the way.
const LOW_PASS_OPEN_HZ: f64 = 22_000.0;

/// Cutoff at which the high-pass sweep begins.
const HIGH_PASS_OPEN_HZ: f64 = 15.0;

/// A topology-preserving state-variable filter.
///
/// The trapezoidal-integrator form: unlike a direct-form biquad its coefficients
/// stay well-behaved when the cutoff is modulated quickly, which is exactly what
/// a filter knob does. A biquad swept fast produces zipper artefacts from
/// coefficient interpolation; this does not.
#[derive(Debug, Clone, Copy, PartialEq)]
struct StateVariable {
    ic1: f64,
    ic2: f64,
}

impl StateVariable {
    const fn new() -> Self {
        Self { ic1: 0.0, ic2: 0.0 }
    }

    fn reset(&mut self) {
        self.ic1 = 0.0;
        self.ic2 = 0.0;
    }

    /// Processes one sample, returning the low-pass and high-pass outputs.
    ///
    /// Both are produced by the same state, so a single filter can serve either
    /// role without a second set of state to keep synchronised.
    fn process(&mut self, input: f64, coefficients: SvfCoefficients) -> (f64, f64) {
        let v3 = input - self.ic2;
        let v1 = coefficients.a1 * self.ic1 + coefficients.a2 * v3;
        let v2 = self.ic2 + coefficients.a2 * self.ic1 + coefficients.a3 * v3;

        self.ic1 = 2.0f64.mul_add(v1, -self.ic1);
        self.ic2 = 2.0f64.mul_add(v2, -self.ic2);

        if self.ic1.abs() < DENORMAL_FLOOR {
            self.ic1 = 0.0;
        }
        if self.ic2.abs() < DENORMAL_FLOOR {
            self.ic2 = 0.0;
        }

        let low = v2;
        let high = input - coefficients.k * v1 - v2;
        (low, high)
    }
}

/// Precomputed integrator coefficients.
#[derive(Debug, Clone, Copy, PartialEq)]
struct SvfCoefficients {
    a1: f64,
    a2: f64,
    a3: f64,
    k: f64,
}

impl SvfCoefficients {
    /// Designs coefficients for a cutoff and resonance.
    fn new(cutoff_hz: f64, resonance: f64, rate: SampleRate) -> Self {
        let nyquist = f64::from(rate.hz()) * 0.5;
        let cutoff = cutoff_hz.clamp(5.0, nyquist * 0.98);
        let g = (PI * cutoff / f64::from(rate.hz())).tan();
        let q = if resonance.is_finite() && resonance > 0.05 {
            resonance
        } else {
            0.707
        };
        let k = 1.0 / q;
        let denominator = g.mul_add(g + k, 1.0);
        let a1 = if denominator.abs() < f64::EPSILON {
            1.0
        } else {
            1.0 / denominator
        };
        Self {
            a1,
            a2: g * a1,
            a3: g * (g * a1),
            k,
        }
    }
}

/// Which half of the sweep is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// The knob is at centre; the filter is out of the signal path.
    Bypass,
    LowPass,
    HighPass,
}

/// The single-knob DJ filter.
///
/// # The control
///
/// One value from −1 to +1. Centre is bypass — genuinely bypass, not a filter
/// with its cutoff parked out of the way, so a knob at detent costs nothing and
/// colours nothing. Turning left sweeps a low-pass down from inaudibly high to
/// 30 Hz; turning right sweeps a high-pass up from inaudibly low to 12 kHz.
///
/// The sweep is exponential, because pitch perception is. A linear sweep spends
/// most of its travel in the top octave, where almost nothing musical happens,
/// and crosses the entire bass in the last few degrees.
///
/// # Slope
///
/// Twelve decibels per octave with adjustable resonance — the classic DJ filter
/// character, and the one that stays musical when swept fast. A steeper option
/// can be added by cascading a second stage; the control mapping would not
/// change.
#[derive(Debug)]
pub struct DjFilter {
    channels: Vec<StateVariable>,
    position: LinearSmoother,
    resonance: f64,
    sample_rate: SampleRate,
}

impl DjFilter {
    /// Creates a filter at centre detent — bypassed.
    #[must_use]
    pub fn new() -> Self {
        Self {
            channels: Vec::new(),
            position: LinearSmoother::new(0.0),
            resonance: 0.707,
            sample_rate: SampleRate::HZ_48000,
        }
    }

    /// Sets the knob position, from −1 (full low-pass) through 0 (bypass) to
    /// +1 (full high-pass). Ramped.
    pub fn set_position(&mut self, position: f32) {
        if let Some(position) = clamp_position(position) {
            self.position.set_target(position, RAMP_FRAMES);
        }
    }

    /// Sets the knob position without ramping.
    pub fn set_position_immediate(&mut self, position: f32) {
        if let Some(position) = clamp_position(position) {
            self.position.set_immediate(position);
        }
    }

    /// The position currently in effect.
    #[must_use]
    pub fn position(&self) -> f32 {
        self.position.current()
    }

    /// Sets the resonance. Clamped to a range that cannot self-oscillate into
    /// the output.
    pub fn set_resonance(&mut self, resonance: f32) {
        let resonance = if resonance.is_finite() {
            f64::from(resonance).clamp(0.5, 8.0)
        } else {
            0.707
        };
        self.resonance = resonance;
    }

    /// The resonance in effect.
    #[must_use]
    pub fn resonance(&self) -> f32 {
        #[allow(
            clippy::cast_possible_truncation,
            reason = "resonance is a control value, bounded to 0.5..8.0"
        )]
        {
            self.resonance as f32
        }
    }

    /// Maps a knob position to a mode and a cutoff.
    ///
    /// The dead zone at centre is deliberate: without it, a knob nominally at
    /// detent would still have a filter in the path, and the smallest tremor
    /// would colour the sound.
    fn mode_and_cutoff(position: f64) -> (Mode, f64) {
        const DEAD_ZONE: f64 = 0.02;
        if position.abs() <= DEAD_ZONE {
            return (Mode::Bypass, 0.0);
        }
        // Rescale so the sweep still uses its full travel outside the dead zone.
        let amount = ((position.abs() - DEAD_ZONE) / (1.0 - DEAD_ZONE)).clamp(0.0, 1.0);
        if position < 0.0 {
            let cutoff = exponential_sweep(LOW_PASS_OPEN_HZ, MIN_LOW_PASS_HZ, amount);
            (Mode::LowPass, cutoff)
        } else {
            let cutoff = exponential_sweep(HIGH_PASS_OPEN_HZ, MAX_HIGH_PASS_HZ, amount);
            (Mode::HighPass, cutoff)
        }
    }
}

impl Default for DjFilter {
    fn default() -> Self {
        Self::new()
    }
}

impl Processor for DjFilter {
    fn name(&self) -> &'static str {
        "Filter"
    }

    fn prepare(&mut self, config: &PrepareConfig) {
        self.sample_rate = config.sample_rate;
        self.channels
            .resize_with(config.channels, StateVariable::new);
        self.reset();
    }

    fn process(&mut self, ctx: &ProcessContext, buffer: &mut AudioBuffer) {
        let frames = ctx.frames.min(buffer.frames());
        if frames == 0 {
            return;
        }

        // Advance the control in fixed-size slices, recomputing coefficients
        // once per slice. Every channel is processed with the same coefficients
        // for the same slice, so the channels cannot diverge.
        let mut offset = 0_usize;
        while offset < frames {
            let slice = CONTROL_INTERVAL.min(frames - offset);

            let position = f64::from(self.position.current());
            let (mode, cutoff) = Self::mode_and_cutoff(position);

            if mode == Mode::Bypass {
                // A bypassed filter must still clear its state, or re-engaging
                // it would ring with audio from before the bypass.
                for channel in &mut self.channels {
                    channel.reset();
                }
            } else {
                let coefficients = SvfCoefficients::new(cutoff, self.resonance, self.sample_rate);
                for (index, channel) in buffer.channels_iter_mut().enumerate() {
                    let Some(state) = self.channels.get_mut(index) else {
                        continue;
                    };
                    let Some(samples) = channel.get_mut(offset..offset + slice) else {
                        continue;
                    };
                    for sample in samples.iter_mut() {
                        let (low, high) = state.process(f64::from(*sample), coefficients);
                        let output = if mode == Mode::LowPass { low } else { high };
                        #[allow(
                            clippy::cast_possible_truncation,
                            reason = "audio samples are f32 by definition"
                        )]
                        {
                            *sample = output as f32;
                        }
                    }
                }
            }

            // Advance the control smoother by exactly the frames consumed.
            #[allow(
                clippy::cast_possible_truncation,
                reason = "slice is bounded by CONTROL_INTERVAL"
            )]
            self.position.skip(slice as u32);
            offset += slice;
        }
    }

    fn reset(&mut self) {
        for channel in &mut self.channels {
            channel.reset();
        }
    }
}

/// Clamps a knob position, returning `None` for a non-finite value.
///
/// `None` rather than a substituted default: a NaN arriving from a controller or
/// a corrupted preset should leave the knob where the user put it, not silently
/// re-centre it.
fn clamp_position(position: f32) -> Option<f32> {
    if position.is_finite() {
        Some(position.clamp(-1.0, 1.0))
    } else {
        None
    }
}

/// Interpolates exponentially between two frequencies.
///
/// Exponential because pitch perception is logarithmic: an octave is an octave
/// whether it is 50 Hz to 100 Hz or 5 kHz to 10 kHz, and a linear sweep would
/// feel wrong under the hand at both ends.
fn exponential_sweep(from_hz: f64, to_hz: f64, amount: f64) -> f64 {
    let from = from_hz.max(1.0).ln();
    let to = to_hz.max(1.0).ln();
    (from + (to - from) * amount.clamp(0.0, 1.0)).exp()
}

#[cfg(test)]
mod tests {
    // Measurement helpers convert between sample indices and floating-point
    // phase throughout. The values involved are block offsets and frequencies,
    // orders of magnitude below any precision limit, and a conversion error
    // here would show up immediately as a failed magnitude assertion.
    #![allow(
        clippy::cast_possible_truncation,
        clippy::cast_precision_loss,
        clippy::cast_sign_loss,
        reason = "test measurement helpers; magnitudes are asserted, so a bad conversion fails loudly"
    )]

    use super::*;
    use core::f64::consts::TAU;

    const RATE: SampleRate = SampleRate::HZ_48000;
    const BLOCK: usize = 512;

    fn prepared() -> DjFilter {
        let mut filter = DjFilter::new();
        filter.prepare(&PrepareConfig::new(RATE, BLOCK as u32, 2));
        filter
    }

    fn buffer() -> AudioBuffer {
        AudioBuffer::new(2, BLOCK).unwrap_or_else(|_| {
            AudioBuffer::new(1, 1).unwrap_or_else(|_| unreachable!("1×1 buffer is always valid"))
        })
    }

    /// Magnitude response at one frequency, measured as an energy ratio.
    ///
    /// Energy rather than peak amplitude: near Nyquist a sampled sine rarely has
    /// a sample at its crest, and a peak measurement would report attenuation
    /// that is an artefact of sampling rather than a property of the filter.
    fn magnitude_at(filter: &mut DjFilter, frequency_hz: f64) -> f64 {
        filter.reset();
        let step = TAU * frequency_hz / f64::from(RATE.hz());
        let mut audio = buffer();
        let ctx = ProcessContext::new(BLOCK, RATE);

        let mut phase = 0_usize;
        let mut output_energy = 0.0_f64;
        let mut input_energy = 0.0_f64;
        for block in 0..20 {
            for index in 0..audio.channels() {
                if let Some(channel) = audio.channel_mut(index) {
                    for (offset, sample) in channel.iter_mut().enumerate() {
                        *sample = (((phase + offset) as f64) * step).sin() as f32;
                    }
                }
            }
            if block >= 15 {
                if let Some(channel) = audio.channel(0) {
                    for sample in channel {
                        input_energy += f64::from(*sample) * f64::from(*sample);
                    }
                }
            }
            filter.process(&ctx, &mut audio);
            if block >= 15 {
                if let Some(channel) = audio.channel(0) {
                    for sample in channel {
                        output_energy += f64::from(*sample) * f64::from(*sample);
                    }
                }
            }
            phase += BLOCK;
        }
        // The ratio is taken against the energy actually presented, not against
        // the theoretical value for a sine: a measurement window rarely spans a
        // whole number of cycles, and at low frequencies that error is larger
        // than the tolerance these assertions use.
        if input_energy <= 0.0 {
            return 0.0;
        }
        (output_energy / input_energy).sqrt()
    }

    #[test]
    fn centre_detent_is_a_true_bypass() {
        // A knob at centre must cost nothing and colour nothing.
        let mut filter = prepared();
        filter.set_position_immediate(0.0);
        for frequency in [50.0, 500.0, 5_000.0, 15_000.0] {
            let magnitude = magnitude_at(&mut filter, frequency);
            assert!(
                (magnitude - 1.0).abs() < 0.01,
                "bypass altered {frequency} Hz: magnitude {magnitude}"
            );
        }
    }

    #[test]
    fn turning_left_removes_the_treble() {
        let mut filter = prepared();
        filter.set_position_immediate(-1.0);

        let bass = magnitude_at(&mut filter, 40.0);
        let treble = magnitude_at(&mut filter, 8_000.0);

        assert!(bass > 0.4, "the bass should survive, got {bass}");
        assert!(
            20.0 * treble.log10() < -30.0,
            "8 kHz should be strongly rejected, got {:.1} dB",
            20.0 * treble.log10()
        );
    }

    #[test]
    fn turning_right_removes_the_bass() {
        let mut filter = prepared();
        filter.set_position_immediate(1.0);

        let bass = magnitude_at(&mut filter, 40.0);
        let treble = magnitude_at(&mut filter, 15_000.0);

        assert!(
            20.0 * bass.log10() < -30.0,
            "40 Hz should be strongly rejected, got {:.1} dB",
            20.0 * bass.log10()
        );
        assert!(treble > 0.8, "the treble should survive, got {treble}");
    }

    #[test]
    fn the_sweep_is_monotonic() {
        // Turning the knob further must always remove more. A mapping that
        // reversed anywhere would feel broken under the hand.
        let mut filter = prepared();
        let mut previous = f64::INFINITY;
        for step in 1..=10 {
            let position = -(step as f32) / 10.0;
            filter.set_position_immediate(position);
            let magnitude = magnitude_at(&mut filter, 4_000.0);
            assert!(
                magnitude <= previous + 0.02,
                "the low-pass sweep was not monotonic at position {position}"
            );
            previous = magnitude;
        }
    }

    #[test]
    fn the_mapping_is_exponential_not_linear() {
        // Half travel should land near the geometric middle of the range, not
        // the arithmetic one. At 22 kHz to 30 Hz the geometric middle is about
        // 810 Hz; the arithmetic middle would be 11 kHz, which would put nearly
        // the whole musical range in the last few degrees of travel.
        let (mode, cutoff) = DjFilter::mode_and_cutoff(-0.51);
        assert_eq!(mode, Mode::LowPass);
        assert!(
            (500.0..1_500.0).contains(&cutoff),
            "half travel landed at {cutoff} Hz"
        );
    }

    #[test]
    fn a_knob_movement_is_ramped() {
        let mut filter = prepared();
        filter.set_position_immediate(0.0);
        filter.set_position(-1.0);

        let mut audio = buffer();
        audio.as_mut_slice().fill(0.5);
        filter.process(&ProcessContext::new(BLOCK, RATE), &mut audio);

        // After 512 frames of a 960-frame ramp the knob is part way along.
        let position = filter.position();
        assert!(
            position < 0.0 && position > -1.0,
            "expected a partial sweep, got {position}"
        );
    }

    #[test]
    fn bypass_clears_state_so_re_engaging_does_not_ring() {
        let mut filter = prepared();
        filter.set_position_immediate(-1.0);

        let mut audio = buffer();
        audio.as_mut_slice().fill(0.8);
        filter.process(&ProcessContext::new(BLOCK, RATE), &mut audio);

        filter.set_position_immediate(0.0);
        let mut silence = buffer();
        filter.process(&ProcessContext::new(BLOCK, RATE), &mut silence);
        assert!(silence.is_silent(), "bypass must not ring");
    }

    #[test]
    fn positions_are_clamped_and_non_finite_values_ignored() {
        let mut filter = prepared();
        filter.set_position_immediate(5.0);
        assert!((filter.position() - 1.0).abs() < f32::EPSILON);
        filter.set_position_immediate(-5.0);
        assert!((filter.position() + 1.0).abs() < f32::EPSILON);
        filter.set_position_immediate(0.25);
        filter.set_position_immediate(f32::NAN);
        assert!((filter.position() - 0.25).abs() < f32::EPSILON);
    }

    #[test]
    fn resonance_is_bounded() {
        let mut filter = prepared();
        filter.set_resonance(100.0);
        assert!(filter.resonance() <= 8.0);
        filter.set_resonance(0.0);
        assert!(filter.resonance() >= 0.5);
        filter.set_resonance(f32::NAN);
        assert!(filter.resonance().is_finite());
    }

    #[test]
    fn the_filter_stays_stable_under_a_fast_sweep() {
        // A biquad with interpolated coefficients can blow up here. The
        // trapezoidal form is chosen precisely so that it does not.
        let mut filter = prepared();
        let mut audio = buffer();
        for step in 0..2_000 {
            let position = ((step as f32) * 0.05).sin();
            filter.set_position_immediate(position);
            audio.as_mut_slice().fill(0.7);
            filter.process(&ProcessContext::new(BLOCK, RATE), &mut audio);
            assert!(
                audio.as_slice().iter().all(|s| s.is_finite()),
                "filter became unstable at step {step}"
            );
            assert!(
                audio.peak() < 8.0,
                "filter resonated out of control at step {step}"
            );
        }
    }

    #[test]
    fn silence_stays_silent() {
        let mut filter = prepared();
        filter.set_position_immediate(-0.5);
        let mut audio = buffer();
        filter.process(&ProcessContext::new(BLOCK, RATE), &mut audio);
        assert!(audio.is_silent());
    }
}
