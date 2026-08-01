//! The project, as an append-only log of operations.
//!
//! # Why a log rather than a document
//!
//! Four specifications converge on the same requirement from different
//! directions. Master Prompt #3C requires every action to be reversible and
//! forbids overwriting originals. Master Prompt #7 requires version history at
//! every stage. Master Prompt #9 states that projects are immutable snapshots
//! supporting restore, compare, duplicate, branch and merge. Master Prompt #24
//! requires offline editing with incremental synchronisation and conflicts that
//! are explained rather than silently resolved.
//!
//! A mutable document with an undo stack satisfies none of them. Undo stacks are
//! process-local, do not survive a crash, cannot branch, cannot be compared and
//! cannot be synchronised incrementally. Retrofitting any one of those later
//! means rewriting every edit path in the application.
//!
//! So the project *is* the log. State is a fold over it. Undo, redo, named
//! versions, branching, comparison, crash recovery and incremental sync are then
//! consequences of one mechanism rather than seven features, which is why this
//! module is small and the features it provides are not.
//!
//! # Determinism
//!
//! The fold is a pure function and the log has a total order that every device
//! computes identically. The same log therefore produces the same project on
//! every platform and in every build that understands its operation types. That
//! is what makes preview and export agree, and what makes a synchronised project
//! converge without a central authority.
//!
//! # Conflicts are found, not invented
//!
//! Every operation records the version vector its author had seen. Two
//! operations are concurrent when neither author had seen the other — a fact,
//! not an estimate. Concurrent operations that commute are merged silently;
//! concurrent operations that do not are reported with both intentions intact,
//! as Master Prompt #24 requires.
//!
//! Lamport timestamps alone cannot make that distinction — they order events but
//! cannot tell "after" from "elsewhere" — which is why the extra bookkeeping is
//! worth its cost here.

mod log;
mod operation;
mod state;

pub use log::{Conflict, ConflictKind, MergeReport, OperationLog, ProjectError};
pub use operation::{
    DeviceId, MarkerId, MarkerKind, Operation, OperationId, OperationPayload, PlacementId,
    TrackRef, VersionVector,
};
pub use state::{Marker, Placement, ProjectState};
