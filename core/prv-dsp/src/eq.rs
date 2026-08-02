use prv_rt::{AudioBuffer, LinearSmoother};
use prv_time::SampleRate;

use crate::biquad::{Biquad, BiquadCoefficients, BUTTERWORTH_Q};
use crate::processor::{PrepareConfig, ProcessContext, Processor};

/// Default lower crossover, in hertz.
///
/// 200 Hz is where a DJ mixer's low band sits: high enough to take the kick and
/// the bass line together, low enough that killing it does not thin the vocal.
pub const DEFAULT_LOW_CROSSOVER_HZ: f64 = 200.0;

/// Default upper crossover, in hertz.
///
/// 2 kHz keeps vocal presence and hats in the high band while leaving the body
/// of the vocal in the mid, which is what makes a mid kill usable for swapping
/// vocals between two tracks.
pub const DEFAULT_HIGH_CROSSOVER_HZ: f64 = 2_000.0;

/// Frames over which a band gain change is ramped.
///
/// Roughly five milliseconds at 48 kHz: fast enough that a hard kill feels
/// instant under the hand, slow enough that the discontinuity is inaudible.
const GAIN_RAMP_FRAMES: u32 = 256;

/// The largest gain a band will accept, linear.
///
/// About +12 dB. Bounded because an unbounded band gain is the fastest route to
/// a clipped master, and Master Prompt #3B forbids abrupt level jumps.
const MAX_BAND_GAIN: f32 = 4.0;

/// Per-channel filter state.
///
/// Nine second-order sections: two cascaded pairs for each crossover, forming
/// fourth-order Linkwitz-Riley slopes, plus the all-pass that keeps the low band
/// in phase with the other two.
#[derive(Debug, Clone, Copy, PartialEq)]
struct ChannelState {
    low_pass_low: [Biquad; 2],
    high_pass_low: [Biquad; 2],
    low_pass_high: [Biquad; 2],
    high_pass_high: [Biquad; 2],
    all_pass_high: Biquad,
}

impl ChannelState {
    const fn new() -> Self {
        Self {
            low_pass_low: [Biquad::new(); 2],
            high_pass_low: [Biquad::new(); 2],
            low_pass_high: [Biquad::new(); 2],
            high_pass_high: [Biquad::new(); 2],
            all_pass_high: Biquad::new(),
        }
    }

    fn reset(&mut self) {
        for section in &mut self.low_pass_low {
            section.reset();
        }
        for section in &mut self.high_pass_low {
            section.reset();
        }
        for section in &mut self.low_pass_high {
            section.reset();
        }
        for section in &mut self.high_pass_high {
            section.reset();
        }
        self.all_pass_high.reset();
    }
}

/// The three-band DJ equaliser.
///
/// # Why a crossover rather than three shelving filters
///
/// The obvious implementation of a three-band equaliser is a low shelf, a peak
/// and a high shelf. It is simpler and it is wrong for this instrument, because
/// a shelf cannot remove a band: turned fully down it leaves a residue, and a DJ
/// killing the bass to bring in another track's bass expects *silence* in that
/// band, not −18 dB of it.
///
/// So the signal is split into three bands by a crossover, each band is scaled,
/// and the three are summed. A gain of zero is then a true kill.
///
/// # Why the bands sum back to flat
///
/// A fourth-order Linkwitz-Riley crossover has the property that its low and
/// high outputs sum to an all-pass — flat magnitude, shifted phase. Splitting
/// twice to get three bands means the low band has been through one crossover
/// while the other two have been through two, so summing them naively leaves a
/// dip at the upper crossover: a hollow midrange on a channel nobody has
/// touched.
///
/// Passing the low band through an all-pass matching the second crossover
/// restores the alignment. With every band at unity the equaliser is then
/// magnitude-flat, which is exactly what a performer expects from a knob at
/// centre detent, and what the test suite asserts.
#[derive(Debug)]
pub struct ThreeBandEq {
    channels: Vec<ChannelState>,
    low_gain: LinearSmoother,
    mid_gain: LinearSmoother,
    high_gain: LinearSmoother,
    low_ramp: Vec<f32>,
    mid_ramp: Vec<f32>,
    high_ramp: Vec<f32>,
    low_crossover_hz: f64,
    high_crossover_hz: f64,
    sample_rate: SampleRate,
}

