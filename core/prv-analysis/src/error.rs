use core::fmt;

/// Errors produced when configuring or running an analysis stage.
///
/// Master Prompt #10 requires structured errors that can be explained to the
/// user rather than bare failures. Every variant here names the value that
/// caused it, because the caller is usually a pipeline that must report *which*
/// track failed and why, not merely that something did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum AnalysisError {
    /// A transform size that is not a power of two.
    ///
    /// The transform is radix-2. A general-size transform would be slower for
    /// every real case in order to serve none of them, since window sizes are
    /// chosen by this crate rather than by a user.
    SizeNotPowerOfTwo {
        /// The rejected size.
        size: usize,
    },

    /// A transform or window size below the minimum this crate supports.
    SizeTooSmall {
        /// The rejected size.
        size: usize,
        /// The smallest accepted size.
        minimum: usize,
    },

    /// A buffer whose length does not match the transform it was passed to.
    BufferLength {
        /// The length the transform requires.
        expected: usize,
        /// The length supplied.
        actual: usize,
    },

    /// A hop size of zero, or one larger than the analysis window.
    ///
    /// A hop larger than the window would skip audio entirely, which is a
    /// configuration mistake rather than a coarse setting, so it is rejected
    /// instead of clamped.
    HopOutOfRange {
        /// The rejected hop, in samples.
        hop: usize,
        /// The window size it was compared against.
        window: usize,
    },

    /// Not enough audio to run a stage that needs a minimum duration.
    ///
    /// Tempo estimation over two seconds of audio is not a low-confidence
    /// answer; it is not an answer. Reporting this rather than returning a
    /// number with low confidence keeps the confidence scale meaningful.
    NotEnoughAudio {
        /// The number of sample frames supplied.
        frames: usize,
        /// The minimum this stage requires.
        minimum: usize,
    },

    /// No usable periodicity was found in the audio.
    ///
    /// Applause, spoken word, ambient recordings and silence all reach this.
    /// The distinction from a low-confidence tempo matters: a low confidence
    /// says "probably 128, possibly not"; this says "there is no beat here".
    NoPeriodicity,
}

impl fmt::Display for AnalysisError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SizeNotPowerOfTwo { size } => {
                write!(f, "transform size {size} is not a power of two")
            }
            Self::SizeTooSmall { size, minimum } => {
                write!(f, "size {size} is below the minimum of {minimum}")
            }
            Self::BufferLength { expected, actual } => {
                write!(
                    f,
                    "buffer length {actual} does not match the required {expected}"
                )
            }
            Self::HopOutOfRange { hop, window } => {
                write!(f, "hop {hop} is not within 1 to {window}")
            }
            Self::NotEnoughAudio { frames, minimum } => {
                write!(
                    f,
                    "{frames} sample frames is below the minimum of {minimum}"
                )
            }
            Self::NoPeriodicity => f.write_str("no usable periodicity found in the audio"),
        }
    }
}

impl core::error::Error for AnalysisError {}
