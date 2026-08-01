//! Musical time, tempo, and the authoritative transport clock.
//!
//! # Why this crate exists
//!
//! Module Specification #002 states that the system has exactly one authoritative
//! clock and that no module creates its own. Master Prompt #18 requires every
//! transport operation to be sample accurate. Everything downstream — the
//! timeline, waveform rendering, automation, recording, and later lighting and
//! video — reads musical position from here. If two subsystems computed position
//! independently they would eventually disagree, and a disagreement of a single
//! sample is a visible misalignment between the playhead and the waveform.
//!
//! # The invariant that makes drift impossible
//!
//! Position is stored as an exact integer count of sample frames and is never
//! derived from wall-clock time. Musical position is *computed* from that integer
//! on demand using exact integer arithmetic, never accumulated incrementally in
//! floating point.
//!
//! This distinction is the whole design. Accumulating `position += frames / frames_per_beat`
//! in `f64` drifts, because each addition rounds. Computing `beats = f(position)`
//! from an exact integer does not, because there is nothing to accumulate. Over a
//! six-hour set the difference between these two approaches is the difference
//! between a beat grid that still lines up and one that does not.
//!
//! Tempo is stored as microseconds per beat rather than beats per minute for the
//! same reason: it is exact, and it makes every conversion a rational operation
//! on integers. Beats per minute is offered as a lossy convenience at the edges.
//!
//! # Determinism across platforms
//!
//! All conversions use integer arithmetic with 128-bit intermediates. The same
//! inputs produce bit-identical results on every platform, which is what
//! ADR-0006 requires of the planner and what makes preview and export agree.
//!
//! # Realtime safety
//!
//! Every type here is `Copy`, allocation-free and panic-free. The transport clock
//! is advanced from the audio callback, so it must satisfy the contract in
//! ADR-0002.
//!
//! # Example
//!
//! ```
//! use prv_time::{Frames, SampleRate, Tempo, TimeSignature, TransportClock};
//!
//! let mut clock = TransportClock::new(
//!     SampleRate::HZ_48000,
//!     Tempo::from_bpm(128.0).unwrap(),
//!     TimeSignature::FOUR_FOUR,
//! );
//!
//! // One beat at 128 BPM and 48 kHz is 22 500 frames.
//! clock.advance(22_500);
//!
//! let position = clock.musical_position();
//! assert_eq!(position.bar, 0);
//! assert_eq!(position.beat, 1);
//! assert_eq!(position.tick, 0);
//! assert_eq!(clock.position(), Frames::new(22_500));
//! ```

mod clock;
mod error;
mod frames;
mod musical_time;
mod sample_rate;
mod signature;
mod tempo;

pub use clock::{TransportClock, TransportSnapshot};
pub use error::TimeError;
pub use frames::Frames;
pub use musical_time::{MusicalTime, Ticks, TICKS_PER_BEAT};
pub use sample_rate::SampleRate;
pub use signature::TimeSignature;
pub use tempo::Tempo;
