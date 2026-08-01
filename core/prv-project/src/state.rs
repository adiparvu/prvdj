use core::fmt;
use std::collections::BTreeMap;

use prv_time::Frames;

use crate::operation::{MarkerId, MarkerKind, OperationPayload, PlacementId, TrackRef};

/// One appearance of a track on the timeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placement {
    /// The library track this refers to.
    pub track: TrackRef,
    /// Where it starts.
    pub position: Frames,
    /// How long it plays for.
    pub length: Frames,
    /// Which lane it sits on.
    pub lane: u32,
}

impl Placement {
    /// One past the last frame this placement covers.
    #[must_use]
    pub fn end(self) -> Frames {
        self.position + self.length
    }

    /// Whether this placement overlaps another on the same lane.
    ///
    /// Overlap is not forbidden — a transition *is* two tracks overlapping — but
    /// the timeline needs to know about it to draw and to schedule correctly.
    #[must_use]
    pub fn overlaps(self, other: Self) -> bool {
        self.lane == other.lane && self.position < other.end() && other.position < self.end()
    }
}

/// A labelled point on the timeline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Marker {
    /// Where it sits.
    pub position: Frames,
    /// What it means.
    pub kind: MarkerKind,
    /// Its label.
    pub label: String,
}

/// The project, materialised.
///
/// # Why the collections are ordered
///
/// `BTreeMap` rather than `HashMap`, deliberately. Iteration order is part of
/// the result: an export, a rendered graph and a comparison between two versions
/// must all be reproducible, and a hash map's order varies between runs. ADR-0006
/// requires generation to be byte-identical across runs and platforms, and that
/// guarantee cannot survive an unordered collection anywhere in the fold.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProjectState {
    /// The project's name.
    pub name: String,
    /// Every placement, by identity.
    pub placements: BTreeMap<PlacementId, Placement>,
    /// Every marker, by identity.
    pub markers: BTreeMap<MarkerId, Marker>,
}

impl ProjectState {
    /// An empty project.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Applies one operation.
    ///
    /// The match below is exhaustive on purpose. `OperationPayload` is
    /// `non_exhaustive` for other crates, but within this one every variant must
    /// be handled: adding an operation type and forgetting to fold it would
    /// produce a project that silently ignored an edit, and a compiler error is
    /// a far better outcome than a lost edit.
    ///
    /// # Why an operation on something absent is not an error
    ///
    /// Moving a placement that has been removed does nothing. That is not a
    /// failure to report; it is the ordinary outcome of two people editing at
    /// once, and the removal is reported as the conflict if it is one. Treating
    /// it as an error would make the fold fail on a perfectly valid merged log,
    /// which would leave a user unable to open their own project.
    pub fn apply(&mut self, payload: &OperationPayload) {
        match payload {
            OperationPayload::SetProjectName { name } => {
                self.name.clear();
                self.name.push_str(name);
            }
            OperationPayload::PlaceTrack {
                placement,
                track,
                position,
                length,
                lane,
            } => {
                self.placements.insert(
                    *placement,
                    Placement {
                        track: *track,
                        position: *position,
                        length: *length,
                        lane: *lane,
                    },
                );
            }
            OperationPayload::MovePlacement {
                placement,
                position,
                lane,
            } => {
                if let Some(existing) = self.placements.get_mut(placement) {
                    existing.position = *position;
                    existing.lane = *lane;
                }
            }
            OperationPayload::TrimPlacement { placement, length } => {
                if let Some(existing) = self.placements.get_mut(placement) {
                    existing.length = *length;
                }
            }
            OperationPayload::RemovePlacement { placement } => {
                self.placements.remove(placement);
            }
            OperationPayload::AddMarker {
                marker,
                position,
                kind,
                label,
            } => {
                self.markers.insert(
                    *marker,
                    Marker {
                        position: *position,
                        kind: *kind,
                        label: label.clone(),
                    },
                );
            }
            OperationPayload::RemoveMarker { marker } => {
                self.markers.remove(marker);
            }
        }
    }

