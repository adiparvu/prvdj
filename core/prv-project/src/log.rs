use core::fmt;
use std::collections::BTreeMap;

use crate::operation::{DeviceId, Operation, OperationId, OperationPayload, Target, VersionVector};
use crate::state::ProjectState;

/// The largest number of operations a single log will hold.
///
/// A million edits is far beyond any real project — an intensively edited
/// two-hour set is a few tens of thousands. The bound exists so that a corrupted
/// or hostile file cannot ask for an unbounded allocation.
const MAX_OPERATIONS: usize = 1_000_000;

/// The largest number of named versions.
const MAX_LABELS: usize = 10_000;

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
}

impl fmt::Display for ConflictKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ConcurrentEdit => f.write_str("both edited this at the same time"),
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
        let sequence = self.seen.sequence_for(device).saturating_add(1);
        Operation {
            id: OperationId::new(device, sequence),
            context: self.seen.clone(),
            timestamp_micros,
            payload,
        }
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
            if self.contains(operation.id) {
                report.already_present += 1;
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

    /// Authors the operation that undoes the last one a device made.
    ///
    /// Returns `None` when the device has nothing left to undo, or when what it
    /// did has no inverse — undoing the removal of something that was never
    /// there.
    ///
    /// # Why by device
    ///
    /// In a shared project, undo means "undo *my* last edit", not "undo whatever
    /// happened last". Undoing a collaborator's work because they happened to
    /// act more recently would be the least expected behaviour available.
    #[must_use]
    pub fn undo_for(&self, device: DeviceId, timestamp_micros: i64) -> Option<Operation> {
        let position = self
            .operations
            .iter()
            .rposition(|operation| operation.id.device == device)?;
        let operation = self.operations.get(position)?;
        // The state as it was immediately before that operation is what the
        // inverse must be computed against.
        let before = self.materialise(position);
        let inverse = before.inverse_of(&operation.payload)?;
        Some(self.author(device, timestamp_micros, inverse))
    }
}

#[cfg(test)]
mod tests {
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
        assert!(undo.is_some());
        let Some(undo) = undo else { return };
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

        let Some(undo) = log.undo_for(device(1), 0) else {
            unreachable!()
        };
        assert!(log.append(undo).is_ok());

        let Some(redo) = log.undo_for(device(1), 0) else {
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

        let Some(undo) = log.undo_for(device(1), 0) else {
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
        assert!(log.undo_for(device(1), 0).is_none());

        let mut other = OperationLog::new();
        commit(&mut other, 2, place(1, 0));
        assert!(
            other.undo_for(device(1), 0).is_none(),
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

        let Some(undo) = log.undo_for(device(1), 0) else {
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
}
