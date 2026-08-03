use core::fmt;
use std::collections::BTreeMap;

use crate::operation::{
    CarriedOperation, DeviceId, Operation, OperationId, OperationPayload, Target, VersionVector,
};
use crate::state::ProjectState;

/// The largest number of operations a single log will hold.
///
/// A million edits is far beyond any real project — an intensively edited
/// two-hour set is a few tens of thousands. The bound exists so that a corrupted
/// or hostile file cannot ask for an unbounded allocation.
const MAX_OPERATIONS: usize = 1_000_000;

/// The largest number of named versions.
const MAX_LABELS: usize = 10_000;

/// The largest number of operations a log will carry without understanding.
///
/// Bounded separately from the operations it *can* read, and for a different
/// reason. That bound guards against a corrupt file; this one guards against a
/// peer — a build claiming to be newer could otherwise fill a device with
/// operations it will never be able to interpret. Reaching it means something is
/// wrong rather than that a project got large, so it refuses rather than
/// discarding: a carried operation is still somebody's work.
const MAX_CARRIED: usize = 100_000;

/// Failures from operating on a log.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ProjectError {
    /// The log already contains an operation with this identity.
    ///
    /// Not an error during merge, where duplicates are expected and skipped;
    /// an error when appending locally, where it means the device reused a
    /// sequence number and its future operations would be ambiguous.
    DuplicateOperation(OperationId),
    /// The log has reached its bound.
    LogFull,
    /// The log is carrying as many unreadable operations as it will.
    ///
    /// Reaching this means a peer has sent a great many operations this build
    /// cannot interpret, which is a thing to tell somebody about rather than a
    /// thing to handle quietly.
    CarriedFull,
    /// Too many named versions.
    TooManyLabels,
    /// No version by that name.
    UnknownLabel(String),
    /// A position beyond the end of the log.
    PositionOutOfRange {
        /// The position asked for.
        requested: usize,
        /// The length of the log.
        length: usize,
    },
}

impl fmt::Display for ProjectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateOperation(id) => write!(f, "operation {id} is already in the log"),
            Self::LogFull => f.write_str("the project has too many operations"),
            Self::CarriedFull => f.write_str(
                "the project is already carrying as many operations as it can that were made \
                 with a newer version",
            ),
            Self::TooManyLabels => f.write_str("the project has too many named versions"),
            Self::UnknownLabel(name) => write!(f, "no version named \"{name}\""),
            Self::PositionOutOfRange { requested, length } => {
                write!(
                    f,
                    "position {requested} is beyond the log's {length} operations"
                )
            }
        }
    }
}

impl core::error::Error for ProjectError {}

/// Why two operations conflict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ConflictKind {
    /// Both edited the same property of the same thing, at the same time,
    /// without either author seeing the other.
    ConcurrentEdit,

    /// Two different operations arrived under the same identity.
    ///
    /// An identity is a device and a sequence number, which is collision-free
    /// across devices and *not* across branches of one device: two branches
    /// taken from the same point both allocate the next number. The high-water
    /// mark in [`OperationLog::branch_at`] removes the common case; two
    /// independent branches of one device can still meet.
    ///
    /// Treating this as "already present" is what the merge used to do, and it
    /// silently discarded the incoming work — precisely what Master Prompt #24
    /// forbids. Reported instead, so the person who made the edit is told it
    /// could not be applied under that name rather than watching it vanish.
    SameNameDifferentWork,
}

impl fmt::Display for ConflictKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ConcurrentEdit => f.write_str("both edited this at the same time"),
            Self::SameNameDifferentWork => {
                f.write_str("two different edits arrived under the same name")
            }
        }
    }
}

/// Two operations that disagree.
///
/// Reported rather than resolved. The total order guarantees the project
/// *converges* — every device computes the same state — but convergence is not
/// correctness: whichever operation sorts last silently wins, and one person's
/// intention disappears. Master Prompt #24 forbids that, so the pair is
/// surfaced with both intentions intact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict {
    /// The operation already in the log.
    pub existing: OperationId,
    /// The operation that arrived.
    pub incoming: OperationId,
    /// What both touched.
    pub target: Target,
    /// Why they disagree.
    pub kind: ConflictKind,
}

impl fmt::Display for Conflict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} and {} both changed {} — {}",
            self.existing, self.incoming, self.target, self.kind
        )
    }
}

/// What undoing a device's last edit would do.
///
/// # Why this is not simply a list of operations
///
/// In a shared project, "undo my last edit" can collide with somebody else's
/// later one. If I move a clip and you then move it again, the inverse of *my*
/// move — computed against the state before it — puts the clip back where it was
/// before either of us touched it, discarding your work without saying so.
///
/// Master Prompt #24 forbids silently discarding work and requires conflicts to
/// be explained, so this reports the collision instead of performing it and the
/// interface asks rather than guesses. Refusing is not a limitation to remove
/// later: there is no correct silent answer, and the two plausible ones — revert
/// theirs, or ignore mine — are each wrong for somebody.
// Not `Eq`: an operation carries a payload whose parameter values are floating
// point, and ADR-0003 records why those are compared for equality and not for
// identity.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Undo {
    /// The device has nothing left to undo.
    Nothing,
    /// Another device changed the same thing afterwards.
    Superseded {
        /// Which device.
        by: DeviceId,
    },
    /// The operations that would undo it.
    Operations(Vec<Operation>),
}

impl Undo {
    /// The operations, if there are any.
    #[must_use]
    pub fn operations(&self) -> &[Operation] {
        match self {
            Self::Operations(operations) => operations,
            Self::Nothing | Self::Superseded { .. } => &[],
        }
    }

    /// Whether anything would happen.
    #[must_use]
    pub const fn is_possible(&self) -> bool {
        matches!(self, Self::Operations(_))
    }
}

/// What a merge did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MergeReport {
    /// Operations that were not already present and have been applied.
    pub applied: usize,
    /// Operations already present, skipped.
    ///
    /// Expected, not exceptional: synchronisation ships whatever the other side
    /// might not have, and overlap is normal.
    pub already_present: usize,
    /// Pairs that disagree and need a decision.
    pub conflicts: Vec<Conflict>,
}

impl MergeReport {
    /// Whether anything needs the user's attention.
    #[must_use]
    pub fn needs_review(&self) -> bool {
        !self.conflicts.is_empty()
    }
}

impl fmt::Display for MergeReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} applied, {} already present, {} to review",
            self.applied,
            self.already_present,
            self.conflicts.len()
        )
    }
}

