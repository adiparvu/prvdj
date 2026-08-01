//! Musical keys, the Camelot wheel, and harmonic compatibility.
//!
//! # What this crate is for
//!
//! Master Prompt #3B requires the system to understand harmonic compatibility
//! and forbids it from ever producing clashing keys. Master Prompt #12 requires
//! that the same idea be expressible to a beginner as "these songs naturally
//! sound good together". Master Prompt #20 requires key detection to report
//! compatible keys and potential harmonic risks.
//!
//! All three need the same thing underneath: a model of how two keys relate,
//! and a judgement about whether moving between them works.
//!
//! # Relations first, scores second
//!
//! [`compatibility`] returns a [`KeyRelation`] alongside a numeric score. The
//! relation is the important half.
//!
//! ADR-0006 requires every recommendation to carry an evidence record from which
//! its explanation is *derived*, rather than a plausible story written after the
//! fact. "These two tracks are a fifth apart" is a fact about music that stays
//! true regardless of how the planner is tuned. The numeric weight attached to
//! it is a tuning parameter that will change as the objective function is
//! calibrated. Keeping them separate means explanations remain truthful across
//! that tuning, and means a beginner and a professional can be shown the same
//! fact in different words.
//!
//! # Calibration status
//!
//! The relation classification below reflects established harmonic mixing
//! practice and is stable. The numeric weights are **initial values**, to be
//! calibrated against listener evaluation during Phase 3 as ADR-0006 describes.
//! They are deliberately documented as provisional rather than presented as
//! settled, because a confidence the user cannot trust is worse than none.

mod camelot;
mod compatibility;
mod key;

pub use camelot::{CamelotCode, Wheel};
pub use compatibility::{compatibility, HarmonicCompatibility, HarmonicSafety, KeyRelation};
pub use key::{Key, Mode, PitchClass};
