use core::fmt;
use std::collections::BTreeMap;

use prv_time::{Frames, Tempo};

use crate::operation::{MarkerId, MarkerKind, OperationPayload, PlacementId, TrackRef};
use crate::parameter::{Interpolation, ParameterAddress};

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
    /// How far into the source it begins.
    ///
    /// Zero for a placement written before the offset was recordable, which is
    /// what such a placement meant: it began at the start of its media.
    pub source_offset: Frames,
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

/// One automation point, as the document stores it.
///
/// Deliberately not the timeline's `AutomationPoint`: this is the persisted
/// form, and the timeline builds its own evaluable structure from it. Keeping
/// the two apart is what lets the timeline change how it evaluates a curve —
/// a lookup table, a different search — without changing what a project file
/// means.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AutomationValue {
    /// The normalised value, from zero to one.
    pub value: f32,
    /// How the value approaches the next point.
    pub interpolation: Interpolation,
}

/// One parameter's automation, as the document stores it.
#[derive(Debug, Clone, PartialEq)]
pub struct AutomationTrack {
    /// The points, keyed by frame position so the map keeps them in order and
    /// one position holds at most one point.
    pub points: BTreeMap<i64, AutomationValue>,
    /// Whether the lane applies.
    pub enabled: bool,
}

impl Default for AutomationTrack {
    fn default() -> Self {
        // A lane exists because a user drew on it, so it starts applied.
        // Defaulting to disabled would make every automation edit silently do
        // nothing until a second, undiscoverable action.
        Self {
            points: BTreeMap::new(),
            enabled: true,
        }
    }
}

impl AutomationTrack {
    /// Whether the track holds no points.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }
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
///
/// `Eq` is not derived: automation carries an `f32`, and floating point has no
/// total equality. `PartialEq` is what the fold's tests compare with, and
/// claiming `Eq` would be a lie about the value's semantics.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ProjectState {
    /// The project's name.
    pub name: String,
    /// Every placement, by identity.
    pub placements: BTreeMap<PlacementId, Placement>,
    /// Every marker, by identity.
    pub markers: BTreeMap<MarkerId, Marker>,

    /// Automation, one lane per parameter address.
    ///
    /// A `BTreeMap` like the rest of the state: iteration order is the address
    /// order on every platform and every run, which is what makes a rendered
    /// export match a preview and a synchronised project match its source.
    pub automation: BTreeMap<ParameterAddress, AutomationTrack>,

