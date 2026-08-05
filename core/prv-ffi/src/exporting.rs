//! Turning a rendered set into the bytes of a file.
//!
//! # Why this is a handle rather than a function
//!
//! Dither is noise added before rounding, and it works only because the noise
//! differs from sample to sample. A per-block function would restart its noise
//! at the same place every block — and a pattern repeating every 512 samples at
//! 48 kHz is a tone at ninety-four hertz sitting under the whole export.
//!
//! So the state crosses the boundary as a handle the host keeps for the length
//! of one export, exactly as [`prv_export::Quantiser`] is kept inside the core.
//!
//! # What the host still does
//!
//! Everything that touches a file. This produces the sample bytes, in the order
//! and the width a container holds them; the header, the path, the permissions
//! and the atomic replace are the platform's, because ADR-0001 says so and
//! because a WAVE header is forty-four bytes of arithmetic that no audio
//! decision depends on.

use prv_export::{bytes_per_sample, BitDepth, Quantiser};

use crate::delivery::depth_from_code;

use crate::status::Status;

/// One export in progress.
#[derive(Debug)]
pub struct Export {
    quantiser: Quantiser,
    channels: usize,
    written: u64,
}

impl Export {
    /// Begins an export at a depth.
    ///
    /// `dither` should come from an [`prv_export::ExportReport`] rather than
    /// from a preference — whether to dither is decided by what the depth does
    /// to the mix and where the file is going, and both are already answered.
    ///
    /// `seed` makes the noise reproducible: the same project and the same seed
    /// produce the same file, byte for byte, which is what ADR-0006 requires of
    /// a render.
    ///
    /// # Errors
    ///
    /// [`Status::InvalidArgument`] for a depth code this version does not
    /// define, or a channel count of zero.
    pub fn new(depth_code: i32, dither: bool, seed: u64, channels: u32) -> Result<Self, Status> {
        let depth = depth_from_code(depth_code).ok_or(Status::InvalidArgument)?;
        let channels = usize::try_from(channels).map_err(|_| Status::InvalidArgument)?;
        if channels == 0 {
            return Err(Status::InvalidArgument);
        }
        Ok(Self {
            quantiser: Quantiser::new(depth, dither, seed),
            channels,
            written: 0,
        })
    }

    /// How many bytes a block of `frames` will produce.
    ///
    /// A host asks before it allocates, and asks once: the answer does not
    /// change during an export.
    #[must_use]
    pub fn block_bytes(&self, frames: u32) -> u64 {
        let channels = u64::try_from(self.channels).unwrap_or(0);
        let width = u64::try_from(bytes_per_sample(self.quantiser.depth())).unwrap_or(0);
        u64::from(frames)
            .saturating_mul(channels)
            .saturating_mul(width)
    }

    /// Converts one rendered block into file bytes.
    ///
    /// # Errors
    ///
    /// [`Status::BufferTooSmall`] when `into` cannot hold the block, and
    /// [`Status::InvalidArgument`] when the shapes do not agree.
    pub fn block(
        &mut self,
        planar: &[f32],
        frames: usize,
        into: &mut [u8],
    ) -> Result<usize, Status> {
        let bytes = self
            .quantiser
            .write(planar, self.channels, frames, into)
            .map_err(|error| match error {
                prv_export::QuantiseError::BufferTooSmall { .. } => Status::BufferTooSmall,
                _ => Status::InvalidArgument,
            })?;
        self.written = self
            .written
            .saturating_add(u64::try_from(bytes).unwrap_or(0));
        Ok(bytes)
    }

    /// How many bytes of audio have been produced.
    #[must_use]
    pub const fn written(&self) -> u64 {
        self.written
    }

    /// How many samples were limited to full scale.
    ///
    /// Non-zero means the mix clipped on the way out, and the user has to be
    /// told: `prv-export`'s whole argument is that nothing is applied silently,
    /// and clamping is something applied.
    #[must_use]
    pub const fn clipped(&self) -> u64 {
        self.quantiser.clipped()
    }

    /// Whether noise is actually being added.
    ///
    /// Not merely what was asked for. Dither into a floating-point file would
    /// damage a format that was not going to lose anything, so it is refused
    /// rather than obeyed — and a host that reports "dithered" on its export
    /// screen should report what happened rather than what it requested.
    #[must_use]
    pub const fn is_dithering(&self) -> bool {
        self.quantiser.is_dithering()
    }

    /// How many bytes one sample occupies at this depth.
    #[must_use]
    pub fn sample_width(&self) -> u32 {
        u32::try_from(bytes_per_sample(self.quantiser.depth())).unwrap_or(4)
    }

    /// Whether the samples are floating point rather than integers.
    ///
    /// A WAVE header needs this: the format tag differs, and a file that claims
    /// integers and holds floats is noise at full scale.
    #[must_use]
    pub const fn is_float(&self) -> bool {
        matches!(self.quantiser.depth(), BitDepth::Float32)
    }
}

