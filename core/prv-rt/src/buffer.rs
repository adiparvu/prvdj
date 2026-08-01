//! Preallocated planar audio buffers.
//!
//! # Planar rather than interleaved
//!
//! Channels are stored contiguously — all of the left channel, then all of the
//! right — rather than interleaved sample by sample. Every processor in the
//! graph operates on one channel at a time, so planar layout keeps each inner
//! loop walking memory sequentially, which is what the prefetcher and the vector
//! units want. Interleaving is a presentation concern, converted at the device
//! boundary and nowhere else.
//!
//! # Allocation happens once
//!
//! The backing memory is allocated when the buffer is constructed, on a
//! non-realtime thread, before the buffer can be reached from the audio
//! callback. No method here allocates. ADR-0002 makes that a contract, not a
//! preference.

use core::fmt;

/// The largest buffer the engine will allocate, in samples.
///
/// 64 mega-samples is roughly 128 channels of eight seconds at 48 kHz — far
/// beyond any legitimate block size. The bound exists so that a corrupted or
/// hostile channel count cannot ask for an unbounded allocation.
const MAX_TOTAL_SAMPLES: usize = 64 * 1024 * 1024;

/// Errors from buffer construction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum BufferError {
    /// A channel count of zero.
    ZeroChannels,
    /// A frame count of zero.
    ZeroFrames,
    /// The requested size exceeds the engine's allocation bound.
    TooLarge {
        /// Channels requested.
        channels: usize,
        /// Frames requested.
        frames: usize,
    },
}

impl fmt::Display for BufferError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroChannels => f.write_str("a buffer must have at least one channel"),
            Self::ZeroFrames => f.write_str("a buffer must have at least one frame"),
            Self::TooLarge { channels, frames } => write!(
                f,
                "buffer of {channels} channels by {frames} frames exceeds the allocation bound"
            ),
        }
    }
}

impl core::error::Error for BufferError {}

/// A planar, fixed-size audio buffer.
///
/// Channel `c` occupies the contiguous range `c * frames .. (c + 1) * frames`.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioBuffer {
    samples: Box<[f32]>,
    channels: usize,
    frames: usize,
}

impl AudioBuffer {
    /// Allocates a silent buffer.
    ///
    /// This is the only operation in this type that allocates, and it must be
    /// called before the buffer becomes reachable from the audio thread.
    ///
    /// # Errors
    ///
    /// Returns [`BufferError`] if either dimension is zero or the total exceeds
    /// the engine's allocation bound.
    pub fn new(channels: usize, frames: usize) -> Result<Self, BufferError> {
        if channels == 0 {
            return Err(BufferError::ZeroChannels);
        }
        if frames == 0 {
            return Err(BufferError::ZeroFrames);
        }
        let total = channels
            .checked_mul(frames)
            .filter(|total| *total <= MAX_TOTAL_SAMPLES)
            .ok_or(BufferError::TooLarge { channels, frames })?;

        Ok(Self {
            samples: vec![0.0; total].into_boxed_slice(),
            channels,
            frames,
        })
    }

    /// Number of channels.
    #[must_use]
    pub const fn channels(&self) -> usize {
        self.channels
    }

    /// Number of frames per channel.
    #[must_use]
    pub const fn frames(&self) -> usize {
        self.frames
    }

    /// Returns one channel for reading, or `None` if `channel` is out of range.
    #[must_use]
    pub fn channel(&self, channel: usize) -> Option<&[f32]> {
        let start = channel.checked_mul(self.frames)?;
        let end = start.checked_add(self.frames)?;
        self.samples.get(start..end)
    }

    /// Returns one channel for writing, or `None` if `channel` is out of range.
    #[must_use]
    pub fn channel_mut(&mut self, channel: usize) -> Option<&mut [f32]> {
        let start = channel.checked_mul(self.frames)?;
        let end = start.checked_add(self.frames)?;
        self.samples.get_mut(start..end)
    }

    /// Iterates over every channel for reading.
    pub fn channels_iter(&self) -> impl Iterator<Item = &[f32]> {
        self.samples.chunks_exact(self.frames)
    }

    /// Iterates over every channel for writing.
    pub fn channels_iter_mut(&mut self) -> impl Iterator<Item = &mut [f32]> {
        let frames = self.frames;
        self.samples.chunks_exact_mut(frames)
    }

    /// Fills the buffer with silence.
    ///
    /// Called at the start of every block. Master Prompt #15 requires that
    /// "silence should remain silent": a processor that writes nothing must
    /// leave silence behind, not the previous block's contents, which would be
    /// heard as a repeating fragment.
    pub fn clear(&mut self) {
        self.samples.fill(0.0);
    }

    /// Returns the whole backing store for reading.
    #[must_use]
    pub fn as_slice(&self) -> &[f32] {
        &self.samples
    }