impl ThreeBandEq {
    /// Creates an equaliser at unity on every band.
    #[must_use]
    pub fn new() -> Self {
        Self {
            channels: Vec::new(),
            low_gain: LinearSmoother::new(1.0),
            mid_gain: LinearSmoother::new(1.0),
            high_gain: LinearSmoother::new(1.0),
            low_ramp: Vec::new(),
            mid_ramp: Vec::new(),
            high_ramp: Vec::new(),
            low_crossover_hz: DEFAULT_LOW_CROSSOVER_HZ,
            high_crossover_hz: DEFAULT_HIGH_CROSSOVER_HZ,
            sample_rate: SampleRate::HZ_48000,
        }
    }

    /// Sets the three band gains, linear, ramped to avoid a click.
    ///
    /// Zero is a true kill. Values are clamped to a sane range; a non-finite
    /// value is ignored rather than propagated into the signal path.
    pub fn set_gains(&mut self, low: f32, mid: f32, high: f32) {
        self.low_gain.set_target(clamp_gain(low), GAIN_RAMP_FRAMES);
        self.mid_gain.set_target(clamp_gain(mid), GAIN_RAMP_FRAMES);
        self.high_gain
            .set_target(clamp_gain(high), GAIN_RAMP_FRAMES);
    }

    /// Sets the three band gains without ramping.
    ///
    /// For initialisation and for loading a preset, where there is no previous
    /// value to glide from.
    pub fn set_gains_immediate(&mut self, low: f32, mid: f32, high: f32) {
        self.low_gain.set_immediate(clamp_gain(low));
        self.mid_gain.set_immediate(clamp_gain(mid));
        self.high_gain.set_immediate(clamp_gain(high));
    }

    /// The band gains currently in effect.
    #[must_use]
    pub fn gains(&self) -> (f32, f32, f32) {
        (
            self.low_gain.current(),
            self.mid_gain.current(),
            self.high_gain.current(),
        )
    }

    /// Moves the crossover points.
    ///
    /// The upper crossover is kept above the lower one; an inverted pair would
    /// produce a mid band of negative width, which is meaningless.
    pub fn set_crossovers(&mut self, low_hz: f64, high_hz: f64) {
        let nyquist = f64::from(self.sample_rate.hz()) * 0.5;

        // A crossover arrives from outside — an automation curve, a plugin, a
        // project written by another build. `clamp` *propagates* a non-finite
        // value rather than removing it, so one NaN here designs a filter of
        // non-finite coefficients, every sample after it is non-finite, and
        // `reset` does not recover: it clears state, not coefficients. The deck
        // would be silent for the rest of the session.
        //
        // Keeping the previous value is the only answer that leaves the audio
        // playable. A caller that wanted a different crossover and sent
        // nonsense has a defect; a listener should not hear it.
        if low_hz.is_finite() {
            self.low_crossover_hz = low_hz.clamp(20.0, nyquist * 0.4);
        }
        if high_hz.is_finite() {
            self.high_crossover_hz = high_hz.clamp(self.low_crossover_hz * 1.5, nyquist * 0.9);
        } else {
            // The low crossover may have moved, and the high one must stay above
            // it or the bands overlap.
            self.high_crossover_hz = self
                .high_crossover_hz
                .clamp(self.low_crossover_hz * 1.5, nyquist * 0.9);
        }
        self.design();
    }

    /// Recomputes every coefficient from the current crossovers.
    ///
    /// Never called from the audio thread.
    fn design(&mut self) {
        let rate = self.sample_rate;
        let low_pass_low = BiquadCoefficients::low_pass(self.low_crossover_hz, BUTTERWORTH_Q, rate);
        let high_pass_low =
            BiquadCoefficients::high_pass(self.low_crossover_hz, BUTTERWORTH_Q, rate);
        let low_pass_high =
            BiquadCoefficients::low_pass(self.high_crossover_hz, BUTTERWORTH_Q, rate);
        let high_pass_high =
            BiquadCoefficients::high_pass(self.high_crossover_hz, BUTTERWORTH_Q, rate);
        let all_pass_high =
            BiquadCoefficients::all_pass(self.high_crossover_hz, BUTTERWORTH_Q, rate);

        for channel in &mut self.channels {
            for section in &mut channel.low_pass_low {
                section.set_coefficients(low_pass_low);
            }
            for section in &mut channel.high_pass_low {
                section.set_coefficients(high_pass_low);
            }
            for section in &mut channel.low_pass_high {
                section.set_coefficients(low_pass_high);
            }
            for section in &mut channel.high_pass_high {
                section.set_coefficients(high_pass_high);
            }
            channel.all_pass_high.set_coefficients(all_pass_high);
        }
    }
}

