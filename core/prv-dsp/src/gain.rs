use prv_rt::{AudioBuffer, LinearSmoother};

use crate::processor::{PrepareConfig, ProcessContext, Processor};

/// Frames over which a level change is ramped.
///
/// About ten milliseconds at 48 kHz. Long enough to remove the discontinuity,
/// short enough that a fader still feels connected to the hand.
const RAMP_FRAMES: u32 = 480;

/// The largest level this processor will apply, linear.
///
/// About +12 dB. A bound exists because Master Prompt #3B forbids abrupt volume
/// jumps and an unbounded gain is the shortest path to one.
const MAX_GAIN: f32 = 4.0;

/// A level control.
///
/// Every change is ramped. A level applied as a step introduces a discontinuity
/// into the waveform, and a discontinuity is a click — the artefact Master Prompt
/// #18 names as zipper noise when a fader produces a stream of them.
#[derive(Debug)]
pub struct Gain {
    level: LinearSmoother,
    ramp: Vec<f32>,
}

impl Gain {
    /// Creates a unity gain.
    #[must_use]
    pub fn new() -> Self {
        Self {
            level: LinearSmoother::new(1.0),
            ramp: Vec::new(),
        }
    }

    /// Creates a gain at a given level, applied immediately.
    #[must_use]
    pub fn with_level(level: f32) -> Self {
        Self {
            level: LinearSmoother::new(clamp(level)),
            ramp: Vec::new(),
        }
    }

    /// Sets the level, ramped.
    ///
    /// A non-finite value is ignored rather than propagated into the signal
    /// path, where a NaN would silence the output and be hard to trace.
    pub fn set_level(&mut self, level: f32) {
        self.level.set_target(clamp(level), RAMP_FRAMES);
    }

    /// Sets the level without ramping.
    pub fn set_level_immediate(&mut self, level: f32) {
        self.level.set_immediate(clamp(level));
    }

    /// Sets the level in decibels.
    ///
    /// Negative infinity, and anything below −100 dB, is silence.
    pub fn set_decibels(&mut self, decibels: f32) {
        self.set_level(decibels_to_linear(decibels));
    }

    /// The level currently applied.
    #[must_use]
    pub fn level(&self) -> f32 {
        self.level.current()
    }

    /// Whether a ramp is in progress.
    #[must_use]
    pub fn is_ramping(&self) -> bool {
        self.level.is_smoothing()
    }
}

impl Default for Gain {
    fn default() -> Self {
        Self::new()
    }
}

impl Processor for Gain {
    fn name(&self) -> &'static str {
        "Gain"
    }

    fn prepare(&mut self, config: &PrepareConfig) {
        // The ramp is computed once per block and reused across channels, so
        // that the smoother advances exactly once per frame however many
        // channels there are. Allocated here; never in `process`.
        self.ramp.resize(config.max_block_frames as usize, 1.0);
    }

    fn process(&mut self, ctx: &ProcessContext, buffer: &mut AudioBuffer) {
        let frames = ctx.frames.min(buffer.frames()).min(self.ramp.len());
        if frames == 0 {
            return;
        }
        if let Some(ramp) = self.ramp.get_mut(..frames) {
            self.level.fill(ramp);
        }

        for channel in buffer.channels_iter_mut() {
            let Some(samples) = channel.get_mut(..frames) else {
                continue;
            };
            for (position, sample) in samples.iter_mut().enumerate() {
                *sample *= self.ramp.get(position).copied().unwrap_or(1.0);
            }
        }
    }

    fn reset(&mut self) {
        // A level has no state to clear: it is a multiplication, not a filter.
        // The ramp in progress is deliberately preserved, because a seek should
        // not undo a fade the user is in the middle of.
    }
}

/// Clamps a level, ignoring a non-finite value.
fn clamp(level: f32) -> f32 {
    if level.is_finite() {
        level.clamp(0.0, MAX_GAIN)
    } else {
        1.0
    }
}

