use core::fmt;
use std::collections::BTreeMap;

use prv_time::Frames;

/// A device that can author operations.
///
/// Stable for the life of an installation. Identity is what makes an operation
/// identifier unique without a central authority to hand out numbers — which
/// matters because Master Prompt #24 requires editing to work with no network.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DeviceId(u64);

impl DeviceId {
    /// Creates a device identifier.
    #[must_use]
    pub const fn new(id: u64) -> Self {
        Self(id)
    }

    /// The raw value.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl fmt::Display for DeviceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "device:{}", self.0)
    }
}

/// Identifies one operation, uniquely and without coordination.
///
/// A device plus a sequence number it allocates itself. Two devices editing the
/// same project offline cannot collide, because neither number is shared.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OperationId {
    /// Device that authored the operation.
    pub device: DeviceId,
    /// The device's own counter, starting at one.
    pub sequence: u64,
}

impl OperationId {
    /// Creates an identifier.
    #[must_use]
    pub const fn new(device: DeviceId, sequence: u64) -> Self {
        Self { device, sequence }
    }
}

impl fmt::Display for OperationId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}#{}", self.device, self.sequence)
    }
}

/// What a device had seen when it authored an operation.
///
/// One counter per device: the highest sequence number from that device the
/// author had already applied.
///
/// # Why not a single Lamport counter
///
/// A Lamport timestamp gives a total order, which is enough to make every device
/// agree on the final state. It is not enough to tell whether two edits were
/// made *in sequence* or *in parallel*, because a smaller timestamp does not
/// prove the author had seen the larger one.
///
/// That distinction is the whole of conflict detection. Two people editing
/// different parts of a set should merge silently; two people editing the same
/// transition should be asked. Master Prompt #24 requires conflicts to be
/// explained and forbids silently discarding work, so guessing is not available.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VersionVector {
    seen: BTreeMap<DeviceId, u64>,
}

impl VersionVector {
    /// An empty vector: nothing seen.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The highest sequence seen from a device.
    #[must_use]
    pub fn sequence_for(&self, device: DeviceId) -> u64 {
        self.seen.get(&device).copied().unwrap_or(0)
    }

    /// Whether this vector has seen an operation.
    #[must_use]
    pub fn has_seen(&self, id: OperationId) -> bool {
        self.sequence_for(id.device) >= id.sequence
    }

    /// Records an operation as seen.
    pub fn observe(&mut self, id: OperationId) {
        let entry = self.seen.entry(id.device).or_insert(0);
        if id.sequence > *entry {
            *entry = id.sequence;
        }
    }

    /// Absorbs everything another vector has seen.
    pub fn merge_from(&mut self, other: &Self) {
        for (device, sequence) in &other.seen {
            let entry = self.seen.entry(*device).or_insert(0);
            if *sequence > *entry {
                *entry = *sequence;
            }
        }
    }

    /// The logical time an operation authored against this vector would carry.
    ///
    /// One more than the highest sequence seen from any device. Consistent with
    /// causality: an operation that saw another always has a strictly greater
    /// logical time, which is what lets the total order below respect
    /// happens-before while remaining computable from the operation alone.
    #[must_use]
    pub fn logical_time(&self) -> u64 {
        self.seen
            .values()
            .copied()
            .max()
            .unwrap_or(0)
            .saturating_add(1)
    }

    /// Number of devices this vector knows about.
    #[must_use]
    pub fn device_count(&self) -> usize {
        self.seen.len()
    }
}

/// A reference to a track in the library.
///
/// The project refers to media; it never contains it. Master Prompt #29 makes
/// the user the owner of their files, and ADR-0003 makes sharing a project share
/// the document rather than the audio — both of which require the reference to
/// be an identity rather than a path or a copy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TrackRef(u64);

impl TrackRef {
    /// Creates a reference.
    #[must_use]
    pub const fn new(id: u64) -> Self {
        Self(id)
    }

    /// The raw value.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl fmt::Display for TrackRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "track:{}", self.0)
    }
}

/// Identifies one placement of a track on the timeline.
///
/// Distinct from the track it refers to: the same track may appear several
/// times in a set, and each appearance is edited independently.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PlacementId(u64);

impl PlacementId {
    /// Creates an identifier.
    #[must_use]
    pub const fn new(id: u64) -> Self {
        Self(id)
    }