// The depth codes are `crate::delivery`'s, deliberately reused rather than
// minted again. `PrvBitDepth` is already in the header with `PRV_DEPTH_SIXTEEN`
// at zero, and a second enum for the same concept would collide in C's one
// namespace — while renumbering the first would change the meaning of a call
// that already exists, which the ABI's major version forbids.
//
// A tempting improvement was refused for the same reason: numbering from one so
// that zero is never a depth is a better design, and it is not available here.

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::cast_possible_truncation,
        reason = "a test that cannot build its own fixture should fail loudly, and \
                  a fixture that generates a tone is allowed the arithmetic to do it"
    )]

    use super::*;

    const DEPTHS: [BitDepth; 3] = [BitDepth::Sixteen, BitDepth::TwentyFour, BitDepth::Float32];

    /// The code the header already gives a depth.
    fn code_of(depth: BitDepth) -> i32 {
        (0..3)
            .find(|code| depth_from_code(*code) == Some(depth))
            .expect("every depth has a code")
    }

    #[test]
    fn an_export_can_be_begun_at_every_depth_the_header_defines() {
        // The codes are `delivery`'s, reused rather than minted again — a second
        // `PrvBitDepth` would collide in C's one namespace, and renumbering the
        // first would change a call that already exists.
        for depth in DEPTHS {
            let export = Export::new(code_of(depth), false, 1, 2).expect("begins");
            assert_eq!(
                u64::from(export.sample_width()),
                u64::try_from(bytes_per_sample(depth)).expect("small")
            );
        }
    }

    #[test]
    fn a_depth_this_version_does_not_define_is_refused() {
        assert_eq!(
            Export::new(-1, false, 1, 2).err(),
            Some(Status::InvalidArgument)
        );
        assert_eq!(
            Export::new(99, false, 1, 2).err(),
            Some(Status::InvalidArgument)
        );
        assert_eq!(
            Export::new(code_of(BitDepth::Sixteen), false, 1, 0).err(),
            Some(Status::InvalidArgument)
        );
    }

    #[test]
    fn a_block_produces_exactly_what_it_promised_to() {
        let mut export = Export::new(code_of(BitDepth::Sixteen), false, 1, 2).expect("begins");
        let planar = vec![0.0_f32; 512];
        let expected = export.block_bytes(256);

        let mut into = vec![0_u8; usize::try_from(expected).expect("small")];
        assert_eq!(
            export.block(&planar, 256, &mut into).expect("writes") as u64,
            expected
        );
        assert_eq!(export.written(), expected);
    }

    #[test]
    fn a_buffer_that_is_too_small_is_refused_rather_than_filled_partly() {
        let mut export = Export::new(code_of(BitDepth::Sixteen), false, 1, 2).expect("begins");
        let planar = vec![0.0_f32; 512];
        assert_eq!(
            export.block(&planar, 256, &mut [0; 16]).err(),
            Some(Status::BufferTooSmall)
        );
        assert_eq!(export.written(), 0, "a refused block counted as written");
    }

    #[test]
    fn the_noise_carries_across_blocks() {
        // The reason this is a handle. A per-block function restarts its noise
        // every block, which is a tone rather than dither.
        let mut export = Export::new(code_of(BitDepth::Sixteen), true, 5, 1).expect("begins");
        let silence = vec![0.0_f32; 256];

        let mut first = vec![0_u8; 512];
        let mut second = vec![0_u8; 512];
        export.block(&silence, 256, &mut first).expect("writes");
        export.block(&silence, 256, &mut second).expect("writes");

        assert!(export.is_dithering());
        assert_ne!(first, second, "the noise repeated between blocks");
    }

    #[test]
    fn the_same_seed_gives_the_same_file() {
        let render = |seed: u64| {
            let mut export =
                Export::new(code_of(BitDepth::Sixteen), true, seed, 1).expect("begins");
            let planar: Vec<f32> = (0..256)
                .map(|n| (f64::from(n) * 0.05).sin() as f32)
                .collect();
            let mut into = vec![0_u8; 512];
            export.block(&planar, 256, &mut into).expect("writes");
            into
        };
        assert_eq!(render(11), render(11));
        assert_ne!(render(11), render(12));
    }

    #[test]
    fn a_float_export_says_so_and_does_not_dither() {
        let export = Export::new(code_of(BitDepth::Float32), true, 1, 2).expect("begins");
        assert!(export.is_float());
        assert!(!export.is_dithering(), "a float file was dithered");
        assert_eq!(export.sample_width(), 4);
    }

    #[test]
    fn clipping_is_counted_and_reported() {
        let mut export = Export::new(code_of(BitDepth::Sixteen), false, 1, 1).expect("begins");
        let planar = [2.0_f32, -2.0, 0.0];
        let mut into = vec![0_u8; 6];
        export.block(&planar, 3, &mut into).expect("writes");
        assert_eq!(export.clipped(), 2);
    }
}