    /// The operation that would undo `payload`, given this state as it was
    /// *before* the operation was applied.
    ///
    /// # Why undo is an operation rather than a rewind
    ///
    /// Appending the inverse rather than truncating the log means undo is itself
    /// part of the history: it survives a restart, it synchronises to other
    /// devices, and it can itself be undone. A log that shortened would lose all
    /// three, and would make "undo, then someone else's edit arrives" impossible
    /// to reason about.
    ///
    /// Returns `None` when there is nothing to undo — undoing the removal of
    /// something that was never there, for instance.
    #[must_use]
    pub fn inverse_of(&self, payload: &OperationPayload) -> Option<OperationPayload> {
        match payload {
            OperationPayload::SetProjectName { .. } => Some(OperationPayload::SetProjectName {
                name: self.name.clone(),
            }),
            OperationPayload::PlaceTrack { placement, .. } => {
                if self.placements.contains_key(placement) {
                    // The placement already existed, so this was a replacement;
                    // restore what was there.
                    let existing = self.placements.get(placement)?;
                    Some(OperationPayload::PlaceTrack {
                        placement: *placement,
                        track: existing.track,
                        position: existing.position,
                        length: existing.length,
                        lane: existing.lane,
                    })
                } else {
                    Some(OperationPayload::RemovePlacement {
                        placement: *placement,
                    })
                }
            }
            OperationPayload::MovePlacement { placement, .. } => {
                let existing = self.placements.get(placement)?;
                Some(OperationPayload::MovePlacement {
                    placement: *placement,
                    position: existing.position,
                    lane: existing.lane,
                })
            }
            OperationPayload::TrimPlacement { placement, .. } => {
                let existing = self.placements.get(placement)?;
                Some(OperationPayload::TrimPlacement {
                    placement: *placement,
                    length: existing.length,
                })
            }
            OperationPayload::RemovePlacement { placement } => {
                let existing = self.placements.get(placement)?;
                Some(OperationPayload::PlaceTrack {
                    placement: *placement,
                    track: existing.track,
                    position: existing.position,
                    length: existing.length,
                    lane: existing.lane,
                })
            }
            OperationPayload::AddMarker { marker, .. } => {
                if let Some(existing) = self.markers.get(marker) {
                    Some(OperationPayload::AddMarker {
                        marker: *marker,
                        position: existing.position,
                        kind: existing.kind,
                        label: existing.label.clone(),
                    })
                } else {
                    Some(OperationPayload::RemoveMarker { marker: *marker })
                }
            }
            OperationPayload::RemoveMarker { marker } => {
                let existing = self.markers.get(marker)?;
                Some(OperationPayload::AddMarker {
                    marker: *marker,
                    position: existing.position,
                    kind: existing.kind,
                    label: existing.label.clone(),
                })
            }
        }
    }

    /// Every pair of placements that overlap on the same lane.
    ///
    /// Overlap is legitimate — a transition is an overlap — so this reports
    /// rather than rejects. The timeline uses it to draw, and the scheduler to
    /// know how many voices a moment needs.
    #[must_use]
    pub fn overlapping_placements(&self) -> Vec<(PlacementId, PlacementId)> {
        let entries: Vec<(&PlacementId, &Placement)> = self.placements.iter().collect();
        let mut overlaps = Vec::new();
        for (index, (left_id, left)) in entries.iter().enumerate() {
            for (right_id, right) in entries.iter().skip(index + 1) {
                if left.overlaps(**right) {
                    overlaps.push((**left_id, **right_id));
                }
            }
        }
        overlaps
    }

    /// The last frame any placement covers.
    #[must_use]
    pub fn duration(&self) -> Frames {
        self.placements
            .values()
            .map(|placement| placement.end())
            .max()
            .unwrap_or(Frames::ZERO)
    }
}