    /// The raw value.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl fmt::Display for PlacementId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "placement:{}", self.0)
    }
}

/// Identifies one marker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MarkerId(u64);

impl MarkerId {
    /// Creates an identifier.
    #[must_use]
    pub const fn new(id: u64) -> Self {
        Self(id)
    }

    /// The raw value.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl fmt::Display for MarkerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "marker:{}", self.0)
    }
}

/// What a marker means.
///
/// A subset of the section types Master Prompt #3B requires. The list grows by
/// addition; existing variants are never redefined, because a project written by
/// an earlier build must keep its meaning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum MarkerKind {
    /// A cue point.
    Cue,
    /// The start of a build-up.
    BuildUp,
    /// A drop.
    Drop,
    /// A breakdown.
    Breakdown,
    /// A point the user marked for their own reasons.
    Note,
}

impl fmt::Display for MarkerKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Cue => "cue",
            Self::BuildUp => "build-up",
            Self::Drop => "drop",
            Self::Breakdown => "breakdown",
            Self::Note => "note",
        })
    }
}

/// What an operation does.
///
/// # How this grows
///
/// New capability adds a variant. An existing variant is never redefined and
/// never removed, because a project written by an earlier build must keep its
/// meaning — ADR-0003 states that rule and this enum is where it is kept. The
/// type is `non_exhaustive` so that adding a variant is not a breaking change
/// for anything that matches on it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum OperationPayload {
    /// Names or renames the project.
    SetProjectName {
        /// The new name.
        name: String,
    },
    /// Places a track on the timeline.
    PlaceTrack {
        /// Identity of this placement.
        placement: PlacementId,
        /// The library track it refers to.
        track: TrackRef,
        /// Where it starts.
        position: Frames,
        /// How long it plays for.
        length: Frames,
        /// Which lane it sits on.
        lane: u32,
    },
    /// Moves an existing placement.
    MovePlacement {
        /// The placement to move.
        placement: PlacementId,
        /// Its new start.
        position: Frames,
        /// Its new lane.
        lane: u32,
    },
    /// Changes how long a placement plays for.
    TrimPlacement {
        /// The placement to trim.
        placement: PlacementId,
        /// Its new length.
        length: Frames,
    },
    /// Removes a placement.
    RemovePlacement {
        /// The placement to remove.
        placement: PlacementId,
    },
    /// Adds a marker.
    AddMarker {
        /// Identity of the marker.
        marker: MarkerId,
        /// Where it sits.
        position: Frames,
        /// What it means.
        kind: MarkerKind,
        /// Its label.
        label: String,
    },
    /// Removes a marker.
    RemoveMarker {
        /// The marker to remove.
        marker: MarkerId,
    },
}

impl OperationPayload {
    /// The entity this operation affects, if it affects exactly one.
    ///
    /// Conflict detection needs to know what two concurrent operations touched.
    /// An operation with no single target — renaming the project — affects the
    /// project as a whole and is reported as such.
    #[must_use]
    pub fn target(&self) -> Target {
        match self {
            Self::SetProjectName { .. } => Target::Project,
            Self::PlaceTrack { placement, .. }
            | Self::MovePlacement { placement, .. }
            | Self::TrimPlacement { placement, .. }
            | Self::RemovePlacement { placement } => Target::Placement(*placement),
            Self::AddMarker { marker, .. } | Self::RemoveMarker { marker } => {
                Target::Marker(*marker)
            }
        }
    }

    /// Whether two operations on the same target commute.
    ///
    /// Two operations commute when applying them in either order gives the same
    /// result. Two moves of the same placement do not: the last one wins, and
    /// which is last is arbitrary when they were made concurrently. Two
    /// *removals* of the same placement do: the placement ends up gone either
    /// way, and neither user is surprised.
    ///
    /// Getting this right is what decides whether a user is interrupted. Being
    /// too eager reports conflicts nobody cares about and trains people to
    /// dismiss the dialogue; being too lax silently discards an intention, which
    /// Master Prompt #24 forbids outright.
    #[must_use]
    pub fn commutes_with(&self, other: &Self) -> bool {
        if self.target() != other.target() {
            // Different entities never interfere.
            return true;
        }
        match (self, other) {
            // Removal absorbs everything else on the same entity, including
            // another removal. The entity ends up gone whichever order the two
            // arrive in, so there is nothing for a user to decide — and asking
            // them to adjudicate a move against a delete would waste the
            // attention that a real conflict needs.
            (Self::RemovePlacement { .. } | Self::RemoveMarker { .. }, _)
            | (_, Self::RemovePlacement { .. } | Self::RemoveMarker { .. }) => true,
            // Two edits to the same property of the same entity genuinely
            // disagree: whichever the total order puts last would silently win.
            _ => false,
        }
    }
}