impl Default for ThreeBandEq {
    fn default() -> Self {
        Self::new()
    }
}

impl Processor for ThreeBandEq {
    fn name(&self) -> &'static str {
        "3-band EQ"
    }

    fn prepare(&mut self, config: &PrepareConfig) {
        self.sample_rate = config.sample_rate;
        self.channels
            .resize_with(config.channels, ChannelState::new);
        let block = config.max_block_frames as usize;
        // Gain ramps are computed once per block and reused for every channel,
        // so that a smoother advances exactly once per frame however many
        // channels there are. Allocated here, never in `process`.
        self.low_ramp.resize(block, 1.0);
        self.mid_ramp.resize(block, 1.0);
        self.high_ramp.resize(block, 1.0);
        self.design();
        self.reset();
    }

    fn process(&mut self, ctx: &ProcessContext, buffer: &mut AudioBuffer) {
        let frames = ctx.frames.min(buffer.frames()).min(self.low_ramp.len());
        if frames == 0 {
            return;
        }

        if let Some(ramp) = self.low_ramp.get_mut(..frames) {
            self.low_gain.fill(ramp);
        }
        if let Some(ramp) = self.mid_ramp.get_mut(..frames) {
            self.mid_gain.fill(ramp);
        }
        if let Some(ramp) = self.high_ramp.get_mut(..frames) {
            self.high_gain.fill(ramp);
        }

        for (index, channel) in buffer.channels_iter_mut().enumerate() {
            let Some(state) = self.channels.get_mut(index) else {
                continue;
            };
            let Some(samples) = channel.get_mut(..frames) else {
                continue;
            };

            for (position, sample) in samples.iter_mut().enumerate() {
                let input = f64::from(*sample);

                // First crossover: split into everything below and everything
                // above the lower point.
                let mut below = input;
                for section in &mut state.low_pass_low {
                    below = section.process(below);
                }
                let mut above = input;
                for section in &mut state.high_pass_low {
                    above = section.process(above);
                }

                // Second crossover, applied to the upper half only.
                let mut mid = above;
                for section in &mut state.low_pass_high {
                    mid = section.process(mid);
                }
                let mut high = above;
                for section in &mut state.high_pass_high {
                    high = section.process(high);
                }

                // Phase-align the low band with the two that went through the
                // second crossover, so the three sum back to flat.
                let low = state.all_pass_high.process(below);

                let low_gain = f64::from(self.low_ramp.get(position).copied().unwrap_or(1.0));
                let mid_gain = f64::from(self.mid_ramp.get(position).copied().unwrap_or(1.0));
                let high_gain = f64::from(self.high_ramp.get(position).copied().unwrap_or(1.0));

                let output = low * low_gain + mid * mid_gain + high * high_gain;
                #[allow(
                    clippy::cast_possible_truncation,
                    reason = "audio samples are f32 by definition; f64 is used only inside the filter"
                )]
                {
                    *sample = output as f32;
                }
            }
        }
    }

    fn reset(&mut self) {
        for channel in &mut self.channels {
            channel.reset();
        }
    }
}