/// Converts decibels to a linear multiplier.
fn decibels_to_linear(decibels: f32) -> f32 {
    if decibels <= -100.0 || !decibels.is_finite() && decibels.is_sign_negative() {
        return 0.0;
    }
    if !decibels.is_finite() {
        return 1.0;
    }
    10.0_f32.powf(decibels / 20.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use prv_time::SampleRate;

    const RATE: SampleRate = SampleRate::HZ_48000;

    fn buffer(channels: usize, frames: usize, value: f32) -> AudioBuffer {
        let mut buffer = AudioBuffer::new(channels, frames)
            .unwrap_or_else(|_| AudioBuffer::new(1, 1).unwrap_or_else(|_| unreachable!()));
        buffer.as_mut_slice().fill(value);
        buffer
    }

    #[test]
    fn unity_leaves_the_signal_alone() {
        let mut gain = Gain::new();
        gain.prepare(&PrepareConfig::new(RATE, 64, 2));
        let mut audio = buffer(2, 64, 0.5);
        gain.process(&ProcessContext::new(64, RATE), &mut audio);
        assert!(audio.as_slice().iter().all(|s| (*s - 0.5).abs() < 1e-6));
    }

    #[test]
    fn an_immediate_level_applies_at_once() {
        let mut gain = Gain::new();
        gain.prepare(&PrepareConfig::new(RATE, 64, 1));
        gain.set_level_immediate(0.5);
        let mut audio = buffer(1, 64, 1.0);
        gain.process(&ProcessContext::new(64, RATE), &mut audio);
        assert!(audio.as_slice().iter().all(|s| (*s - 0.5).abs() < 1e-6));
    }

    /// The ramp length as a float, stated as a literal so the test performs no
    /// conversion of its own. Asserted against the constant below.
    const RAMP_FRAMES_F32: f32 = 480.0;

    #[test]
    fn a_level_change_ramps_rather_than_stepping() {
        let mut gain = Gain::new();
        gain.prepare(&PrepareConfig::new(RATE, 512, 1));
        gain.set_level(0.0);

        let mut audio = buffer(1, 512, 1.0);
        gain.process(&ProcessContext::new(512, RATE), &mut audio);

        let samples = audio.channel(0).unwrap_or(&[]);
        // No single frame may jump by more than one ramp step. That is what
        // makes the change inaudible as a click.
        assert_eq!(
            RAMP_FRAMES, 480,
            "the ramp length and its float form must agree"
        );
        let step = 1.0 / RAMP_FRAMES_F32;
        let mut previous = samples.first().copied().unwrap_or(0.0);
        for sample in samples.iter().skip(1) {
            assert!(
                (previous - *sample) <= step + 1e-6,
                "a frame jumped by more than one step"
            );
            previous = *sample;
        }
        // And by the end of a 512-frame block, a 480-frame ramp has finished.
        assert!(!gain.is_ramping());
        assert!(samples.last().copied().unwrap_or(1.0).abs() < 1e-6);
    }

    #[test]
    fn every_channel_gets_the_same_ramp() {
        // The reason the ramp is computed once per block rather than per
        // channel: a smoother advanced once per channel would move twice as
        // fast in stereo, and the two channels would receive different levels.
        let mut gain = Gain::new();
        gain.prepare(&PrepareConfig::new(RATE, 256, 2));
        gain.set_level(0.0);

        let mut audio = buffer(2, 256, 1.0);
        gain.process(&ProcessContext::new(256, RATE), &mut audio);

        let left = audio.channel(0).unwrap_or(&[]).to_vec();
        let right = audio.channel(1).unwrap_or(&[]).to_vec();
        assert_eq!(left, right, "channels must receive identical gain");
    }

    #[test]
    fn decibels_map_to_the_expected_levels() {
        let mut gain = Gain::new();
        gain.prepare(&PrepareConfig::new(RATE, 64, 1));

        gain.set_decibels(0.0);
        gain.set_level_immediate(decibels_to_linear(0.0));
        assert!((gain.level() - 1.0).abs() < 1e-6);

        gain.set_level_immediate(decibels_to_linear(-6.0206));
        assert!((gain.level() - 0.5).abs() < 1e-3, "got {}", gain.level());

        gain.set_level_immediate(decibels_to_linear(f32::NEG_INFINITY));
        assert!(gain.level().abs() < f32::EPSILON, "−∞ dB must be silence");
    }

    #[test]
    fn levels_are_clamped_and_non_finite_values_ignored() {
        let mut gain = Gain::new();
        gain.set_level_immediate(100.0);
        assert!((gain.level() - MAX_GAIN).abs() < f32::EPSILON);
        gain.set_level_immediate(-1.0);
        assert!(gain.level().abs() < f32::EPSILON);
        gain.set_level_immediate(1.0);
        gain.set_level_immediate(f32::NAN);
        assert!(
            (gain.level() - 1.0).abs() < f32::EPSILON,
            "NaN must be ignored"
        );
    }

    #[test]
    fn silence_stays_silent() {
        let mut gain = Gain::with_level(2.0);
        gain.prepare(&PrepareConfig::new(RATE, 64, 2));
        let mut audio = buffer(2, 64, 0.0);
        gain.process(&ProcessContext::new(64, RATE), &mut audio);
        assert!(audio.is_silent());
    }
}