/// The project: an append-only, totally ordered log of operations.
///
/// # What a position means
///
/// A position is a count of operations from the start. Position zero is the
/// empty project; position `len()` is the present. Named versions and branches
/// are both positions, which is why they cost almost nothing.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct OperationLog {
    operations: Vec<Operation>,
    labels: BTreeMap<String, usize>,
    seen: VersionVector,
    /// The highest sequence this log has ever handed out, per device.
    ///
    /// Distinct from `seen`, which is what it has *received*. A branch inherits
    /// this so it never re-allocates a number the trunk already used; without
    /// it, merging a branch back reads as "already present" and the branch's
    /// work disappears silently.
    issued: VersionVector,
    /// Operations this build cannot interpret, in the same total order.
    ///
    /// Held so that a device running an older build is a relay rather than a
    /// hole in a fleet, and held *here* rather than in the synchronisation layer
    /// so that being closed and reopened does not quietly end the favour. See
    /// [`CarriedOperation`].
    carried: Vec<CarriedOperation>,
}

impl OperationLog {
    /// Creates an empty log.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of operations.
    #[must_use]
    pub fn len(&self) -> usize {
        self.operations.len()
    }

    /// Whether the log is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.operations.is_empty()
    }

    /// Every operation, in order.
    pub fn operations(&self) -> impl Iterator<Item = &Operation> {
        self.operations.iter()
    }

    /// What this log has seen.
    ///
    /// Sent to the other side of a synchronisation so it can compute what to
    /// send back — which is what makes sync incremental rather than a full
    /// exchange.
    #[must_use]
    pub const fn version_vector(&self) -> &VersionVector {
        &self.seen
    }

    /// Takes in operations this build cannot interpret, so they can travel on.
    ///
    /// Returns how many were new. Ones already held are skipped, exactly as a
    /// merge skips an operation it already has: a peer sends whatever the other
    /// side might be missing, so overlap is the normal case.
    ///
    /// # What carrying commits this log to
    ///
    /// The identity is recorded as seen, which stops peers resending it — and
    /// that is only honest because the bytes are kept. A device that recorded
    /// an operation as seen without keeping it would be telling the fleet it
    /// holds work it cannot produce, and the work would end at that device
    /// while every peer believed it had arrived.
    ///
    /// # Errors
    ///
    /// Returns [`ProjectError::CarriedFull`] at the bound, without taking any
    /// of the batch. Refusing rather than discarding is the same rule the
    /// outbox follows and for the same reason: this is somebody's work, not a
    /// record of it.
    pub fn carry(&mut self, operations: &[CarriedOperation]) -> Result<usize, ProjectError> {
        let fresh: Vec<&CarriedOperation> = operations
            .iter()
            .filter(|operation| !self.holds(operation.id()))
            .collect();
        if self.carried.len().saturating_add(fresh.len()) > MAX_CARRIED {
            return Err(ProjectError::CarriedFull);
        }

        let kept = fresh.len();
        for operation in fresh {
            let key = operation.sort_key();
            let position = self
                .carried
                .partition_point(|existing| existing.sort_key() < key);
            self.seen.observe(operation.id());
            self.issued.observe(operation.id());
            self.carried.insert(position, operation.clone());
        }
        Ok(kept)
    }

    /// Whether an operation is present at all, readable or not.
    ///
    /// Distinct from [`OperationLog::contains`], which asks whether it is
    /// present *and applicable*. Deduplication needs this one: an operation
    /// received twice, once before an upgrade and once after, must not be
    /// stored twice.
    #[must_use]
    pub fn holds(&self, id: OperationId) -> bool {
        self.contains(id) || self.carried.iter().any(|carried| carried.id() == id)
    }

    /// Every operation this build cannot interpret, in order.
    pub fn carried(&self) -> impl Iterator<Item = &CarriedOperation> {
        self.carried.iter()
    }

    /// How many operations are being carried without being understood.
    ///
    /// Worth showing a person. A non-zero count means part of this project was
    /// made with a newer version of the application — true, actionable, and the
    /// only honest thing to say about it.
    #[must_use]
    pub fn carried_count(&self) -> usize {
        self.carried.len()
    }

    /// The carried operations another device has not seen.
    ///
    /// The counterpart of [`OperationLog::operations_since`], and the reason a
    /// relay works: what this device cannot read, it can still be the reason
    /// somebody else receives.
    #[must_use]
    pub fn carried_since(&self, other: &VersionVector) -> Vec<CarriedOperation> {
        self.carried
            .iter()
            .filter(|operation| !other.has_seen(operation.id()))
            .cloned()
            .collect()
    }

    /// Re-reads carried operations, keeping the ones this build now understands.
    ///
    /// Returns how many were promoted. What an upgrade is for: an operation
    /// that arrived from a newer build, and was carried because it could not be
    /// read, becomes an ordinary part of the project the moment this build
    /// learns its meaning. The user sees work appear that was theirs all along.
    ///
    /// Safe to call at any time and cheap when there is nothing to do, which is
    /// why the natural place for it is immediately after a project is opened.
    pub fn promote_carried(&mut self) -> usize {
        let mut promoted: Vec<Operation> = Vec::new();
        self.carried.retain(|carried| {
            match crate::wire::read_carried(carried) {
                Some(operation) => {
                    promoted.push(operation);
                    false
                }
                // Still beyond this build. Keep carrying it.
                None => true,
            }
        });

        let count = promoted.len();
        for operation in promoted {
            let key = operation.sort_key();
            let position = self
                .operations
                .partition_point(|existing| existing.sort_key() < key);
            // `seen` and `issued` already know about it: it was observed when it
            // was carried, and an identity is not observed twice.
            self.operations.insert(position, operation);
        }
        count
    }

    /// Creates an operation authored by a device against this log's current
    /// state, without appending it.
    ///
    /// The caller appends it, which keeps authoring and appending separable: an
    /// operation can be built, previewed and discarded — which is exactly what
    /// Master Prompt #19 requires of an AI proposal the user has not accepted.
    #[must_use]
    pub fn author(
        &self,
        device: DeviceId,
        timestamp_micros: i64,
        payload: OperationPayload,
    ) -> Operation {
        let sequence = self.next_sequence_for(device);
        Operation {
            id: OperationId::new(device, sequence),
            context: self.seen.clone(),
            timestamp_micros,
            payload,
        }
    }

    /// The next sequence number this log may hand out for a device.
    ///
    /// The higher of what it has seen and what it has issued. The two differ
    /// only on a branch, and that is exactly where the difference matters.
    fn next_sequence_for(&self, device: DeviceId) -> u64 {
        self.seen
            .sequence_for(device)
            .max(self.issued.sequence_for(device))
            .saturating_add(1)
    }

    /// Creates a run of operations authored by one device, without appending
    /// them.
    ///
    /// # Why this exists rather than calling `author` in a loop
    ///
    /// `author` numbers an operation from what the log has *seen*, so two calls
    /// before an append produce the same identity. That was invisible while a
    /// gesture was one operation. Since `prv-timeline::Edit` began carrying a
    /// list — trimming the front of a clip is two, deleting one with automation
    /// is many, undoing a removal is two — the natural loop has been producing
    /// colliding identifiers, and the second append fails or, across a branch,
    /// two different payloads share a name.
    ///
    /// This numbers the whole run at once. Every gesture that produces more than
    /// one operation goes through it.
    #[must_use]
    pub fn author_all(
        &self,
        device: DeviceId,
        timestamp_micros: i64,
        payloads: Vec<OperationPayload>,
    ) -> Vec<Operation> {
        let base = self.next_sequence_for(device).saturating_sub(1);
        payloads
            .into_iter()
            .enumerate()
            .map(|(offset, payload)| {
                let step = u64::try_from(offset).unwrap_or(u64::MAX);
                Operation {
                    id: OperationId::new(device, base.saturating_add(step).saturating_add(1)),
                    context: self.seen.clone(),
                    timestamp_micros,
                    payload,
                }
            })
            .collect()
    }

    /// Appends an operation, keeping the log in total order.
    ///
    /// # Errors
    ///
    /// Returns [`ProjectError::DuplicateOperation`] if the identity is already
    /// present, or [`ProjectError::LogFull`] at the bound.
    pub fn append(&mut self, operation: Operation) -> Result<usize, ProjectError> {
        if self.operations.len() >= MAX_OPERATIONS {
            return Err(ProjectError::LogFull);
        }
        if self.contains(operation.id) {
            return Err(ProjectError::DuplicateOperation(operation.id));
        }

        let key = operation.sort_key();
        let position = self
            .operations
            .partition_point(|existing| existing.sort_key() < key);
        self.seen.observe(operation.id);
        // A number that has been used is never handed out again, even if the log
        // is later branched from a point before it.
        self.issued.observe(operation.id);
        self.operations.insert(position, operation);
        Ok(position)
    }

    /// Whether an operation is already present.
    #[must_use]
    pub fn contains(&self, id: OperationId) -> bool {
        self.seen.has_seen(id) && self.operations.iter().any(|operation| operation.id == id)
    }

    /// Folds the log up to a position into a project.
    ///
    /// A position beyond the end is clamped to the end rather than rejected: it
    /// means "the present", and asking for the present should not be an error.
    #[must_use]
    pub fn materialise(&self, upto: usize) -> ProjectState {
        let mut state = ProjectState::new();
        for operation in self.operations.iter().take(upto) {
            state.apply(&operation.payload);
        }
        state
    }

    /// Folds the whole log.
    #[must_use]
    pub fn state(&self) -> ProjectState {
        self.materialise(self.operations.len())
    }

    /// Names the version at a position.
    ///
    /// # Errors
    ///
    /// Returns [`ProjectError::PositionOutOfRange`] for a position beyond the
    /// end, or [`ProjectError::TooManyLabels`] at the bound.
    pub fn label(&mut self, name: &str, position: usize) -> Result<(), ProjectError> {
        if position > self.operations.len() {
            return Err(ProjectError::PositionOutOfRange {
                requested: position,
                length: self.operations.len(),
            });
        }
        if !self.labels.contains_key(name) && self.labels.len() >= MAX_LABELS {
            return Err(ProjectError::TooManyLabels);
        }
        self.labels.insert(name.to_owned(), position);
        Ok(())
    }

    /// The position a name refers to.
    ///
    /// # Errors
    ///
    /// Returns [`ProjectError::UnknownLabel`] if there is no such version.
    pub fn position_of(&self, name: &str) -> Result<usize, ProjectError> {
        self.labels
            .get(name)
            .copied()
            .ok_or_else(|| ProjectError::UnknownLabel(name.to_owned()))
    }

    /// Every named version, in name order.
    pub fn labels(&self) -> impl Iterator<Item = (&str, usize)> {
        self.labels
            .iter()
            .map(|(name, position)| (name.as_str(), *position))
    }

    /// Materialises a named version.
    ///
    /// # Errors
    ///
    /// Returns [`ProjectError::UnknownLabel`] if there is no such version.
    pub fn state_at_label(&self, name: &str) -> Result<ProjectState, ProjectError> {
        Ok(self.materialise(self.position_of(name)?))
    }

    /// Forks the log at a position.
    ///
    /// The result is an independent project sharing history up to that point.
    /// Because operations carry their own identity and causal context, the two
    /// can be merged again later without any record of the fork having been
    /// kept — branching costs a copy and nothing else.
    ///
    /// # Errors
    ///
    /// Returns [`ProjectError::PositionOutOfRange`] for a position beyond the
    /// end.
    pub fn branch_at(&self, position: usize) -> Result<Self, ProjectError> {
        if position > self.operations.len() {
            return Err(ProjectError::PositionOutOfRange {
                requested: position,
                length: self.operations.len(),
            });
        }
        let mut branch = Self::new();
        for operation in self.operations.iter().take(position) {
            branch.seen.observe(operation.id);
            branch.operations.push(operation.clone());
        }
        // Every number this log has ever handed out, carried forward. Without
        // it a branch taken at position one re-allocates the numbers the trunk
        // used after that point, and merging back looks like "already present"
        // — the branch's work disappearing without a word.
        branch.issued.clone_from(&self.issued);
        for operation in &self.operations {
            branch.issued.observe(operation.id);
        }
        Ok(branch)
    }

    /// Everything this log has that another has not seen.
    ///
    /// The payload of an incremental synchronisation. A project's whole editing
    /// history is typically kilobytes, so this reconciles in seconds on the sort
    /// of connection a venue has — while the audio it refers to, which is
    /// gigabytes, transfers separately and selectively.
    #[must_use]
    pub fn operations_since(&self, other: &VersionVector) -> Vec<Operation> {
        self.operations
            .iter()
            .filter(|operation| !other.has_seen(operation.id))
            .cloned()
            .collect()
    }

    /// Merges operations from elsewhere.
    ///
    /// Operations already present are skipped. New operations are inserted in
    /// total order, so both sides converge on the same state whatever order they
    /// arrive in. Concurrent operations that do not commute are reported.
    ///
    /// # Errors
    ///
    /// Returns [`ProjectError::LogFull`] if the merged log would exceed the
    /// bound. Operations applied before the bound was reached remain applied;
    /// the report says how many.
    pub fn merge(&mut self, incoming: &[Operation]) -> Result<MergeReport, ProjectError> {
        let mut report = MergeReport::default();

        for operation in incoming {
            if let Some(held) = self
                .operations
                .iter()
                .find(|candidate| candidate.id == operation.id)
            {
                // The same name. Whether it is the same *work* decides whether
                // this is a duplicate delivery or a collision, and the two must
                // not be confused: one is boring and the other is somebody's
                // edit about to disappear.
                if held.payload == operation.payload {
                    report.already_present += 1;
                } else {
                    report.conflicts.push(Conflict {
                        existing: held.id,
                        incoming: operation.id,
                        target: operation.payload.target(),
                        kind: ConflictKind::SameNameDifferentWork,
                    });
                }
                continue;
            }

            // Compare against what is already here *before* inserting, so an
            // operation is never reported as conflicting with itself.
            for existing in &self.operations {
                if existing.is_concurrent_with(operation)
                    && !existing.payload.commutes_with(&operation.payload)
                {
                    report.conflicts.push(Conflict {
                        existing: existing.id,
                        incoming: operation.id,
                        target: operation.payload.target(),
                        kind: ConflictKind::ConcurrentEdit,
                    });
                }
            }

            self.append(operation.clone())?;
            report.applied += 1;
        }

        Ok(report)
    }

    /// What undoing a device's last edit would do.
    ///
    /// # Why this is not simply a list of operations
    ///
    /// In a shared project, "undo my last edit" can collide with somebody
    /// else's later one. If I move a clip and you then move it again, the
    /// inverse of *my* move — computed against the state before it — puts the
    /// clip back where it was before either of us touched it, discarding your
    /// work without saying so.
    ///
    /// Master Prompt #24 forbids silently discarding work and requires
    /// conflicts to be explained. So this reports the collision instead of
    /// performing it, and the interface asks rather than guesses. Refusing is
    /// not a limitation to remove later: there is no correct silent answer, and
    /// the two plausible ones — revert theirs, or ignore mine — are both wrong
    /// for somebody.
    /// Authors the operations that undo the last one a device made.
    ///
    /// Returns an empty list when the device has nothing left to undo, or when
    /// what it did has no inverse — undoing the removal of something that was
    /// never there.
    ///
    /// A list because one gesture's inverse can need more than one operation:
    /// restoring a trimmed clip takes a placement and the source offset that
    /// `PlaceTrack` cannot carry.
    ///
    /// # Why by device
    ///
    /// In a shared project, undo means "undo *my* last edit", not "undo whatever
    /// happened last". Undoing a collaborator's work because they happened to
    /// act more recently would be the least expected behaviour available.
    #[must_use]
    pub fn undo_for(&self, device: DeviceId, timestamp_micros: i64) -> Undo {
        let Some(position) = self
            .operations
            .iter()
            .rposition(|operation| operation.id.device == device)
        else {
            return Undo::Nothing;
        };
        let Some(operation) = self.operations.get(position) else {
            return Undo::Nothing;
        };

        // Somebody else may have changed the same thing since. Undoing against
        // a state they have moved on from would revert their work as well as
        // mine, and Master Prompt #24 forbids discarding work silently.
        let target = operation.payload.target();
        if let Some(later) = self
            .operations
            .get(position.saturating_add(1)..)
            .unwrap_or_default()
            .iter()
            .find(|candidate| candidate.id.device != device && candidate.payload.target() == target)
        {
            return Undo::Superseded {
                by: later.id.device,
            };
        }

        // The state as it was immediately before that operation is what the
        // inverse must be computed against.
        let before = self.materialise(position);
        let inverses = before.inverses_of(&operation.payload);
        if inverses.is_empty() {
            return Undo::Nothing;
        }
        Undo::Operations(self.author_all(device, timestamp_micros, inverses))
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::panic,
        reason = "a test that cannot build its own fixture should fail loudly"
    )]

    use super::*;
    use crate::operation::{MarkerId, MarkerKind, PlacementId, TrackRef};
    use prv_time::Frames;

    fn device(id: u64) -> DeviceId {
        DeviceId::new(id)
    }

    fn place(id: u64, position: i64) -> OperationPayload {
        OperationPayload::PlaceTrack {
            placement: PlacementId::new(id),
            track: TrackRef::new(id),
            position: Frames::new(position),
            length: Frames::new(48_000),
            lane: 0,
        }
    }

    fn move_to(id: u64, position: i64) -> OperationPayload {
        OperationPayload::MovePlacement {
            placement: PlacementId::new(id),
            position: Frames::new(position),
            lane: 0,
        }
    }

    /// Appends an authored operation, asserting it was accepted.
    fn commit(log: &mut OperationLog, device_id: u64, payload: OperationPayload) {
        let operation = log.author(device(device_id), 0, payload);
        assert!(log.append(operation).is_ok());
    }

    #[test]
    fn an_empty_log_materialises_an_empty_project() {
        let log = OperationLog::new();
        assert!(log.is_empty());
        assert_eq!(log.state(), ProjectState::new());
    }

    #[test]
    fn operations_fold_into_state() {
        let mut log = OperationLog::new();
        commit(
            &mut log,
            1,
            OperationPayload::SetProjectName {
                name: String::from("Sunset"),
            },
        );
        commit(&mut log, 1, place(1, 0));
        commit(&mut log, 1, place(2, 48_000));

        let state = log.state();
        assert_eq!(state.name, "Sunset");
        assert_eq!(state.placements.len(), 2);
        assert_eq!(state.duration(), Frames::new(96_000));
    }

    #[test]
    fn materialising_a_position_gives_the_project_as_it_was() {
        // The mechanism behind comparison and restore: no separate history
        // structure, just a shorter fold.
        let mut log = OperationLog::new();
        commit(&mut log, 1, place(1, 0));
        commit(&mut log, 1, place(2, 48_000));
        commit(&mut log, 1, place(3, 96_000));

        assert_eq!(log.materialise(0).placements.len(), 0);
        assert_eq!(log.materialise(1).placements.len(), 1);
        assert_eq!(log.materialise(2).placements.len(), 2);
        assert_eq!(
            log.materialise(99).placements.len(),
            3,
            "clamped to the present"
        );
    }

    #[test]
    fn a_duplicate_operation_is_refused() {
        let mut log = OperationLog::new();
        let operation = log.author(device(1), 0, place(1, 0));
        assert!(log.append(operation.clone()).is_ok());
        assert_eq!(
            log.append(operation.clone()),
            Err(ProjectError::DuplicateOperation(operation.id))
        );
    }

    #[test]
    fn named_versions_are_positions() {
        let mut log = OperationLog::new();
        commit(&mut log, 1, place(1, 0));
        assert!(log.label("first take", log.len()).is_ok());
        commit(&mut log, 1, place(2, 48_000));

        let restored = log.state_at_label("first take");
        assert_eq!(restored.map(|state| state.placements.len()), Ok(1));
        assert_eq!(log.state().placements.len(), 2);
    }

    #[test]
    fn an_unknown_version_is_reported_by_name() {
        let log = OperationLog::new();
        assert_eq!(
            log.position_of("nothing"),
            Err(ProjectError::UnknownLabel(String::from("nothing")))
        );
    }

    #[test]
    fn a_label_beyond_the_log_is_refused() {
        let mut log = OperationLog::new();
        assert_eq!(
            log.label("future", 5),
            Err(ProjectError::PositionOutOfRange {
                requested: 5,
                length: 0
            })
        );
    }

    #[test]
    fn branching_forks_history_without_recording_the_fork() {
        let mut log = OperationLog::new();
        commit(&mut log, 1, place(1, 0));
        commit(&mut log, 1, place(2, 48_000));

        let branch = log.branch_at(1);
        assert!(branch.is_ok());
        let Ok(mut branch) = branch else { return };

        assert_eq!(branch.len(), 1);
        commit(&mut branch, 2, place(9, 200_000));

        // The two projects have diverged, and neither knows about the other.
        assert_eq!(log.state().placements.len(), 2);
        assert_eq!(branch.state().placements.len(), 2);
        assert!(branch.state().placements.contains_key(&PlacementId::new(9)));
        assert!(!log.state().placements.contains_key(&PlacementId::new(9)));
    }

    #[test]
    fn a_branch_can_be_merged_back() {
        let mut log = OperationLog::new();
        commit(&mut log, 1, place(1, 0));

        let Ok(mut branch) = log.branch_at(1) else {
            unreachable!()
        };
        commit(&mut branch, 2, place(2, 48_000));
        commit(&mut log, 1, place(3, 96_000));

        let incoming = branch.operations_since(log.version_vector());
        let report = log.merge(&incoming);
        assert_eq!(report.as_ref().map(|r| r.applied), Ok(1));
        assert_eq!(log.state().placements.len(), 3);
    }

    #[test]
    fn synchronisation_sends_only_what_the_other_side_lacks() {
        // The reason a project reconciles in seconds on a venue's connection.
        let mut left = OperationLog::new();
        commit(&mut left, 1, place(1, 0));
        commit(&mut left, 1, place(2, 48_000));

        let mut right = OperationLog::new();
        let incoming = left.operations_since(right.version_vector());
        assert_eq!(incoming.len(), 2);
        assert!(right.merge(&incoming).is_ok());

        // Now nothing is outstanding in either direction.
        assert!(left.operations_since(right.version_vector()).is_empty());
        assert!(right.operations_since(left.version_vector()).is_empty());
    }

    #[test]
    fn merging_the_same_operations_twice_changes_nothing() {
        let mut left = OperationLog::new();
        commit(&mut left, 1, place(1, 0));
        let mut right = OperationLog::new();
        let incoming = left.operations_since(right.version_vector());

        assert!(right.merge(&incoming).is_ok());
        let second = right.merge(&incoming);
        assert_eq!(second.as_ref().map(|r| r.applied), Ok(0));
        assert_eq!(second.as_ref().map(|r| r.already_present), Ok(1));
        assert_eq!(right.len(), 1);
    }

    #[test]
    fn two_devices_converge_on_the_same_state() {
        // The property that makes offline editing safe: whatever order the
        // operations arrive in, both sides end up identical.
        let mut left = OperationLog::new();
        let mut right = OperationLog::new();

        commit(&mut left, 1, place(1, 0));
        commit(&mut left, 1, place(2, 48_000));
        commit(&mut right, 2, place(3, 96_000));
        commit(&mut right, 2, place(4, 144_000));

        let to_right = left.operations_since(right.version_vector());
        let to_left = right.operations_since(left.version_vector());
        assert!(right.merge(&to_right).is_ok());
        assert!(left.merge(&to_left).is_ok());

        assert_eq!(left.state(), right.state());
        assert_eq!(left.len(), right.len());
    }

    #[test]
    fn convergence_holds_whatever_order_operations_arrive_in() {
        let mut source = OperationLog::new();
        commit(&mut source, 1, place(1, 0));
        commit(&mut source, 2, place(2, 48_000));
        commit(&mut source, 3, place(3, 96_000));
        let all = source.operations_since(&VersionVector::new());

        let forward = {
            let mut log = OperationLog::new();
            assert!(log.merge(&all).is_ok());
            log.state()
        };
        let reversed = {
            let mut log = OperationLog::new();
            let mut backwards = all.clone();
            backwards.reverse();
            assert!(log.merge(&backwards).is_ok());
            log.state()
        };
        assert_eq!(
            forward, reversed,
            "order of arrival must not change the result"
        );
    }

    #[test]
    fn concurrent_edits_to_different_things_merge_silently() {
        // Two people working on different parts of a set must not be
        // interrupted.
        let mut left = OperationLog::new();
        commit(&mut left, 1, place(1, 0));
        commit(&mut left, 1, place(2, 48_000));

        let Ok(mut right) = left.branch_at(left.len()) else {
            unreachable!()
        };
        commit(&mut left, 1, move_to(1, 200_000));
        commit(&mut right, 2, move_to(2, 300_000));

        let incoming = right.operations_since(left.version_vector());
        let report = left.merge(&incoming);
        assert_eq!(
            report.as_ref().map(MergeReport::needs_review),
            Ok(false),
            "different placements must not conflict"
        );
    }

    #[test]
    fn concurrent_edits_to_the_same_thing_are_reported() {
        // Both people meant something. Whichever the total order puts last would
        // silently win, so the pair reaches the user instead.
        let mut left = OperationLog::new();
        commit(&mut left, 1, place(1, 0));

        let Ok(mut right) = left.branch_at(left.len()) else {
            unreachable!()
        };
        commit(&mut left, 1, move_to(1, 200_000));
        commit(&mut right, 2, move_to(1, 300_000));

        let incoming = right.operations_since(left.version_vector());
        let report = left.merge(&incoming);
        assert!(report.is_ok());
        let Ok(report) = report else { return };

        assert!(report.needs_review());
        assert_eq!(report.conflicts.len(), 1);
        let Some(conflict) = report.conflicts.first() else {
            unreachable!()
        };
        assert_eq!(conflict.target, Target::Placement(PlacementId::new(1)));
        assert_eq!(conflict.kind, ConflictKind::ConcurrentEdit);
        // And the project still converges: a conflict is a report, not a halt.
        assert_eq!(left.state().placements.len(), 1);
    }

    #[test]
    fn a_sequential_edit_is_not_a_conflict() {
        // The distinction a Lamport clock alone cannot make. Here the second
        // device *saw* the first edit before making its own.
        let mut left = OperationLog::new();
        commit(&mut left, 1, place(1, 0));
        commit(&mut left, 1, move_to(1, 200_000));

        let mut right = OperationLog::new();
        let incoming = left.operations_since(right.version_vector());
        assert!(right.merge(&incoming).is_ok());
        commit(&mut right, 2, move_to(1, 300_000));

        let back = right.operations_since(left.version_vector());
        let report = left.merge(&back);
        assert_eq!(
            report.as_ref().map(MergeReport::needs_review),
            Ok(false),
            "an edit made after seeing another is not concurrent with it"
        );
    }

    #[test]
    fn a_concurrent_removal_does_not_conflict_with_an_edit() {
        // Asking a user to adjudicate a move against a delete wastes their
        // attention: the outcome is visible either way.
        let mut left = OperationLog::new();
        commit(&mut left, 1, place(1, 0));

        let Ok(mut right) = left.branch_at(left.len()) else {
            unreachable!()
        };
        commit(&mut left, 1, move_to(1, 200_000));
        commit(
            &mut right,
            2,
            OperationPayload::RemovePlacement {
                placement: PlacementId::new(1),
            },
        );

        let incoming = right.operations_since(left.version_vector());
        let report = left.merge(&incoming);
        assert_eq!(report.map(|r| r.needs_review()), Ok(false));
    }

    #[test]
    fn undo_appends_an_inverse_rather_than_shortening_the_log() {
        // Undo is part of the history: it survives a restart, it synchronises,
        // and it can itself be undone.
        let mut log = OperationLog::new();
        commit(&mut log, 1, place(1, 0));
        commit(&mut log, 1, move_to(1, 200_000));
        let before_undo = log.len();

        let undo = log.undo_for(device(1), 0);
        assert!(undo.is_possible());
        let Some(undo) = undo.operations().first().cloned() else {
            return;
        };
        assert!(log.append(undo).is_ok());

        assert_eq!(
            log.len(),
            before_undo + 1,
            "the log grew, it did not shrink"
        );
        assert_eq!(
            log.state()
                .placements
                .get(&PlacementId::new(1))
                .map(|p| p.position),
            Some(Frames::new(0)),
            "the move was undone"
        );
    }

    #[test]
    fn undo_can_itself_be_undone() {
        let mut log = OperationLog::new();
        commit(&mut log, 1, place(1, 0));
        commit(&mut log, 1, move_to(1, 200_000));

        let Some(undo) = log.undo_for(device(1), 0).operations().first().cloned() else {
            unreachable!()
        };
        assert!(log.append(undo).is_ok());

        let Some(redo) = log.undo_for(device(1), 0).operations().first().cloned() else {
            unreachable!()
        };
        assert!(log.append(redo).is_ok());

        assert_eq!(
            log.state()
                .placements
                .get(&PlacementId::new(1))
                .map(|p| p.position),
            Some(Frames::new(200_000)),
            "undoing the undo restores the move"
        );
    }

    #[test]
    fn undo_is_per_device() {
        // In a shared project, undo means "undo my last edit". Undoing a
        // collaborator's work because they acted more recently would be the
        // least expected behaviour available.
        let mut log = OperationLog::new();
        commit(&mut log, 1, place(1, 0));
        commit(&mut log, 2, place(2, 48_000));

        let Some(undo) = log.undo_for(device(1), 0).operations().first().cloned() else {
            unreachable!()
        };
        assert!(log.append(undo).is_ok());

        let state = log.state();
        assert!(
            !state.placements.contains_key(&PlacementId::new(1)),
            "device 1's own edit was undone"
        );
        assert!(
            state.placements.contains_key(&PlacementId::new(2)),
            "device 2's edit was left alone"
        );
    }

    #[test]
    fn undo_with_nothing_to_undo_returns_nothing() {
        let log = OperationLog::new();
        assert_eq!(log.undo_for(device(1), 0), Undo::Nothing);

        let mut other = OperationLog::new();
        commit(&mut other, 2, place(1, 0));
        assert_eq!(
            other.undo_for(device(1), 0),
            Undo::Nothing,
            "a device with no edits has nothing to undo"
        );
    }

    #[test]
    fn markers_participate_in_the_same_mechanism() {
        let mut log = OperationLog::new();
        commit(
            &mut log,
            1,
            OperationPayload::AddMarker {
                marker: MarkerId::new(1),
                position: Frames::new(24_000),
                kind: MarkerKind::Drop,
                label: String::from("drop"),
            },
        );
        assert_eq!(log.state().markers.len(), 1);

        let Some(undo) = log.undo_for(device(1), 0).operations().first().cloned() else {
            unreachable!()
        };
        assert!(log.append(undo).is_ok());
        assert!(log.state().markers.is_empty());
    }

    #[test]
    fn authoring_does_not_append() {
        // An operation can be built, previewed and discarded — which is what an
        // AI proposal the user has not accepted needs to be.
        let mut log = OperationLog::new();
        let proposal = log.author(device(1), 0, place(1, 0));
        assert!(log.is_empty(), "authoring must not modify the log");
        assert!(log.append(proposal).is_ok());
        assert_eq!(log.len(), 1);
    }

    #[test]
    fn named_versions_are_bounded() {
        // A corrupted or hostile file must not be able to ask for unbounded
        // allocation. The operation bound works the same way; it is not
        // exercised here only because filling it would make the suite slow.
        let mut log = OperationLog::new();
        for index in 0..MAX_LABELS {
            assert!(
                log.label(&format!("v{index}"), 0).is_ok(),
                "label {index} should fit"
            );
        }
        assert_eq!(
            log.label("one too many", 0),
            Err(ProjectError::TooManyLabels)
        );
        // Renaming an existing version is still allowed at the bound, because
        // it does not grow the set.
        assert!(log.label("v0", 0).is_ok());
    }

    #[test]
    fn reports_read_as_sentences() {
        let report = MergeReport {
            applied: 3,
            already_present: 1,
            conflicts: Vec::new(),
        };
        assert_eq!(
            report.to_string(),
            "3 applied, 1 already present, 0 to review"
        );

        let conflict = Conflict {
            existing: OperationId::new(device(1), 2),
            incoming: OperationId::new(device(2), 1),
            target: Target::Placement(PlacementId::new(4)),
            kind: ConflictKind::ConcurrentEdit,
        };
        assert_eq!(
            conflict.to_string(),
            "device:1#2 and device:2#1 both changed placement:4 — both edited this at the same time"
        );
    }

    #[test]
    fn a_run_of_operations_gets_a_run_of_identities() {
        // `author` numbers from what the log has seen, so two calls before an
        // append produce the same identity. That was invisible while a gesture
        // was one operation; since `prv-timeline::Edit` began carrying a list,
        // the natural loop has been minting collisions.
        let log = OperationLog::new();
        let run = log.author_all(
            device(1),
            0,
            vec![place(1, 0), move_to(1, 48_000), move_to(1, 24_000)],
        );

        assert_eq!(run.len(), 3);
        let identities: Vec<u64> = run.iter().map(|operation| operation.id.sequence).collect();
        assert_eq!(identities, vec![1, 2, 3], "the run reused an identity");

        let mut log = log;
        for operation in run {
            assert!(
                log.append(operation).is_ok(),
                "a run authored together must append together"
            );
        }
    }

    #[test]
    fn undoing_a_removal_restores_where_the_clip_began_in_its_source() {
        // `PlaceTrack` cannot carry a source offset — ADR-0003 forbids
        // redefining it — so restoring a trimmed clip takes two operations.
        // With one, the audio under it slid back to the start of the file on
        // undo, silently.
        let mut log = OperationLog::new();
        commit(&mut log, 1, place(1, 0));
        commit(
            &mut log,
            1,
            OperationPayload::SetPlacementSource {
                placement: PlacementId::new(1),
                source_offset: Frames::new(96_000),
            },
        );
        commit(
            &mut log,
            1,
            OperationPayload::RemovePlacement {
                placement: PlacementId::new(1),
            },
        );
        assert!(log.state().placements.is_empty());

        let undo = log.undo_for(device(1), 0);
        assert!(undo.is_possible());
        assert_eq!(
            undo.operations().len(),
            2,
            "restoring a trimmed clip takes a placement and its source offset"
        );
        for operation in undo.operations().to_vec() {
            assert!(log.append(operation).is_ok());
        }

        let restored = log
            .state()
            .placements
            .get(&PlacementId::new(1))
            .copied()
            .map(|placement| placement.source_offset);
        assert_eq!(
            restored,
            Some(Frames::new(96_000)),
            "the audio under the restored clip slid back to the start of the file"
        );
    }

    #[test]
    fn an_undo_that_would_revert_somebody_elses_later_edit_is_refused() {
        // Master Prompt #24 forbids discarding work silently. The inverse of my
        // move is computed against the state before it, so performing it would
        // put the clip back where it was before either of us touched it.
        let mut log = OperationLog::new();
        commit(&mut log, 1, place(1, 0));
        commit(&mut log, 1, move_to(1, 200_000));
        commit(&mut log, 2, move_to(1, 300_000));

        assert_eq!(
            log.undo_for(device(1), 0),
            Undo::Superseded { by: device(2) },
            "my undo silently reverted somebody else's later move"
        );

        // And the other device, whose edit *is* the latest, can still undo.
        assert!(log.undo_for(device(2), 0).is_possible());
    }

    #[test]
    fn an_edit_to_something_else_does_not_block_an_undo() {
        // The refusal is about the same target, not about anybody having
        // touched the project since. A collaborator working on another clip
        // must not freeze my undo.
        let mut log = OperationLog::new();
        commit(&mut log, 1, place(1, 0));
        commit(&mut log, 1, move_to(1, 200_000));
        commit(&mut log, 2, place(2, 0));

        assert!(
            log.undo_for(device(1), 0).is_possible(),
            "an unrelated edit blocked an undo"
        );
    }

    #[test]
    fn a_branch_never_reuses_a_name_the_trunk_already_gave_out() {
        // Identity is a device and a sequence, which is collision-free across
        // devices and was not across branches of one. A branch taken before the
        // trunk's second edit re-allocated that edit's number, and merging back
        // read as "already present" — the branch's work disappearing without a
        // word, which is exactly what Master Prompt #24 forbids.
        let mut trunk = OperationLog::new();
        commit(&mut trunk, 1, place(1, 0));
        commit(&mut trunk, 1, place(2, 48_000));

        let mut branch = trunk.branch_at(1).expect("a valid position");
        commit(&mut branch, 1, place(9, 200_000));

        let branched = branch
            .operations()
            .last()
            .map(|operation| operation.id)
            .expect("the branch has an operation");
        assert!(
            !trunk.contains(branched),
            "the branch reused {branched}, which the trunk had already given out"
        );

        let incoming: Vec<Operation> = branch
            .operations()
            .filter(|operation| !trunk.contains(operation.id))
            .cloned()
            .collect();
        let report = trunk.merge(&incoming).expect("a mergeable branch");
        assert_eq!(report.applied, 1, "the branch's work was not applied");
        assert!(!report.needs_review());
        assert!(trunk.state().placements.contains_key(&PlacementId::new(9)));
    }

    #[test]
    fn two_different_edits_under_one_name_are_reported_rather_than_dropped() {
        // The case the high-water mark cannot remove: two independent branches
        // of one device. Treating it as "already present" discarded the
        // incoming work silently. Reported, the person who made the edit is
        // told it could not be applied under that name.
        let mut log = OperationLog::new();
        commit(&mut log, 1, place(1, 0));

        let held = log
            .operations()
            .next()
            .cloned()
            .expect("the log has an operation");

        // Same identity, different work — what a second branch would produce.
        let impostor = Operation {
            id: held.id,
            context: held.context.clone(),
            timestamp_micros: held.timestamp_micros,
            payload: place(7, 96_000),
        };

        let report = log.merge(&[impostor]).expect("a merge");
        assert_eq!(report.applied, 0);
        assert_eq!(report.already_present, 0, "a collision read as a duplicate");
        assert_eq!(report.conflicts.len(), 1);
        assert_eq!(
            report.conflicts.first().map(|conflict| conflict.kind),
            Some(ConflictKind::SameNameDifferentWork)
        );
    }

    #[test]
    fn the_same_operation_delivered_twice_is_still_boring() {
        // The other half: a network that retries must not turn a duplicate into
        // a conflict, or every flaky connection would ask the user a question.
        let mut log = OperationLog::new();
        commit(&mut log, 1, place(1, 0));
        let held: Vec<Operation> = log.operations().cloned().collect();

        let report = log.merge(&held).expect("a merge");
        assert_eq!(report.already_present, 1);
        assert!(!report.needs_review());
    }
}