/// What an operation affects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Target {
    /// The project as a whole.
    Project,
    /// One placement.
    Placement(PlacementId),
    /// One marker.
    Marker(MarkerId),
}

impl fmt::Display for Target {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Project => f.write_str("the project"),
            Self::Placement(id) => write!(f, "{id}"),
            Self::Marker(id) => write!(f, "{id}"),
        }
    }
}

/// One entry in the log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Operation {
    /// Unique identity.
    pub id: OperationId,
    /// What the author had seen when this was made.
    pub context: VersionVector,
    /// Wall-clock time, in microseconds since the epoch.
    ///
    /// Informational only. Never used for ordering: clocks disagree between
    /// devices, and a project whose history depended on them would reorder
    /// itself when someone's clock was wrong.
    pub timestamp_micros: i64,
    /// What it does.
    pub payload: OperationPayload,
}

impl Operation {
    /// The logical time used for ordering.
    #[must_use]
    pub fn logical_time(&self) -> u64 {
        self.context.logical_time()
    }

    /// The deterministic sort key.
    ///
    /// Logical time first, so causality is respected; then device and sequence,
    /// so that concurrent operations get a stable order every device computes
    /// identically. Without the tiebreak, two devices could fold the same set of
    /// operations into different states.
    #[must_use]
    pub fn sort_key(&self) -> (u64, u64, u64) {
        (self.logical_time(), self.id.device.get(), self.id.sequence)
    }

