//! The mix planner: turning "a three-hour set that builds" into an actual set.
//!
//! # What this crate is
//!
//! ADR-0006 divides the product along one seam. Musical decisions are
//! *computed*, under hard constraints, from analysed facts. Language is used
//! only to translate intent inward and reasoning outward. This crate is
//! everything on the computed side of that seam.
//!
//! It takes a [`Goal`] — a duration, an energy shape, a creativity setting —
//! and a set of [`Candidate`] tracks, and returns [`MixPlan`]s: ordered sets
//! with the evidence for every move retained.
//!
//! # Why the deterministic half is the larger half
//!
//! A language model can produce a plausible tracklist. It cannot guarantee that
//! no two adjacent tracks clash harmonically, that no tempo jump exceeds what a
//! deck can do, or that the same set comes back tomorrow. Master Prompt #3B
//! requires all three, and Master Prompt #27 requires the behaviour to be
//! testable — which a sampling method is not.
//!
//! So the search here is a beam search over a constraint-filtered candidate
//! space, and it is deterministic: the same inputs produce the same sets, on
//! every platform, offline. That is what makes the tests in this crate mean
//! something, and it is what lets the product keep its full musical capability
//! when a user turns cloud AI off.
//!
//! # Constraints are not low scores
//!
//! The single most important structural decision: a move that violates a hard
//! constraint is not ranked last, it is not generated. [`Rejection`] is a
//! separate type from a score for that reason.
//!
//! A scoring system where everything is comparable will, under enough pressure —
//! a short library, a long set, a demanding curve — eventually surface the
//! least-bad clash. A system where clashes are not in the candidate set cannot,
//! however desperate the search becomes. That is the difference between a
//! safety rule and a preference.
//!
//! # The evidence is the explanation
//!
//! Every score is kept with its components, and every plan keeps every score.
//! ADR-0006 requires the explanation a user reads to be a rendering of the
//! actual arithmetic rather than a story written afterwards, and that is only
//! possible if the arithmetic survives the search.
//!
//! # Layout
//!
//! | Module | Question it answers |
//! |--------|--------------------|
//! | [`goal`] | What did the user ask for? |
//! | [`candidate`] | What does the planner know about a track? |
//! | [`transition`] | Is this move allowed, and how good is it? |
//! | [`plan`] | Which ordering of tracks is best? |
//! | [`render`] | What edits turn that ordering into a mix? |
//!
//! # Example
//!
//! ```
//! use prv_mix::{Candidate, EnergyShape, Goal, TrackId, plan};
//! use prv_time::{Frames, Tempo};
//!
//! let five_minutes = Frames::new(44_100 * 300);
//! let library: Vec<Candidate> = (0..8)
//!     .map(|index| {
//!         Candidate::new(
//!             TrackId::new(index),
//!             five_minutes,
//!             Tempo::from_bpm(126.0).unwrap(),
//!             0.4 + 0.05 * index as f32,
//!         )
//!     })
//!     .collect();
//!
//! let goal = Goal::new(Frames::new(44_100 * 1800), EnergyShape::Rising);
//! let plans = plan(&library, &goal, 3).unwrap();
//!
//! assert!(!plans.is_empty());
//! // Every move after the opening carries the evidence that produced it.
//! for track in plans[0].tracks().iter().skip(1) {
//!     assert!(track.transition().is_some());
//! }
//! ```

mod num;

pub mod candidate;
pub mod goal;
pub mod plan;
pub mod render;
pub mod transition;

pub use candidate::{Candidate, MixPoint, MixPointRole, TrackId};
pub use goal::{Creativity, EnergyShape, Goal};
pub use plan::{plan, MixPlan, PlanError, PlannedTrack};
pub use render::{
    render, PlacementIds, RenderError, RenderedMix, RenderedTransition, Technique, TechniqueChoice,
};
pub use transition::{Component, Rejection, ScoreComponents, TransitionScore, Weights};
