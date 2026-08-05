//! Turning a rendered mix into the bytes a file holds.
//!
//! # Why the core does this when it does not write files
//!
//! ADR-0001 keeps input and output outside, and this is neither. Deciding how a
//! floating-point sample becomes sixteen bits is an audio decision: it involves
//! dither, it involves what happens to a value above full scale, and getting it
//! wrong is audible. A platform layer that made those choices would be making
//! mastering decisions in a file-writing routine.
//!
//! So the core produces the sample bytes and the host puts a header on them.
//! [`target::BitDepth::reduces_resolution`](crate::target::BitDepth::reduces_resolution)
//! already says whether dither is needed; this applies it.
//!
//! # Dither has to have state, and that is the whole subtlety
//!
//! Dither is noise added before rounding, and it works because the noise is
//! *different every sample*. A quantiser rebuilt for each block would restart
//! its noise at the same place every time — and a pattern that repeats every 512
//! samples at 48 kHz is not noise, it is a tone at ninety-four hertz, sitting
//! under the whole mix.
//!
//! That is why this is a struct that a host keeps across an export rather than a
//! function it calls per block.
//!
//! # Deterministic, which sounds like a contradiction and is not
//!
//! ADR-0006 requires a render to be reproducible: the same project must produce
//! the same file. So the noise is generated rather than sampled from anywhere —
//! a counter-based generator with a seed, which gives a sequence that is
//! statistically fine for dither and identical on every machine and every run.
//!
//! "Random" in dither means *decorrelated from the signal*, not *unpredictable*.
//! Those are different requirements and only the first one matters here.

use crate::target::BitDepth;

/// Turns rendered samples into the bytes a file holds.
///
/// Kept by the host for the whole of one export, so the dither noise carries on
/// rather than restarting. See the module documentation for why that matters.
#[derive(Debug, Clone)]
pub struct Quantiser {
    depth: BitDepth,
    dither: bool,
    state: u64,
    clipped: u64,
}

/// How many bytes one sample occupies at a depth.
#[must_use]
pub const fn bytes_per_sample(depth: BitDepth) -> usize {
    match depth {
        BitDepth::Sixteen => 2,
        BitDepth::TwentyFour => 3,
        BitDepth::Float32 => 4,
    }
}

/// The largest value a signed integer of this many bits can hold.
const fn full_scale(depth: BitDepth) -> f64 {
    match depth {
        BitDepth::Sixteen => 32_767.0,
        BitDepth::TwentyFour => 8_388_607.0,
        BitDepth::Float32 => 1.0,
    }
}

impl Quantiser {
    /// Builds one for a depth, dithering or not.
    ///
    /// `dither` should come from
    /// [`ExportReport::dither`](crate::report::ExportReport::dither) rather than
    /// from a preference: whether to dither is decided by what the depth does to
    /// the mix and where the file is going, and both of those are already
    /// answered.
    ///
    /// `seed` makes the noise reproducible. Any value works; the same value
    /// gives the same file.
    #[must_use]
    pub const fn new(depth: BitDepth, dither: bool, seed: u64) -> Self {
        Self {
            depth,
            dither: dither && depth.reduces_resolution(),
            // A zero seed would leave the generator at a fixed point, producing
            // no noise at all — dither that silently does nothing is worse than
            // none, because the report says it was applied.
            state: if seed == 0 {
                0x9E37_79B9_7F4A_7C15
            } else {
                seed
            },
            clipped: 0,
        }
    }

    /// The depth being written.
    #[must_use]
    pub const fn depth(&self) -> BitDepth {
        self.depth
    }

    /// Whether noise is actually being added.
    ///
    /// Not simply what was asked for: dither into a floating-point file would
    /// add noise to a format that was not going to lose anything, so it is
    /// refused at construction rather than obeyed.
    #[must_use]
    pub const fn is_dithering(&self) -> bool {
        self.dither
    }

    /// How many samples have been limited to full scale so far.
    ///
    /// Worth reporting rather than hiding. A mix that clips on the way out is
    /// something the user has to know about — `prv-export`'s whole argument is
    /// that nothing is applied silently, and clamping is something applied.
    #[must_use]
    pub const fn clipped(&self) -> u64 {
        self.clipped
    }