    /// Whether this operation was made without having seen `other`, and vice
    /// versa.
    #[must_use]
    pub fn is_concurrent_with(&self, other: &Self) -> bool {
        !self.context.has_seen(other.id) && !other.context.has_seen(self.id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(id: u64) -> DeviceId {
        DeviceId::new(id)
    }

    fn operation(device_id: u64, sequence: u64, context: VersionVector) -> Operation {
        Operation {
            id: OperationId::new(device(device_id), sequence),
            context,
            timestamp_micros: 0,
            payload: OperationPayload::SetProjectName {
                name: String::from("test"),
            },
        }
    }

    #[test]
    fn a_fresh_vector_has_seen_nothing() {
        let vector = VersionVector::new();
        assert_eq!(vector.sequence_for(device(1)), 0);
        assert!(!vector.has_seen(OperationId::new(device(1), 1)));
        assert_eq!(vector.logical_time(), 1);
    }

    #[test]
    fn observing_records_the_highest_sequence() {
        let mut vector = VersionVector::new();
        vector.observe(OperationId::new(device(1), 5));
        vector.observe(OperationId::new(device(1), 3));
        assert_eq!(
            vector.sequence_for(device(1)),
            5,
            "an older operation must not lower the watermark"
        );
        assert!(vector.has_seen(OperationId::new(device(1), 4)));
        assert!(!vector.has_seen(OperationId::new(device(1), 6)));
    }

    #[test]
    fn merging_takes_the_highest_from_each_device() {
        let mut left = VersionVector::new();
        left.observe(OperationId::new(device(1), 5));
        left.observe(OperationId::new(device(2), 2));

        let mut right = VersionVector::new();
        right.observe(OperationId::new(device(1), 3));
        right.observe(OperationId::new(device(3), 7));

        left.merge_from(&right);
        assert_eq!(left.sequence_for(device(1)), 5);
        assert_eq!(left.sequence_for(device(2)), 2);
        assert_eq!(left.sequence_for(device(3)), 7);
        assert_eq!(left.device_count(), 3);
    }

    #[test]
    fn logical_time_respects_causality() {
        // An operation that saw another must sort after it. This is what makes
        // the total order consistent with happens-before.
        let first = operation(1, 1, VersionVector::new());

        let mut seen = VersionVector::new();
        seen.observe(first.id);
        let second = operation(2, 1, seen);

        assert!(
            second.logical_time() > first.logical_time(),
            "an operation that saw another must sort after it"
        );
    }

    #[test]
    fn concurrency_is_detected_rather_than_guessed() {
        // Two devices editing offline. Neither saw the other, so both are
        // concurrent — a fact the version vectors record exactly.
        let left = operation(1, 1, VersionVector::new());
        let right = operation(2, 1, VersionVector::new());
        assert!(left.is_concurrent_with(right_ref(&right)));
        assert!(right.is_concurrent_with(&left));

        // Now one device syncs and edits again. That edit is not concurrent.
        let mut after_sync = VersionVector::new();
        after_sync.observe(left.id);
        after_sync.observe(right.id);
        let later = operation(2, 2, after_sync);
        assert!(!later.is_concurrent_with(&left));
        assert!(!left.is_concurrent_with(&later));
    }

    /// Borrow helper, so the assertion above reads symmetrically.
    fn right_ref(operation: &Operation) -> &Operation {
        operation
    }

    #[test]
    fn the_sort_key_breaks_ties_deterministically() {
        // Two concurrent operations have the same logical time. Without a
        // tiebreak, two devices could fold the same set into different states.
        let left = operation(1, 1, VersionVector::new());
        let right = operation(2, 1, VersionVector::new());
        assert_eq!(left.logical_time(), right.logical_time());
        assert!(left.sort_key() < right.sort_key());
    }

    #[test]
    fn wall_clock_time_is_not_used_for_ordering() {
        // A device with a wrong clock must not reorder the project.
        let mut early = operation(1, 1, VersionVector::new());
        early.timestamp_micros = 1_000_000;
        let mut late = operation(2, 1, VersionVector::new());
        late.timestamp_micros = -5_000_000;

        assert!(
            early.sort_key() < late.sort_key(),
            "order must follow logical time and device, not the wall clock"
        );
    }

    #[test]
    fn operations_on_different_entities_always_commute() {
        let move_one = OperationPayload::MovePlacement {
            placement: PlacementId::new(1),
            position: Frames::new(0),
            lane: 0,
        };
        let move_two = OperationPayload::MovePlacement {
            placement: PlacementId::new(2),
            position: Frames::new(100),
            lane: 1,
        };
        assert!(move_one.commutes_with(&move_two));
    }

    #[test]
    fn two_moves_of_the_same_placement_do_not_commute() {
        // The case that must reach the user: both people meant something, and
        // whichever the total order puts last would silently win.
        let left = OperationPayload::MovePlacement {
            placement: PlacementId::new(1),
            position: Frames::new(0),
            lane: 0,
        };
        let right = OperationPayload::MovePlacement {
            placement: PlacementId::new(1),
            position: Frames::new(48_000),
            lane: 0,
        };
        assert!(!left.commutes_with(&right));
    }

    #[test]
    fn removal_is_idempotent_and_absorbs_edits() {
        let remove = OperationPayload::RemovePlacement {
            placement: PlacementId::new(1),
        };
        let remove_again = OperationPayload::RemovePlacement {
            placement: PlacementId::new(1),
        };
        let moved = OperationPayload::MovePlacement {
            placement: PlacementId::new(1),
            position: Frames::new(100),
            lane: 0,
        };

        assert!(remove.commutes_with(&remove_again));
        assert!(
            remove.commutes_with(&moved),
            "asking a user to adjudicate a move against a delete wastes their attention"
        );
        assert!(moved.commutes_with(&remove));
    }

    #[test]
    fn targets_identify_what_an_operation_touches() {
        assert_eq!(
            OperationPayload::SetProjectName {
                name: String::new()
            }
            .target(),
            Target::Project
        );
        assert_eq!(
            OperationPayload::RemoveMarker {
                marker: MarkerId::new(3)
            }
            .target(),
            Target::Marker(MarkerId::new(3))
        );
    }

    #[test]
    fn identifiers_display_readably() {
        assert_eq!(OperationId::new(device(7), 3).to_string(), "device:7#3");
        assert_eq!(PlacementId::new(2).to_string(), "placement:2");
        assert_eq!(MarkerKind::Drop.to_string(), "drop");
        assert_eq!(Target::Project.to_string(), "the project");
    }
}