impl fmt::Display for ProjectState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "\"{}\" — {} placements, {} markers",
            self.name,
            self.placements.len(),
            self.markers.len()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn place(id: u64, position: i64, length: i64, lane: u32) -> OperationPayload {
        OperationPayload::PlaceTrack {
            placement: PlacementId::new(id),
            track: TrackRef::new(id),
            position: Frames::new(position),
            length: Frames::new(length),
            lane,
        }
    }

    #[test]
    fn a_new_project_is_empty() {
        let state = ProjectState::new();
        assert!(state.name.is_empty());
        assert!(state.placements.is_empty());
        assert_eq!(state.duration(), Frames::ZERO);
    }

    #[test]
    fn placing_and_moving_a_track() {
        let mut state = ProjectState::new();
        state.apply(&place(1, 0, 48_000, 0));
        assert_eq!(state.placements.len(), 1);
        assert_eq!(state.duration(), Frames::new(48_000));

        state.apply(&OperationPayload::MovePlacement {
            placement: PlacementId::new(1),
            position: Frames::new(96_000),
            lane: 2,
        });
        let placement = state.placements.get(&PlacementId::new(1));
        assert_eq!(placement.map(|p| p.position), Some(Frames::new(96_000)));
        assert_eq!(placement.map(|p| p.lane), Some(2));
        assert_eq!(state.duration(), Frames::new(144_000));
    }

    #[test]
    fn an_operation_on_something_absent_does_nothing() {
        // The ordinary outcome of two people editing at once. Treating it as an
        // error would make a valid merged log fail to open.
        let mut state = ProjectState::new();
        state.apply(&OperationPayload::MovePlacement {
            placement: PlacementId::new(99),
            position: Frames::new(1_000),
            lane: 0,
        });
        state.apply(&OperationPayload::RemovePlacement {
            placement: PlacementId::new(99),
        });
        assert!(state.placements.is_empty());
    }

    #[test]
    fn undoing_a_placement_removes_it() {
        let state = ProjectState::new();
        let payload = place(1, 0, 48_000, 0);
        let inverse = state.inverse_of(&payload);
        assert_eq!(
            inverse,
            Some(OperationPayload::RemovePlacement {
                placement: PlacementId::new(1)
            })
        );
    }

    #[test]
    fn undoing_a_move_restores_the_previous_position() {
        let mut state = ProjectState::new();
        state.apply(&place(1, 1_000, 48_000, 3));

        let payload = OperationPayload::MovePlacement {
            placement: PlacementId::new(1),
            position: Frames::new(96_000),
            lane: 0,
        };
        let inverse = state.inverse_of(&payload);
        assert_eq!(
            inverse,
            Some(OperationPayload::MovePlacement {
                placement: PlacementId::new(1),
                position: Frames::new(1_000),
                lane: 3
            })
        );
    }

    #[test]
    fn undoing_a_removal_restores_everything_about_it() {
        // Not just its existence: its track, position, length and lane.
        let mut state = ProjectState::new();
        state.apply(&place(1, 5_000, 24_000, 2));

        let payload = OperationPayload::RemovePlacement {
            placement: PlacementId::new(1),
        };
        let inverse = state.inverse_of(&payload);
        assert_eq!(
            inverse,
            Some(OperationPayload::PlaceTrack {
                placement: PlacementId::new(1),
                track: TrackRef::new(1),
                position: Frames::new(5_000),
                length: Frames::new(24_000),
                lane: 2
            })
        );
    }

    #[test]
    fn applying_an_inverse_restores_the_earlier_state() {
        // The property that makes undo correct, checked end to end rather than
        // by inspecting the inverse.
        let mut state = ProjectState::new();
        state.apply(&place(1, 1_000, 48_000, 0));
        state.apply(&place(2, 60_000, 48_000, 1));
        let before = state.clone();

        let edit = OperationPayload::MovePlacement {
            placement: PlacementId::new(1),
            position: Frames::new(500_000),
            lane: 5,
        };
        let Some(inverse) = state.inverse_of(&edit) else {
            unreachable!("a move of an existing placement always has an inverse")
        };
        state.apply(&edit);
        assert_ne!(state, before);
        state.apply(&inverse);
        assert_eq!(state, before, "undo must restore the earlier state exactly");
    }

    #[test]
    fn undoing_something_that_never_existed_has_no_inverse() {
        let state = ProjectState::new();
        assert_eq!(
            state.inverse_of(&OperationPayload::RemovePlacement {
                placement: PlacementId::new(1)
            }),
            None
        );
        assert_eq!(
            state.inverse_of(&OperationPayload::MovePlacement {
                placement: PlacementId::new(1),
                position: Frames::ZERO,
                lane: 0
            }),
            None
        );
    }

    #[test]
    fn markers_round_trip_through_undo() {
        let mut state = ProjectState::new();
        let add = OperationPayload::AddMarker {
            marker: MarkerId::new(1),
            position: Frames::new(24_000),
            kind: MarkerKind::Drop,
            label: String::from("first drop"),
        };
        state.apply(&add);
        let before = state.clone();

        let remove = OperationPayload::RemoveMarker {
            marker: MarkerId::new(1),
        };
        let Some(inverse) = state.inverse_of(&remove) else {
            unreachable!()
        };
        state.apply(&remove);
        assert!(state.markers.is_empty());
        state.apply(&inverse);
        assert_eq!(state, before);
    }

    #[test]
    fn overlaps_are_reported_rather_than_rejected() {
        // A transition is an overlap. The timeline needs to know about it, not
        // be prevented from having it.
        let mut state = ProjectState::new();
        state.apply(&place(1, 0, 48_000, 0));
        state.apply(&place(2, 24_000, 48_000, 0));
        state.apply(&place(3, 0, 48_000, 1));

        let overlaps = state.overlapping_placements();
        assert_eq!(overlaps.len(), 1, "only the same-lane pair overlaps");
        assert_eq!(
            overlaps.first(),
            Some(&(PlacementId::new(1), PlacementId::new(2)))
        );
    }

    #[test]
    fn placements_touching_end_to_end_do_not_overlap() {
        let mut state = ProjectState::new();
        state.apply(&place(1, 0, 48_000, 0));
        state.apply(&place(2, 48_000, 48_000, 0));
        assert!(state.overlapping_placements().is_empty());
    }

    #[test]
    fn iteration_order_is_stable_across_runs() {
        // Reproducibility depends on it: an unordered collection anywhere in the
        // fold would break the guarantee that the same log produces the same
        // project.
        let mut first = ProjectState::new();
        let mut second = ProjectState::new();
        for id in [7_i64, 3, 9, 1, 5] {
            first.apply(&place(id.unsigned_abs(), id * 1_000, 1_000, 0));
        }
        for id in [1_i64, 3, 5, 7, 9] {
            second.apply(&place(id.unsigned_abs(), id * 1_000, 1_000, 0));
        }
        let first_order: Vec<PlacementId> = first.placements.keys().copied().collect();
        let second_order: Vec<PlacementId> = second.placements.keys().copied().collect();
        assert_eq!(first_order, second_order);
    }
}