    /// How many bytes `frames * channels` samples will occupy.
    #[must_use]
    pub const fn byte_len(&self, samples: usize) -> usize {
        samples.saturating_mul(bytes_per_sample(self.depth))
    }

    /// Writes planar samples into `into`, interleaved and little-endian.
    ///
    /// `planar` is channel-major: `frames` samples of channel zero, then
    /// `frames` of channel one. The output is interleaved, because that is what
    /// every uncompressed container holds.
    ///
    /// Returns how many bytes were written.
    ///
    /// # Errors
    ///
    /// [`QuantiseError`] when the shapes do not agree or the output is too
    /// small — checked rather than trusted, because getting a channel count
    /// wrong here writes one channel's samples into another's file.
    pub fn write(
        &mut self,
        planar: &[f32],
        channels: usize,
        frames: usize,
        into: &mut [u8],
    ) -> Result<usize, QuantiseError> {
        let samples = channels
            .checked_mul(frames)
            .ok_or(QuantiseError::ShapeTooLarge)?;
        if planar.len() < samples {
            return Err(QuantiseError::NotEnoughSamples {
                needed: samples,
                given: planar.len(),
            });
        }
        let needed = self.byte_len(samples);
        if into.len() < needed {
            return Err(QuantiseError::BufferTooSmall {
                needed,
                given: into.len(),
            });
        }

        let width = bytes_per_sample(self.depth);
        let mut at: usize = 0;
        for frame in 0..frames {
            for channel in 0..channels {
                let index = channel
                    .checked_mul(frames)
                    .and_then(|base| base.checked_add(frame))
                    .ok_or(QuantiseError::ShapeTooLarge)?;
                let sample = planar.get(index).copied().unwrap_or(0.0);
                let end = at.checked_add(width).ok_or(QuantiseError::ShapeTooLarge)?;
                let slot = into
                    .get_mut(at..end)
                    .ok_or(QuantiseError::BufferTooSmall { needed, given: 0 })?;
                self.encode(sample, slot);
                at = end;
            }
        }
        Ok(needed)
    }

    /// Writes one sample.
    fn encode(&mut self, sample: f32, into: &mut [u8]) {
        // A value that is not a number cannot be written, and writing whatever
        // the bit pattern happens to be would put a click in a file. Silence is
        // the only defensible substitute.
        let value = if sample.is_finite() {
            f64::from(sample)
        } else {
            0.0
        };

        match self.depth {
            BitDepth::Float32 => {
                // Nothing is lost, so nothing is dithered and nothing is
                // clamped: the whole reason to choose this depth is that values
                // above full scale survive.
                #[allow(
                    clippy::cast_possible_truncation,
                    reason = "back to the width it arrived in"
                )]
                let written = (value as f32).to_le_bytes();
                if let Some(slot) = into.get_mut(..4) {
                    slot.copy_from_slice(&written);
                }
            }
            BitDepth::Sixteen | BitDepth::TwentyFour => {
                let scale = full_scale(self.depth);
                let mut scaled = value * scale;
                if self.dither {
                    scaled += self.noise();
                }

                let limit = scale;
                if scaled > limit || scaled < -limit - 1.0 {
                    self.clipped = self.clipped.saturating_add(1);
                }
                let clamped = scaled.clamp(-limit - 1.0, limit).round();

                #[allow(
                    clippy::cast_possible_truncation,
                    reason = "clamped to the depth's range on the line above"
                )]
                let integer = clamped as i32;
                let bytes = integer.to_le_bytes();
                let width = bytes_per_sample(self.depth);
                if let (Some(slot), Some(source)) = (into.get_mut(..width), bytes.get(..width)) {
                    slot.copy_from_slice(source);
                }
            }
        }
    }

    /// One dither sample: triangular, one bit peak to peak.
    ///
    /// Triangular rather than rectangular because the point of dither is to make
    /// the quantisation error independent of the signal, and only a triangular
    /// distribution achieves that — rectangular dither leaves the *noise
    /// modulation* audible on quiet passages, which is the artefact people
    /// actually hear.
    ///
    /// Two independent rectangular draws summed give a triangular distribution.
    fn noise(&mut self) -> f64 {
        let first = self.next_unit();
        let second = self.next_unit();
        first - second
    }

    /// A value in zero to one, from a counter-based generator.
    ///
    /// `splitmix64`: fast, no state beyond the counter, and good enough for
    /// dither by a wide margin. The sequence is identical on every machine,
    /// which is what makes an export reproducible.
    fn next_unit(&mut self) -> f64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;

        // The top 53 bits, which is what a double can hold exactly.
        #[allow(
            clippy::cast_precision_loss,
            reason = "53 bits is exactly what an f64 mantissa holds"
        )]
        let unit = (z >> 11) as f64 / (1_u64 << 53) as f64;
        unit
    }
}

