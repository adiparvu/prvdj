//! Waveform tiles.
//!
//! # What a waveform is, computationally
//!
//! A four-minute track at 48 kHz is eleven million samples per channel. A
//! timeline is at most a few thousand pixels wide. Drawing a waveform means
//! answering, for each pixel, "what was the loudest and quietest sample in this
//! span?" — and answering it again, differently, every time the user zooms.
//!
//! Scanning the audio per frame is impossible: at 120 frames a second there is
//! no time to read eleven million samples. So the answer is precomputed into
//! *tiles*, each summarising a fixed span, at several resolutions. Rendering
//! then reads a few thousand tiles instead of eleven million samples.
//!
//! # Why several resolutions rather than one
//!
//! One resolution cannot serve both ends of the zoom range. Fine enough for a
//! close edit and it is millions of tiles when zoomed out; coarse enough for the
//! whole track and a close edit shows a rectangle. Module Specification #003
//! requires five resolution bands and forbids upscaling coarse data, because an
//! upscaled peak is a claim about a transient that was never measured — and a
//! DJ cutting on a transient that is not there cuts in the wrong place.
//!
//! # What this crate guarantees
//!
//! - **Tiles never lie.** A tile's minimum and maximum are the true extremes of
//!   its span, and coarse levels are built from the audio rather than from finer
//!   tiles, so no error compounds down the ladder.
//! - **Generation is resumable.** Audio is fed in arbitrary chunks; stopping and
//!   continuing produces the same result as one pass. An import interrupted at
//!   90 % resumes rather than restarts (Master Prompt #7).
//! - **Invalidation is precise.** Every tile carries the version of the
//!   algorithm that produced it, so improving analysis re-renders exactly what
//!   that change affects and nothing more (Master Prompt #20).
//! - **Rendering allocates nothing.** Peaks are written into a caller-supplied
//!   buffer, so a scrolling timeline does not allocate per frame.

mod tile;
mod viewport;
mod waveform;

pub use tile::{GenerationVersion, Tile};
pub use viewport::{Peak, Viewport};
pub use waveform::{Waveform, WaveformBuilder, WaveformError, WaveformLevel, RESOLUTION_LADDER};
