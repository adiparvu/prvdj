//! The mix timeline: clips, automation, and a way to name a parameter.
//!
//! # What this crate is for
//!
//! Master Prompt #21 describes the surface a user edits a mix on: tracks laid
//! out in time, automation beside them, everything snapping to a musical grid.
//! This crate is the domain model behind that surface.
//!
//! It also closes a debt. `prv-dsp` has had processors with parameters since
//! Sprint 1 and no way to *refer* to one, recorded as a deliberate omission
//! because three features need the same answer — automation, plugin parameters
//! and the operation log — and solving it for any one of them alone would have
//! produced something the other two could not use. [`ParameterAddress`] is that
//! answer.
//!
//! # It is a shape, not a second source of truth
//!
//! ADR-0003 makes the project an append-only operation log and the state a pure
//! fold over it. Nothing here persists anything or holds history: undo,
//! versions and branching are properties of the log, and two mechanisms for
//! going back in time is how a project ends up able to reach a state neither of
//! them believes in.
//!
//! # Two contexts, one crate
//!
//! Editing happens off the audio thread and may do whatever it likes.
//! [`AutomationLane::value_at`] happens *on* it, once per control block per
//! automated parameter, and obeys ADR-0002: no allocation, no locking, and work
//! logarithmic in the number of points rather than linear. The split is why the
//! lane is an owned sorted structure built off-thread and a binary search
//! on-thread.
//!
//! # Layout
//!
//! | Module | Question it answers |
//! |--------|--------------------|
//! | [`parameter`] | What values does a parameter take, and how does a control map onto it? |
//! | [`automation`] | What value does that parameter have, at this moment? |
//! | [`timeline`] | What plays, where, and what does an edit do? |
//!
//! # Example
//!
//! ```
//! use prv_project::PlacementId;
//! use prv_time::{BeatGrid, Frames, SampleRate, SnapResolution, Tempo, TimeSignature};
//! use prv_timeline::{
//!     AutomationLane, AutomationPoint, Clip, Interpolation, ParameterAddress, ParameterKey,
//!     ParameterOwner, Snap, Timeline,
//! };
//!
//! let grid = BeatGrid::new(
//!     SampleRate::HZ_44100,
//!     Tempo::from_bpm(120.0).unwrap(),
//!     TimeSignature::FOUR_FOUR,
//!     Frames::ZERO,
//! );
//! let bar = Frames::new(88_200);
//!
//! let mut timeline = Timeline::new();
//! // A slightly late drop snaps onto the bar line.
//! let placed = timeline
//!     .add(
//!         Clip::new(PlacementId::new(1), 0, Frames::new(bar.get() + 300), bar),
//!         &grid,
//!         Snap::To(SnapResolution::Bar),
//!     )
//!     .unwrap();
//! assert_eq!(placed.start(), bar);
//!
//! // A filter sweep across that bar.
//! let address =
//!     ParameterAddress::new(ParameterOwner::Lane(0), ParameterKey::Filter).unwrap();
//! let mut lane = AutomationLane::new(address.clone());
//! lane.insert(AutomationPoint::new(bar, 0.0, Interpolation::Accelerating)).unwrap();
//! lane.insert(AutomationPoint::new(Frames::new(bar.get() * 2), 1.0, Interpolation::Linear))
//!     .unwrap();
//! timeline.set_automation(lane);
//!
//! // Accelerating: most of the travel happens late, so the change arrives.
//! let midpoint = timeline
//!     .automation_for(&address)
//!     .and_then(|lane| lane.value_at(Frames::new(bar.get() + bar.get() / 2)))
//!     .unwrap();
//! assert!(midpoint < 0.3);
//! ```

mod num;

pub mod automation;
pub mod parameter;
pub mod timeline;

pub use automation::{AutomationError, AutomationLane, AutomationPoint, MAX_POINTS};
pub use parameter::{DescriptorError, ParameterCurve, ParameterDescriptor, ParameterUnit};

// Re-exported so a caller building a timeline does not have to know which crate
// owns which half of a parameter. Identity is the document's (ADR-0007);
// description is this crate's.
pub use prv_project::{
    Interpolation, ParameterAddress, ParameterError, ParameterKey, ParameterOwner,
    PluginParameterId, MAX_PLUGIN_PARAMETER_LENGTH,
};
pub use timeline::{Clip, EditError, Snap, Timeline, MAX_CLIPS, MAX_LANES};
