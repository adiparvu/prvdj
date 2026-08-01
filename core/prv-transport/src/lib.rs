//! Playback state, loop regions, and the transport that drives the clock.
//!
//! # The rule this crate exists to honour
//!
//! Module Specification #002 is unusually direct about one thing: *state
//! transitions must be explicit. No hidden transitions.*
//!
//! That rule is easy to agree with and easy to break. Transport state normally
//! decays into a scattering of booleans — `is_playing`, `is_seeking`,
//! `is_buffering`, `has_error` — which can express states that make no sense
//! (playing while stopped) and which drift out of agreement as features are
//! added. By the second year nobody can say what the transport will do when a
//! device disappears during a seek.
//!
//! Here the state is a single value, and every transition is a total function
//! from a state and an event to either a new state or an explicit rejection.
//! Illegal combinations are not merely discouraged; they are unrepresentable,
//! and a rejected transition is a value the caller can inspect rather than a
//! silent no-op.
//!
//! # Intent is explicit, not hidden
//!
//! After a seek or a buffer underrun the transport must return to whatever the
//! user asked for. The usual way to remember that is a private boolean, which is
//! exactly the hidden state the specification forbids.
//!
//! The transition table itself is exported as [`next_state`], a pure function.
//! An interface can ask what *would* happen — should this button be enabled? —
//! without mutating anything, and the table can be tested by exhaustion rather
//! than by example.
//!
//! Instead the transport carries [`PlaybackIntent`] — what the user asked for —
//! alongside [`PlaybackState`] — what the engine is doing. Both are public and
//! inspectable. A performer whose track is buffering can see that the transport
//! still intends to play, and so can a diagnostic.
//!
//! # Realtime safety
//!
//! [`Transport`] is `Copy`, allocation-free and panic-free. It is advanced from
//! the audio callback, so it obeys the contract in ADR-0002.

mod looping;
mod state;
mod transport;

pub use looping::LoopRegion;
pub use state::{next_state, InvalidTransition, PlaybackIntent, PlaybackState, TransportEvent};
pub use transport::{Transport, TransportError};
