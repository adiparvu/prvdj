use core::fmt;

use prv_rt::AudioBuffer;
use prv_time::SampleRate;

/// Everything a processor needs to size itself, supplied before it can be
/// reached from the audio thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrepareConfig {
    /// The rate audio will arrive at.
    pub sample_rate: SampleRate,
    /// The largest block the host will ever ask for.
    ///
    /// A processor that needs scratch space allocates for this once, here, and
    /// never again.
    pub max_block_frames: u32,
    /// Number of channels.
    pub channels: usize,
}

impl PrepareConfig {
    /// Creates a configuration.
    #[must_use]
    pub const fn new(sample_rate: SampleRate, max_block_frames: u32, channels: usize) -> Self {
        Self {
            sample_rate,
            max_block_frames,
            channels,
        }
    }
}

/// What a processor is told about the block it is about to render.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessContext {
    /// Frames in this block. Never larger than `max_block_frames`.
    pub frames: usize,
    /// The rate in force.
    pub sample_rate: SampleRate,
}

impl ProcessContext {
    /// Creates a context.
    #[must_use]
    pub const fn new(frames: usize, sample_rate: SampleRate) -> Self {
        Self {
            frames,
            sample_rate,
        }
    }
}

/// An element of the signal path.
///
/// # The contract
///
/// [`Self::prepare`] may do anything, including allocate. It runs off the audio
/// thread, before the processor is reachable from the callback.
///
/// [`Self::process`] runs on the audio thread and must not allocate, lock, block,
/// make a system call or panic, and its work must be bounded by the frame count
/// alone. ADR-0002 states the contract; the allocation gate in continuous
/// integration enforces it.
///
/// # Latency
///
/// A processor that delays its output reports how much through
/// [`Self::latency_frames`], so the graph can compensate. A processor that
/// reports the wrong latency is worse than one that reports none, because the
/// compensation will then actively misalign it.
pub trait Processor: Send + fmt::Debug {
    /// A short, stable name, for diagnostics and for the effect rack.
    fn name(&self) -> &'static str;

    /// Prepares for playback at a given rate and block size.
    ///
    /// May allocate. Always called before the first [`Self::process`], and again
    /// whenever the rate or block size changes.
    fn prepare(&mut self, config: &PrepareConfig);

    /// Processes one block in place.
    ///
    /// Must obey the audio-thread contract.
    fn process(&mut self, ctx: &ProcessContext, buffer: &mut AudioBuffer);

    /// Clears internal state without changing parameters.
    ///
    /// Called on a seek or a track change, so that the tail of the previous
    /// audio does not bleed into the new material — the artefact Master Prompt
    /// #15 means by "silence should remain silent".
    fn reset(&mut self);

    /// Frames of delay this processor introduces. Zero by default.
    fn latency_frames(&self) -> u32 {
        0
    }
}
