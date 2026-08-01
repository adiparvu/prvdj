//! Deterministic signal generators for this crate's tests.
//!
//! Every generator here is reproducible from a seed. That is not tidiness: an
//! analysis test that fails on one run in fifty because of a random signal is
//! worse than no test, because the team learns to re-run it. Master Prompt #27
//! requires the suite to be trustworthy enough that a red result stops a
//! release, and a flaky test destroys exactly that.

use prv_time::SampleRate;

use crate::num::{count_to_f64, narrow, ratio, round_to_count};

/// A xorshift generator, used because it is short enough to read and its
/// sequence is fixed for a given seed on every platform.
///
/// Not suitable for anything but test signals. It is not a cryptographic
/// generator and is never used outside `cfg(test)`.
pub(crate) struct Noise(u64);

impl Noise {
    pub(crate) const fn new(seed: u64) -> Self {
        // A zero state is a fixed point of xorshift, so it is replaced rather
        // than allowed to produce an all-zero "signal" that would make a test
        // pass for the wrong reason.
        Self(if seed == 0 {
            0x2545_F491_4F6C_DD1D
        } else {
            seed
        })
    }

    /// Returns the next value, in the range −1 to just under 1.
    pub(crate) fn next_bipolar(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        // The top 24 bits, which always fit a `u32` and therefore convert to
        // `f64` without any lossy cast.
        let bits = u32::try_from(self.0 >> 40).unwrap_or(0);
        f64::from(bits) / 8_388_608.0 - 1.0
    }
}

/// Converts a duration to a sample count.
pub(crate) fn samples(seconds: f64, rate: SampleRate) -> usize {
    round_to_count(seconds * f64::from(rate.hz()))
}

/// A sine tone at a fixed amplitude.
pub(crate) fn tone(frequency: f64, length: usize, amplitude: f64, rate: SampleRate) -> Vec<f32> {
    let step = core::f64::consts::TAU * frequency / f64::from(rate.hz());
    (0..length)
        .map(|n| narrow(amplitude * (step * count_to_f64(n)).sin()))
        .collect()
}

/// A percussive click: a short burst of noise with an exponential decay.
///
/// Broadband and decaying, because that is what a real drum hit looks like to a
/// spectral detector. A single-sample impulse would be an unrealistically easy
/// target and would let a detector that only responds to instantaneous
/// discontinuities pass.
pub(crate) fn write_click(into: &mut [f32], start: usize, seed: u64, rate: SampleRate) {
    let length = samples(0.01, rate);
    let mut noise = Noise::new(seed);
    for offset in 0..length {
        let decay = (-8.0 * ratio(offset, length)).exp();
        let value = noise.next_bipolar() * decay;
        if let Some(slot) = into.get_mut(start + offset) {
            *slot = narrow(value);
        }
    }
}

/// A click track at a fixed period.
pub(crate) fn click_track(period: usize, count: usize, rate: SampleRate) -> Vec<f32> {
    let mut buffer = vec![0.0_f32; period * count + period];
    for index in 0..count {
        write_click(&mut buffer, index * period, 0x51_ED + index as u64, rate);
    }
    buffer
}

/// The arithmetic mean of a slice, or zero for an empty one.
pub(crate) fn mean(values: &[f32]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.iter().map(|&v| f64::from(v)).sum::<f64>() / count_to_f64(values.len())
}