    /// Tempo changes, keyed by the frame at which each takes effect.
    ///
    /// A set has one tempo at a time; two records playing together run at that
    /// one tempo rather than at either of their own. Storing frames rather than
    /// ticks is deliberate: a tick position depends on the tempo map, so a map
    /// keyed by ticks would define itself in terms of itself. `prv-time` builds
    /// the tick-based structure it needs from this.
    pub tempo_changes: BTreeMap<i64, Tempo>,
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
                        source_offset: Frames::ZERO,
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
            OperationPayload::SetAutomationPoint { .. }
            | OperationPayload::RemoveAutomationPoint { .. }
            | OperationPayload::SetAutomationEnabled { .. } => self.apply_automation(payload),
            OperationPayload::SetPlacementSource {
                placement,
                source_offset,
            } => {
                if let Some(existing) = self.placements.get_mut(placement) {
                    existing.source_offset = *source_offset;
                }
            }
            OperationPayload::SetTempo { position, tempo } => {
                self.tempo_changes.insert(position.get(), *tempo);
            }
            OperationPayload::RemoveTempo { position } => {
                self.tempo_changes.remove(&position.get());
            }
        }
        // No wildcard arm. `non_exhaustive` binds other crates, not this one,
        // so within the crate that defines the payload the compiler still
        // demands every variant — which is exactly the guarantee wanted here. A
        // variant added without a fold arm would be an edit that silently
        // vanishes when the project is reopened, and this makes that a build
        // failure rather than a support ticket.
    }

    /// Applies an automation operation.
    ///
    /// Split out of [`ProjectState::apply`] to keep that function readable
    /// rather than because automation is separate in principle; it obeys
    /// exactly the same rules as everything else in the fold.
    fn apply_automation(&mut self, payload: &OperationPayload) {
        match payload {
            OperationPayload::SetAutomationPoint {
                address,
                position,
                value,
                interpolation,
            } => {
                self.automation
                    .entry(address.clone())
                    .or_default()
                    .points
                    .insert(
                        position.get(),
                        AutomationValue {
                            // Clamped here rather than trusted, because this
                            // value reaches the audio thread and a log can
                            // arrive from another device or an older build.
                            value: if value.is_nan() {
                                0.0
                            } else {
                                value.clamp(0.0, 1.0)
                            },
                            interpolation: *interpolation,
                        },
                    );
            }
            OperationPayload::RemoveAutomationPoint { address, position } => {
                if let Some(track) = self.automation.get_mut(address) {
                    track.points.remove(&position.get());
                    // A lane with no points and nothing switched off is not a
                    // lane. Leaving empty entries behind would grow the
                    // document every time a user drew and erased a sweep.
                    if track.points.is_empty() && track.enabled {
                        self.automation.remove(address);
                    }
                }
            }
            OperationPayload::SetAutomationEnabled { address, enabled } => {
                self.automation.entry(address.clone()).or_default().enabled = *enabled;
            }
            // The caller matched the automation variants before delegating, so
            // nothing else reaches here.
            _ => {}
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

            OperationPayload::SetAutomationPoint { .. }
            | OperationPayload::RemoveAutomationPoint { .. }
            | OperationPayload::SetAutomationEnabled { .. } => self.inverse_of_automation(payload),

            OperationPayload::SetPlacementSource { placement, .. } => {
                let existing = self.placements.get(placement)?;
                Some(OperationPayload::SetPlacementSource {
                    placement: *placement,
                    source_offset: existing.source_offset,
                })
            }

            // A tempo change undoes to whatever was at that position, or to its
            // absence. Both are operations, so undo stays an append.
            OperationPayload::SetTempo { position, .. } => {
                Some(self.tempo_changes.get(&position.get()).map_or(
                    OperationPayload::RemoveTempo {
                        position: *position,
                    },
                    |existing| OperationPayload::SetTempo {
                        position: *position,
                        tempo: *existing,
                    },
                ))
            }
            OperationPayload::RemoveTempo { position } => {
                let existing = self.tempo_changes.get(&position.get())?;
                Some(OperationPayload::SetTempo {
                    position: *position,
                    tempo: *existing,
                })
            }
        }
        // No wildcard arm, for the same reason `apply` has none: a variant
        // added without an inverse would be silently un-undoable.
    }

    /// The inverse of an automation operation.
    ///
    /// Split out of [`ProjectState::inverse_of`] to keep that function
    /// readable rather than because the automation cases are separate in
    /// principle; they obey exactly the same rule as the rest.
    fn inverse_of_automation(&self, payload: &OperationPayload) -> Option<OperationPayload> {
        let existing_point = |address: &ParameterAddress, position: &Frames| {
            self.automation
                .get(address)
                .and_then(|track| track.points.get(&position.get()))
                .copied()
        };

        match payload {
            // Setting a point undoes either to the point that was there or to
            // its absence. Both are expressible as operations, which is what
            // keeps undo an append rather than a rewind.
            OperationPayload::SetAutomationPoint {
                address, position, ..
            } => Some(existing_point(address, position).map_or_else(
                || OperationPayload::RemoveAutomationPoint {
                    address: address.clone(),
                    position: *position,
                },
                |existing| OperationPayload::SetAutomationPoint {
                    address: address.clone(),
                    position: *position,
                    value: existing.value,
                    interpolation: existing.interpolation,
                },
            )),

            OperationPayload::RemoveAutomationPoint { address, position } => {
                let existing = existing_point(address, position)?;
                Some(OperationPayload::SetAutomationPoint {
                    address: address.clone(),
                    position: *position,
                    value: existing.value,
                    interpolation: existing.interpolation,
                })
            }

            OperationPayload::SetAutomationEnabled { address, .. } => {
                Some(OperationPayload::SetAutomationEnabled {
                    address: address.clone(),
                    // A lane that does not exist yet is enabled by default, so
                    // that is what undoing a disable returns it to.
                    enabled: self
                        .automation
                        .get(address)
                        .is_none_or(|track| track.enabled),
                })
            }

            // The caller matched the automation variants before delegating, so
            // nothing else reaches here.
            _ => None,
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
    #![allow(
        clippy::expect_used,
        clippy::panic,
        reason = "a test that cannot build its own fixture should fail loudly"
    )]

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
    fn a_source_offset_is_recorded_without_redefining_an_older_operation() {
        // ADR-0003 forbids redefining a variant: a project written by an
        // earlier build must keep its meaning. A log that predates this
        // operation means an offset of zero, which is exactly what it meant
        // when it was written — so the new capability is a new operation.
        let mut state = ProjectState::default();
        state.apply(&place(1, 0, 1000, 0));
        assert_eq!(
            state
                .placements
                .get(&PlacementId::new(1))
                .map(|p| p.source_offset),
            Some(Frames::ZERO),
            "a placement written without an offset should begin at its start"
        );

        let set = OperationPayload::SetPlacementSource {
            placement: PlacementId::new(1),
            source_offset: Frames::new(400),
        };
        let undo = state.inverse_of(&set).expect("there is an inverse");
        state.apply(&set);
        assert_eq!(
            state
                .placements
                .get(&PlacementId::new(1))
                .map(|p| p.source_offset),
            Some(Frames::new(400))
        );

        state.apply(&undo);
        assert_eq!(
            state
                .placements
                .get(&PlacementId::new(1))
                .map(|p| p.source_offset),
            Some(Frames::ZERO),
            "undoing a source change did not restore the earlier offset"
        );

        // Setting the source of something that is not there is not undoable,
        // and says so rather than inventing an operation.
        assert!(state
            .inverse_of(&OperationPayload::SetPlacementSource {
                placement: PlacementId::new(99),
                source_offset: Frames::new(5),
            })
            .is_none());
    }

    #[test]
    fn a_tempo_change_is_a_document_value_like_any_other() {
        // A set has one tempo at a time, and it belongs to the set rather than
        // to either record playing. Keyed by frames rather than ticks because a
        // tick position depends on the tempo map, and a map keyed by ticks
        // would define itself in terms of itself.
        use prv_time::Tempo;

        let mut state = ProjectState::default();
        let opening = Tempo::from_bpm(124.0).expect("valid");
        let later = Tempo::from_bpm(128.0).expect("valid");

        let set_opening = OperationPayload::SetTempo {
            position: Frames::ZERO,
            tempo: opening,
        };
        let undo_opening = state.inverse_of(&set_opening).expect("there is an inverse");
        state.apply(&set_opening);
        assert_eq!(state.tempo_changes.get(&0), Some(&opening));

        // Changing the same point undoes to what was there, not to nothing.
        let change = OperationPayload::SetTempo {
            position: Frames::ZERO,
            tempo: later,
        };
        let undo_change = state.inverse_of(&change).expect("there is an inverse");
        state.apply(&change);
        assert_eq!(state.tempo_changes.get(&0), Some(&later));
        state.apply(&undo_change);
        assert_eq!(
            state.tempo_changes.get(&0),
            Some(&opening),
            "undoing a tempo change did not restore the earlier tempo"
        );

        state.apply(&undo_opening);
        assert!(
            state.tempo_changes.is_empty(),
            "undoing the first tempo left a change behind"
        );

        // Removing something that was never there is not undoable, and says so
        // rather than inventing an operation.
        assert!(state
            .inverse_of(&OperationPayload::RemoveTempo {
                position: Frames::new(999)
            })
            .is_none());
    }

    #[test]
    fn an_automation_edit_is_undoable_like_any_other() {
        // The gap this closes. Automation was a timeline concept the log could
        // not record, which meant every sweep a user drew was outside undo,
        // outside versions and outside synchronisation.
        use crate::parameter::{Interpolation, ParameterAddress, ParameterKey, ParameterOwner};

        let address =
            ParameterAddress::new(ParameterOwner::Master, ParameterKey::Filter).expect("valid");
        let mut state = ProjectState::default();

        let first = OperationPayload::SetAutomationPoint {
            address: address.clone(),
            position: Frames::new(1000),
            value: 0.25,
            interpolation: Interpolation::Linear,
        };
        // Undoing the creation of a point removes it, because there was
        // nothing there before.
        let undo_first = state.inverse_of(&first).expect("there is an inverse");
        state.apply(&first);
        assert_eq!(
            state
                .automation
                .get(&address)
                .and_then(|track| track.points.get(&1000))
                .map(|point| point.value),
            Some(0.25)
        );

        // Changing the same point undoes to its previous value, not to nothing.
        let second = OperationPayload::SetAutomationPoint {
            address: address.clone(),
            position: Frames::new(1000),
            value: 0.75,
            interpolation: Interpolation::Smooth,
        };
        let undo_second = state.inverse_of(&second).expect("there is an inverse");
        state.apply(&second);
        state.apply(&undo_second);
        assert_eq!(
            state
                .automation
                .get(&address)
                .and_then(|track| track.points.get(&1000))
                .map(|point| point.value),
            Some(0.25),
            "undoing a value change did not restore the earlier value"
        );

        state.apply(&undo_first);
        assert!(
            !state.automation.contains_key(&address),
            "undoing the first point left an empty lane behind"
        );
    }

    #[test]
    fn disabling_a_lane_keeps_its_points_and_undoes_cleanly() {
        // Master Prompt #9: "turn this off for a moment" is not a request to
        // delete it.
        use crate::parameter::{Interpolation, ParameterAddress, ParameterKey, ParameterOwner};

        let address =
            ParameterAddress::new(ParameterOwner::Lane(2), ParameterKey::Gain).expect("valid");
        let mut state = ProjectState::default();
        state.apply(&OperationPayload::SetAutomationPoint {
            address: address.clone(),
            position: Frames::ZERO,
            value: 0.5,
            interpolation: Interpolation::Linear,
        });

        let disable = OperationPayload::SetAutomationEnabled {
            address: address.clone(),
            enabled: false,
        };
        let undo = state.inverse_of(&disable).expect("there is an inverse");
        state.apply(&disable);

        let track = state
            .automation
            .get(&address)
            .expect("the lane is still there");
        assert!(!track.enabled);
        assert_eq!(track.points.len(), 1, "disabling discarded the points");

        state.apply(&undo);
        assert!(
            state
                .automation
                .get(&address)
                .is_some_and(|track| track.enabled),
            "undoing a disable did not re-enable the lane"
        );
    }

    #[test]
    fn an_automation_value_from_the_log_is_clamped_before_it_is_stored() {
        // A log can arrive from another device or an older build, and this
        // value reaches the audio thread. Trusting it would let a non-number
        // silence every filter it touches until the engine restarts.
        use crate::parameter::{Interpolation, ParameterAddress, ParameterKey, ParameterOwner};

        let address =
            ParameterAddress::new(ParameterOwner::Master, ParameterKey::Gain).expect("valid");
        let mut state = ProjectState::default();
        for (position, value, expected) in [
            (0_i64, f32::NAN, 0.0_f32),
            (1, 9.0, 1.0),
            (2, -3.0, 0.0),
            (3, f32::INFINITY, 1.0),
        ] {
            state.apply(&OperationPayload::SetAutomationPoint {
                address: address.clone(),
                position: Frames::new(position),
                value,
                interpolation: Interpolation::Linear,
            });
            assert_eq!(
                state
                    .automation
                    .get(&address)
                    .and_then(|track| track.points.get(&position))
                    .map(|point| point.value),
                Some(expected),
                "a value of {value} was stored unclamped"
            );
        }
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