    /// Returns the whole backing store for writing.
    #[must_use]
    pub fn as_mut_slice(&mut self) -> &mut [f32] {
        &mut self.samples
    }

    /// Returns `true` if every sample is exactly zero.
    ///
    /// Used by diagnostics and by tests that assert a processor left silence
    /// untouched.
    #[must_use]
    pub fn is_silent(&self) -> bool {
        self.samples.iter().all(|sample| *sample == 0.0)
    }

    /// Returns the largest absolute sample value in the buffer.
    ///
    /// The basis of peak metering. Returns zero for a silent buffer.
    #[must_use]
    pub fn peak(&self) -> f32 {
        self.samples
            .iter()
            .fold(0.0_f32, |peak, sample| peak.max(sample.abs()))
    }
}

#[cfg(test)]
mod tests {
    // Exact float comparison is the point of these assertions, not an
    // oversight: silence must be exactly zero, and a peak value that is
    // approximately 0.9 would mean the buffer altered the sample it was given.
    #![allow(
        clippy::float_cmp,
        reason = "these assertions verify exact sample values by design"
    )]

    use super::*;

    #[test]
    fn a_new_buffer_is_silent_and_correctly_shaped() {
        let buffer = AudioBuffer::new(2, 512);
        assert!(buffer.is_ok());
        if let Ok(buffer) = buffer {
            assert_eq!(buffer.channels(), 2);
            assert_eq!(buffer.frames(), 512);
            assert_eq!(buffer.as_slice().len(), 1_024);
            assert!(buffer.is_silent());
            assert_eq!(buffer.peak(), 0.0);
        }
    }

    #[test]
    fn rejects_degenerate_and_oversized_shapes() {
        assert_eq!(AudioBuffer::new(0, 512), Err(BufferError::ZeroChannels));
        assert_eq!(AudioBuffer::new(2, 0), Err(BufferError::ZeroFrames));
        assert!(matches!(
            AudioBuffer::new(1_000, MAX_TOTAL_SAMPLES),
            Err(BufferError::TooLarge { .. })
        ));
        assert!(matches!(
            AudioBuffer::new(usize::MAX, 2),
            Err(BufferError::TooLarge { .. })
        ));
    }

    #[test]
    fn channels_are_independent_regions() {
        let buffer = AudioBuffer::new(2, 4);
        assert!(buffer.is_ok());
        let Ok(mut buffer) = buffer else { return };

        if let Some(left) = buffer.channel_mut(0) {
            left.fill(1.0);
        }
        assert_eq!(buffer.channel(0), Some([1.0_f32; 4].as_slice()));
        assert_eq!(buffer.channel(1), Some([0.0_f32; 4].as_slice()));
    }

    #[test]
    fn out_of_range_channels_return_none_rather_than_panicking() {
        let buffer = AudioBuffer::new(2, 4);
        assert!(buffer.is_ok());
        let Ok(mut buffer) = buffer else { return };

        assert_eq!(buffer.channel(2), None);
        assert_eq!(buffer.channel_mut(2), None);
        assert_eq!(buffer.channel(usize::MAX), None);
        assert_eq!(buffer.channel_mut(usize::MAX), None);
    }

    #[test]
    fn iteration_covers_every_channel_exactly_once() {
        let buffer = AudioBuffer::new(4, 8);
        assert!(buffer.is_ok());
        let Ok(mut buffer) = buffer else { return };

        let mut index = 0.0_f32;
        for channel in buffer.channels_iter_mut() {
            index += 1.0;
            channel.fill(index);
        }
        assert_eq!(index, 4.0);

        let sums: Vec<f32> = buffer
            .channels_iter()
            .map(|channel| channel.iter().sum())
            .collect();
        assert_eq!(sums, vec![8.0, 16.0, 24.0, 32.0]);
    }

    #[test]
    fn clearing_restores_silence() {
        let buffer = AudioBuffer::new(2, 16);
        assert!(buffer.is_ok());
        let Ok(mut buffer) = buffer else { return };

        buffer.as_mut_slice().fill(0.7);
        assert!(!buffer.is_silent());
        buffer.clear();
        assert!(buffer.is_silent());
    }

    #[test]
    fn peak_reports_the_largest_magnitude_regardless_of_sign() {
        let buffer = AudioBuffer::new(1, 4);
        assert!(buffer.is_ok());
        let Ok(mut buffer) = buffer else { return };

        if let Some(channel) = buffer.channel_mut(0) {
            if let Some(first) = channel.first_mut() {
                *first = 0.25;
            }
            if let Some(second) = channel.get_mut(1) {
                *second = -0.9;
            }
        }
        assert_eq!(buffer.peak(), 0.9);
    }
}