/// Clamps a band gain, ignoring a non-finite value.
fn clamp_gain(gain: f32) -> f32 {
    if gain.is_finite() {
        gain.clamp(0.0, MAX_BAND_GAIN)
    } else {
        1.0
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::panic,
        reason = "a test that cannot build its own fixture should fail loudly"
    )]
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

    fn prepared() -> ThreeBandEq {
        let mut eq = ThreeBandEq::new();
        eq.prepare(&PrepareConfig::new(RATE, BLOCK as u32, 2));
        eq
    }

    /// Drives the equaliser with a sine and returns the magnitude response.
    ///
    /// The settling period is discarded so that the filter's start-up transient
    /// does not contaminate the measurement, and the result is an energy ratio
    /// rather than a peak: a sampled sine near Nyquist rarely has a sample at
    /// its crest, so a peak measurement would report attenuation that is an
    /// artefact of sampling rather than a property of the filter.
    fn magnitude_at(eq: &mut ThreeBandEq, frequency_hz: f64) -> f64 {
        eq.reset();
        let step = TAU * frequency_hz / f64::from(RATE.hz());
        let mut buffer = AudioBuffer::new(2, BLOCK).unwrap_or_else(|_| {
            AudioBuffer::new(1, 1).unwrap_or_else(|_| unreachable!("1×1 buffer is always valid"))
        });
        let ctx = ProcessContext::new(BLOCK, RATE);

        let mut phase = 0_usize;
        let mut output_energy = 0.0_f64;
        let mut input_energy = 0.0_f64;
        // Twenty blocks: the first fifteen settle, the last five are measured.
        for block in 0..20 {
            for channel_index in 0..buffer.channels() {
                if let Some(channel) = buffer.channel_mut(channel_index) {
                    for (offset, sample) in channel.iter_mut().enumerate() {
                        *sample = (((phase + offset) as f64) * step).sin() as f32;
                    }
                }
            }
            if block >= 15 {
                if let Some(channel) = buffer.channel(0) {
                    for sample in channel {
                        input_energy += f64::from(*sample) * f64::from(*sample);
                    }
                }
            }
            eq.process(&ctx, &mut buffer);
            if block >= 15 {
                if let Some(channel) = buffer.channel(0) {
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
    fn at_unity_the_response_is_flat() {
        // The property that the all-pass compensation exists to provide. Without
        // it there is a dip at the upper crossover with every band at centre —
        // a hollow midrange on an untouched channel.
        let mut eq = prepared();
        eq.set_gains_immediate(1.0, 1.0, 1.0);

        for frequency in [
            50.0, 100.0, 200.0, 400.0, 800.0, 1_500.0, 2_000.0, 3_000.0, 6_000.0, 12_000.0,
        ] {
            let magnitude = magnitude_at(&mut eq, frequency);
            let decibels = 20.0 * magnitude.log10();
            assert!(
                decibels.abs() < 0.6,
                "at unity the response must be flat; {frequency} Hz was {decibels:.2} dB"
            );
        }
    }

    #[test]
    fn killing_the_low_band_removes_the_bass_and_leaves_the_rest() {
        let mut eq = prepared();
        eq.set_gains_immediate(0.0, 1.0, 1.0);

        let bass = magnitude_at(&mut eq, 50.0);
        let treble = magnitude_at(&mut eq, 8_000.0);

        let bass_db = 20.0 * bass.log10();
        assert!(
            bass_db < -30.0,
            "a kill must be a kill, not an attenuation; 50 Hz was {bass_db:.1} dB"
        );
        assert!(
            (20.0 * treble.log10()).abs() < 0.6,
            "the untouched bands must be unaffected"
        );
    }

    #[test]
    fn killing_the_high_band_removes_the_treble() {
        let mut eq = prepared();
        eq.set_gains_immediate(1.0, 1.0, 0.0);

        let treble = 20.0 * magnitude_at(&mut eq, 12_000.0).log10();
        let bass = 20.0 * magnitude_at(&mut eq, 50.0).log10();

        assert!(treble < -30.0, "12 kHz was {treble:.1} dB");
        assert!(bass.abs() < 0.6, "50 Hz was {bass:.1} dB");
    }

    #[test]
    fn killing_the_mid_band_removes_the_middle() {
        let mut eq = prepared();
        eq.set_gains_immediate(1.0, 0.0, 1.0);

        let mid = 20.0 * magnitude_at(&mut eq, 700.0).log10();
        assert!(mid < -20.0, "700 Hz was {mid:.1} dB");
    }

    #[test]
    fn killing_every_band_produces_silence() {
        let mut eq = prepared();
        eq.set_gains_immediate(0.0, 0.0, 0.0);
        let magnitude = magnitude_at(&mut eq, 1_000.0);
        assert!(magnitude < 1e-4, "expected silence, got {magnitude}");
    }

    #[test]
    fn a_gain_change_is_ramped_rather_than_stepped() {
        // A step in gain is a discontinuity, and a discontinuity is a click.
        let mut eq = prepared();
        eq.set_gains_immediate(1.0, 1.0, 1.0);
        eq.set_gains(0.0, 0.0, 0.0);

        let mut buffer = AudioBuffer::new(1, 64).unwrap_or_else(|_| {
            AudioBuffer::new(1, 1).unwrap_or_else(|_| unreachable!("1×1 buffer is always valid"))
        });
        eq.prepare(&PrepareConfig::new(RATE, 64, 1));
        eq.set_gains_immediate(1.0, 1.0, 1.0);
        eq.set_gains(0.0, 0.0, 0.0);

        if let Some(channel) = buffer.channel_mut(0) {
            channel.fill(0.5);
        }
        eq.process(&ProcessContext::new(64, RATE), &mut buffer);

        // After 64 frames of a 256-frame ramp the gain is still well above zero,
        // which is what "ramped" means.
        let (low, mid, high) = eq.gains();
        assert!(low > 0.0 && low < 1.0, "low gain mid-ramp was {low}");
        assert!(mid > 0.0 && mid < 1.0);
        assert!(high > 0.0 && high < 1.0);
    }

    #[test]
    fn gains_are_clamped_and_non_finite_values_are_ignored() {
        let mut eq = prepared();
        eq.set_gains_immediate(100.0, -5.0, f32::NAN);
        let (low, mid, high) = eq.gains();
        assert!((low - MAX_BAND_GAIN).abs() < f32::EPSILON, "got {low}");
        assert!(
            mid.abs() < f32::EPSILON,
            "negative gain must clamp to a kill"
        );
        assert!((high - 1.0).abs() < f32::EPSILON, "NaN must be ignored");
    }

    #[test]
    fn crossovers_cannot_be_inverted() {
        let mut eq = prepared();
        eq.set_crossovers(5_000.0, 100.0);
        assert!(
            eq.high_crossover_hz > eq.low_crossover_hz,
            "the upper crossover must stay above the lower one"
        );
    }

    #[test]
    fn silence_in_produces_silence_out() {
        let mut eq = prepared();
        let mut buffer = AudioBuffer::new(2, BLOCK).unwrap_or_else(|_| {
            AudioBuffer::new(1, 1).unwrap_or_else(|_| unreachable!("1×1 buffer is always valid"))
        });
        eq.process(&ProcessContext::new(BLOCK, RATE), &mut buffer);
        assert!(buffer.is_silent(), "silence must remain silent");
    }

    #[test]
    fn a_block_shorter_than_the_maximum_is_handled() {
        let mut eq = prepared();
        let mut buffer = AudioBuffer::new(2, BLOCK).unwrap_or_else(|_| {
            AudioBuffer::new(1, 1).unwrap_or_else(|_| unreachable!("1×1 buffer is always valid"))
        });
        if let Some(channel) = buffer.channel_mut(0) {
            channel.fill(0.25);
        }
        // The host may hand over a partial block at the end of a stream.
        eq.process(&ProcessContext::new(64, RATE), &mut buffer);
        for index in 64..BLOCK {
            let value = buffer.channel(0).and_then(|c| c.get(index)).copied();
            assert_eq!(
                value,
                Some(0.25),
                "frames beyond the block must be left untouched"
            );
        }
    }

    #[test]
    fn a_non_finite_crossover_never_reaches_the_coefficients() {
        // `clamp` propagates NaN rather than removing it. One of these would
        // have made every sample non-finite for the rest of the session, and
        // `reset` would not have recovered — it clears state, not coefficients.
        let rate = SampleRate::HZ_48000;
        for (low, high) in [
            (f64::NAN, 4_000.0),
            (200.0, f64::NAN),
            (f64::NAN, f64::NAN),
            (f64::INFINITY, f64::NEG_INFINITY),
        ] {
            let mut equaliser = ThreeBandEq::new();
            equaliser.prepare(&PrepareConfig::new(rate, 64, 1));
            equaliser.set_crossovers(low, high);

            let mut buffer = AudioBuffer::new(1, 64).expect("a buffer");
            if let Some(channel) = buffer.channel_mut(0) {
                channel.fill(0.5);
            }
            equaliser.process(&ProcessContext::new(64, rate), &mut buffer);

            let bad = buffer
                .channel(0)
                .map_or(0, |data| data.iter().filter(|s| !s.is_finite()).count());
            assert_eq!(
                bad, 0,
                "crossovers ({low}, {high}) produced {bad} non-finite samples"
            );
        }
    }

    #[test]
    fn the_bands_never_overlap_however_the_crossovers_arrive() {
        // The high crossover is defined relative to the low one, so a sequence
        // that moves the low one upward must carry the high one with it.
        let rate = SampleRate::HZ_48000;
        let mut equaliser = ThreeBandEq::new();
        equaliser.prepare(&PrepareConfig::new(rate, 64, 1));

        for (low, high) in [
            (100.0, 8_000.0),
            (9_000.0, f64::NAN),
            (50.0, 60.0),
            (f64::NAN, 100.0),
        ] {
            equaliser.set_crossovers(low, high);
            assert!(
                equaliser.high_crossover_hz > equaliser.low_crossover_hz,
                "after ({low}, {high}) the bands overlap: {} against {}",
                equaliser.low_crossover_hz,
                equaliser.high_crossover_hz
            );
        }
    }
}