/// Why a block could not be written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum QuantiseError {
    /// Fewer samples were given than the shape describes.
    NotEnoughSamples {
        /// How many the shape needs.
        needed: usize,
        /// How many arrived.
        given: usize,
    },
    /// The output buffer is too small.
    BufferTooSmall {
        /// How many bytes are needed.
        needed: usize,
        /// How many were offered.
        given: usize,
    },
    /// A channel count and frame count that multiply past what can be addressed.
    ShapeTooLarge,
}

impl core::fmt::Display for QuantiseError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NotEnoughSamples { needed, given } => {
                write!(f, "the block needs {needed} samples and {given} were given")
            }
            Self::BufferTooSmall { needed, given } => {
                write!(f, "the block needs {needed} bytes and {given} were offered")
            }
            Self::ShapeTooLarge => f.write_str("the block's shape is larger than can be addressed"),
        }
    }
}

impl core::error::Error for QuantiseError {}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::float_cmp,
        clippy::integer_division,
        clippy::cast_possible_truncation,
        reason = "a test that cannot build its own fixture should fail loudly, and \
                  a fixture that generates a tone is allowed the arithmetic to do it"
    )]

    use super::*;

    fn quantise(depth: BitDepth, dither: bool, planar: &[f32], channels: usize) -> Vec<u8> {
        let frames = planar.len() / channels;
        let mut writer = Quantiser::new(depth, dither, 1);
        let mut out = vec![0_u8; writer.byte_len(planar.len())];
        writer
            .write(planar, channels, frames, &mut out)
            .expect("writes");
        out
    }

    #[test]
    fn samples_come_out_interleaved_however_they_went_in() {
        // Planar in, interleaved out. Getting this wrong writes one channel's
        // samples into the other's file, which is the kind of defect that is
        // obvious in a listening test and invisible in a diff.
        let planar = [1.0_f32, 1.0, -1.0, -1.0]; // channel 0: +1 +1, channel 1: -1 -1
        let bytes = quantise(BitDepth::Sixteen, false, &planar, 2);

        assert_eq!(bytes.len(), 8);
        let read = |at: usize| i16::from_le_bytes([bytes[at], bytes[at + 1]]);
        assert_eq!(read(0), 32_767, "frame 0 channel 0");
        assert_eq!(read(2), -32_767, "frame 0 channel 1");
        assert_eq!(read(4), 32_767, "frame 1 channel 0");
        assert_eq!(read(6), -32_767, "frame 1 channel 1");
    }

    #[test]
    fn the_same_mix_always_produces_the_same_file() {
        // ADR-0006 requires a render to be reproducible. Dither is noise, and
        // noise that came from anywhere but a seed would make every export a
        // different file.
        let planar: Vec<f32> = (0..512)
            .map(|n| (f64::from(n) * 0.01).sin() as f32)
            .collect();

        let first = quantise(BitDepth::Sixteen, true, &planar, 2);
        let second = quantise(BitDepth::Sixteen, true, &planar, 2);
        assert_eq!(first, second);
    }

    #[test]
    fn dither_does_not_restart_between_blocks() {
        // The subtlety the whole type exists for. A quantiser rebuilt per block
        // repeats its noise every block — and a pattern repeating every 512
        // samples at 48 kHz is a tone at ninety-four hertz, not noise.
        let silence = vec![0.0_f32; 256];

        let mut carried = Quantiser::new(BitDepth::Sixteen, true, 7);
        let mut first = vec![0_u8; carried.byte_len(256)];
        let mut second = vec![0_u8; carried.byte_len(256)];
        carried.write(&silence, 1, 256, &mut first).expect("writes");
        carried
            .write(&silence, 1, 256, &mut second)
            .expect("writes");

        assert_ne!(
            first, second,
            "the noise repeated, which would be an audible tone"
        );
    }

    #[test]
    fn dither_is_refused_where_it_would_only_add_noise() {
        // Asking for dither into floating point is asking to damage a format
        // that was not going to lose anything.
        let float = Quantiser::new(BitDepth::Float32, true, 1);
        assert!(!float.is_dithering());

        let sixteen = Quantiser::new(BitDepth::Sixteen, true, 1);
        assert!(sixteen.is_dithering());
    }

    #[test]
    fn a_zero_seed_still_produces_noise() {
        // A generator left at a fixed point produces nothing, and dither that
        // silently does nothing is worse than none — the report says it was
        // applied.
        let silence = vec![0.0_f32; 64];
        let mut writer = Quantiser::new(BitDepth::Sixteen, true, 0);
        let mut out = vec![0_u8; writer.byte_len(64)];
        writer.write(&silence, 1, 64, &mut out).expect("writes");

        assert!(out.iter().any(|byte| *byte != 0), "the dither was silent");
    }

    #[test]
    fn floating_point_keeps_what_is_above_full_scale() {
        // The reason to choose this depth at all: a file going back into a
        // session must not have been clipped on the way out.
        let planar = [1.5_f32, -1.5];
        let bytes = quantise(BitDepth::Float32, false, &planar, 1);

        let read = |at: usize| {
            f32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
        };
        assert_eq!(read(0), 1.5);
        assert_eq!(read(4), -1.5);
    }

    #[test]
    fn clipping_is_counted_rather_than_hidden() {
        // `prv-export`'s whole argument is that nothing is applied silently, and
        // clamping is something applied.
        let planar = [2.0_f32, -2.0, 0.5];
        let mut writer = Quantiser::new(BitDepth::Sixteen, false, 1);
        let mut out = vec![0_u8; writer.byte_len(3)];
        writer.write(&planar, 1, 3, &mut out).expect("writes");

        assert_eq!(writer.clipped(), 2);
    }

    #[test]
    fn twenty_four_bits_occupy_three_bytes_and_keep_their_sign() {
        let planar = [-1.0_f32];
        let bytes = quantise(BitDepth::TwentyFour, false, &planar, 1);
        assert_eq!(bytes.len(), 3);

        // Little-endian, sign-extended by hand as a reader would.
        let value = i32::from_le_bytes([bytes[0], bytes[1], bytes[2], 0xFF]);
        assert_eq!(value, -8_388_607);
    }

    #[test]
    fn a_sample_that_is_not_a_number_becomes_silence_rather_than_a_click() {
        let planar = [f32::NAN, f32::INFINITY];
        let bytes = quantise(BitDepth::Sixteen, false, &planar, 1);
        assert_eq!(bytes, vec![0, 0, 0, 0]);
    }

    #[test]
    fn a_shape_that_does_not_fit_is_refused_rather_than_truncated() {
        let mut writer = Quantiser::new(BitDepth::Sixteen, false, 1);
        let planar = [0.0_f32; 4];

        assert!(matches!(
            writer.write(&planar, 2, 8, &mut [0; 64]),
            Err(QuantiseError::NotEnoughSamples { .. })
        ));
        assert!(matches!(
            writer.write(&planar, 2, 2, &mut [0; 2]),
            Err(QuantiseError::BufferTooSmall { .. })
        ));
    }

    #[test]
    fn dither_stays_within_a_bit_and_leaves_silence_recognisable() {
        // Dither is noise, and noise that was loud would be a defect of its own.
        // One bit peak to peak is what triangular dither costs.
        let silence = vec![0.0_f32; 4_096];
        let mut writer = Quantiser::new(BitDepth::Sixteen, true, 3);
        let mut out = vec![0_u8; writer.byte_len(silence.len())];
        writer
            .write(&silence, 1, silence.len(), &mut out)
            .expect("writes");

        let peak = out
            .chunks_exact(2)
            .map(|pair| i16::from_le_bytes([pair[0], pair[1]]).abs())
            .max()
            .expect("samples");
        assert!(
            peak <= 1,
            "the dither was {peak} bits peak, expected at most 1"
        );
        assert_eq!(writer.clipped(), 0);
    }
}