#[cfg(test)]
mod carrying {
    #![allow(
        clippy::expect_used,
        clippy::panic,
        reason = "a test that cannot build its own fixture should fail loudly"
    )]

    use super::*;
    use crate::operation::MarkerId;
    use crate::wire;

    fn device(id: u64) -> DeviceId {
        DeviceId::new(id)
    }

    fn remove_marker(id: u64) -> OperationPayload {
        OperationPayload::RemoveMarker {
            marker: MarkerId::new(id),
        }
    }

    /// A message from a build that does not exist yet, carrying `count`
    /// operations this one cannot read.
    ///
    /// Built by hand because the encoder cannot produce it — which is the whole
    /// reason the decoder has to be tested against it.
    fn from_a_newer_build(author: u64, count: u64) -> Vec<CarriedOperation> {
        (1..=count)
            .map(|sequence| {
                let mut body = Vec::new();
                body.extend_from_slice(&author.to_le_bytes());
                body.extend_from_slice(&sequence.to_le_bytes());
                body.extend_from_slice(&0_i64.to_le_bytes());
                body.extend_from_slice(&0_u32.to_le_bytes());
                body.extend_from_slice(&60_000_u16.to_le_bytes());
                body.extend_from_slice(&0_u32.to_le_bytes());
                CarriedOperation::new(
                    OperationId::new(device(author), sequence),
                    VersionVector::new(),
                    0,
                    60_000,
                    body,
                )
            })
            .collect()
    }

    #[test]
    fn a_relay_still_relays_after_being_closed_and_reopened() {
        // The reason carrying belongs to the document rather than to the
        // synchronisation layer. A device that held these in memory alone would
        // carry a colleague's work until it quit, and then quietly stop — which
        // is the same as losing it, later and less visibly.
        let mut relay = OperationLog::new();
        relay.carry(&from_a_newer_build(3, 4)).expect("carries");
        assert_eq!(relay.carried_count(), 4);

        // Closed and reopened. A project is its log, so this is what reopening
        // one is: the same value, arrived at again.
        let reopened = relay.clone();
        assert_eq!(reopened.carried_count(), 4);

        let asking = VersionVector::new();
        assert_eq!(reopened.carried_since(&asking).len(), 4);
    }

    #[test]
    fn carrying_records_it_as_seen_so_peers_stop_resending_it() {
        // Only honest because the bytes are kept. A device that claimed to have
        // seen an operation it did not hold would end the relay at itself while
        // every peer believed the work had arrived.
        let mut relay = OperationLog::new();
        relay.carry(&from_a_newer_build(3, 2)).expect("carries");

        let seen = relay.version_vector();
        assert!(seen.has_seen(OperationId::new(device(3), 2)));
        assert_eq!(relay.carried_since(seen).len(), 0);
    }

    #[test]
    fn the_same_operation_carried_twice_is_held_once() {
        let mut relay = OperationLog::new();
        let batch = from_a_newer_build(3, 3);

        assert_eq!(relay.carry(&batch).expect("carries"), 3);
        assert_eq!(relay.carry(&batch).expect("carries"), 0);
        assert_eq!(relay.carried_count(), 3);
    }

    #[test]
    fn carrying_is_bounded_and_refuses_rather_than_discarding() {
        // A build claiming to be newer could otherwise fill a device with
        // operations it will never interpret. Refusing keeps the rule the
        // outbox keeps: this is somebody's work, not a record of it.
        let mut relay = OperationLog::new();
        let sequences = u64::try_from(MAX_CARRIED).expect("fits");
        relay
            .carry(&from_a_newer_build(3, sequences))
            .expect("carries");
        assert_eq!(relay.carried_count(), MAX_CARRIED);

        let one_more = from_a_newer_build(4, 1);
        assert_eq!(relay.carry(&one_more), Err(ProjectError::CarriedFull));
        assert_eq!(relay.carried_count(), MAX_CARRIED, "a refusal took some");
    }

    #[test]
    fn an_upgrade_turns_carried_work_into_the_project_it_always_was() {
        // Simulated the only way one build can: by building the state an
        // upgrade leaves behind — entries this build can read, sitting in the
        // carried store because the build that put them there could not.
        let mut author = OperationLog::new();
        let operations: Vec<Operation> = (1..=3)
            .map(|index| {
                let operation = author.author(device(1), 1_000, remove_marker(index));
                author.append(operation.clone()).expect("appends");
                operation
            })
            .collect();

        let bytes = wire::encode(&operations).expect("encodes");
        let carried: Vec<CarriedOperation> = wire::decode(&bytes)
            .expect("decodes")
            .entries()
            .iter()
            .filter_map(|entry| match entry {
                crate::wire::Entry::Understood(operation) => Some(CarriedOperation::new(
                    operation.id,
                    operation.context.clone(),
                    operation.timestamp_micros,
                    60_000,
                    wire::entry_body(operation).expect("encodes"),
                )),
                crate::wire::Entry::Unrecognised(_) => None,
            })
            .collect();

        let mut upgraded = OperationLog::new();
        upgraded.carry(&carried).expect("carries");
        assert_eq!(upgraded.len(), 0);
        assert_eq!(upgraded.carried_count(), 3);

        assert_eq!(upgraded.promote_carried(), 3);
        assert_eq!(upgraded.carried_count(), 0);
        assert_eq!(upgraded.len(), 3);
        assert_eq!(upgraded.state(), author.state());

        // And running it again finds nothing to do, which is what makes it safe
        // to run every time a project is opened.
        assert_eq!(upgraded.promote_carried(), 0);
    }

    #[test]
    fn an_operation_still_beyond_this_build_survives_a_promotion_attempt() {
        let mut relay = OperationLog::new();
        relay.carry(&from_a_newer_build(3, 2)).expect("carries");

        assert_eq!(relay.promote_carried(), 0);
        assert_eq!(relay.carried_count(), 2, "an operation was lost to a retry");
    }

    #[test]
    fn a_promoted_operation_is_not_taken_in_again() {
        // The same operation may arrive twice: once before an upgrade and once
        // after. `holds` is what stops it being stored in both halves.
        let mut author = OperationLog::new();
        let operation = author.author(device(1), 1_000, remove_marker(1));
        author.append(operation.clone()).expect("appends");

        let carried = CarriedOperation::new(
            operation.id,
            operation.context.clone(),
            operation.timestamp_micros,
            60_000,
            wire::entry_body(&operation).expect("encodes"),
        );

        let mut upgraded = OperationLog::new();
        upgraded
            .carry(core::slice::from_ref(&carried))
            .expect("carries");
        assert_eq!(upgraded.promote_carried(), 1);

        assert_eq!(
            upgraded
                .carry(core::slice::from_ref(&carried))
                .expect("carries"),
            0
        );
        assert_eq!(upgraded.carried_count(), 0);
        assert_eq!(upgraded.len(), 1);
    }

    #[test]
    fn a_stale_device_in_the_middle_delivers_work_it_cannot_read() {
        // Three devices, the middle one a version behind. What it cannot apply
        // it still carries, so the third receives the first's work intact —
        // which is the difference between a stale install being inconvenient
        // and being a hole in the fleet.
        let mut relay = OperationLog::new();
        let mine = relay.author(device(2), 1_000, remove_marker(9));
        relay.append(mine).expect("appends");
        relay.carry(&from_a_newer_build(3, 5)).expect("carries");

        let asking = VersionVector::new();
        let message = wire::encode_all(
            &relay.operations_since(&asking),
            &relay.carried_since(&asking),
        )
        .expect("encodes");

        let arrived = wire::decode(&message).expect("decodes");
        assert_eq!(arrived.understood().count(), 1);
        assert_eq!(arrived.unrecognised_count(), 5);

        // And the third device holds everything the first sent, byte for byte.
        let mut destination = OperationLog::new();
        destination.merge(&arrived.to_operations()).expect("merges");
        destination.carry(&arrived.to_carried()).expect("carries");
        assert_eq!(destination.len(), 1);
        assert_eq!(destination.carried_count(), 5);
        assert_eq!(
            destination.carried().next().map(CarriedOperation::bytes),
            relay.carried().next().map(CarriedOperation::bytes)
        );
    }
}
