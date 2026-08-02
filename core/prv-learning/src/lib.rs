//! The personal profile: what the system has learned about one user's taste.
//!
//! # What this crate is for
//!
//! Master Prompt #5 asks the product to become a DJ's own assistant rather than
//! a generic one. ADR-0006 says how: the weights of the objective function come
//! from the scenario *and* from the user's learned profile, while the hard
//! constraints come from neither.
//!
//! This crate is the second of those. It observes what the user did with what
//! was offered, and produces [`prv_mix::Weights`].
//!
//! # Three decisions that make this trustworthy rather than merely clever
//!
//! **It learns from behaviour, not from a preferences screen.** People are poor
//! at introspecting about taste and good at exercising it. A DJ who would tell
//! you they never cut between records cuts between records all evening when the
//! harmony is wrong.
//!
//! **It learns from what *distinguished* their choices, not from what they saw
//! most of.** A user who only ever saw harmonically excellent suggestions and
//! kept them all has said nothing about harmony. Correlation captures that;
//! averaging does not, and would over-weight the one thing they had no say in.
//!
//! **It cannot reach the safety rules.** Learning moves how much a component
//! counts, within fixed bounds, and never touches the constraints. A clashing
//! key is not a candidate at any profile, after any amount of observation.
//! Master Prompt #3B's rules are guarantees, and a guarantee a sufficiently
//! unusual user could train away is not one.
//!
//! # Privacy
//!
//! An observation holds six component scores and an outcome. It holds no track
//! name, no artist, no path and no device. Master Prompt #26 requires privacy by
//! design, and the cheapest way to keep a profile from becoming personal data is
//! for it never to contain any.
//!
//! The user can see what was inferred ([`Profile::explain`]), correct one part
//! of it ([`Profile::forget`]) and delete all of it ([`Profile::forget_all`]).
//! Nothing here is used to train anything shared; Master Prompt #26 forbids
//! using a user's work for that without explicit permission, and this crate has
//! no way to send anything anywhere.
//!
//! # Example
//!
//! ```
//! use prv_learning::{Observation, Outcome, Profile};
//! use prv_mix::{ScoreComponents, Weights};
//!
//! let mut profile = Profile::new();
//! // A user who keeps harmonically strong suggestions and rejects weak ones.
//! for index in 0..64 {
//!     let keeps = index % 2 == 0;
//!     profile.observe(Observation::new(
//!         index,
//!         ScoreComponents {
//!             harmonic: if keeps { 0.9 } else { 0.2 },
//!             tempo: 0.6,
//!             energy: 0.5,
//!             structure: 0.5,
//!             level: 0.5,
//!             vocal: 0.5,
//!         },
//!         if keeps { Outcome::Kept } else { Outcome::Rejected },
//!     ));
//! }
//!
//! assert!(profile.weights().harmonic > Weights::DEFAULT.harmonic);
//!
//! // And the user can see why, and disagree.
//! let leading = profile.explain();
//! assert!(leading[0].observations() > 0);
//! profile.forget_all();
//! assert_eq!(profile.weights(), Weights::DEFAULT);
//! ```

mod num;

pub mod observation;
pub mod profile;

pub use observation::{Observation, Outcome, COMPONENTS};
pub use profile::{Inference, Profile, FULL_CONFIDENCE_OBSERVATIONS, MAX_OBSERVATIONS};
