//! The timeline: what plays, where, with what automation.
//!
//! # A view, not a second source of truth
//!
//! ADR-0003 makes the project an append-only operation log, and the state a
//! pure fold over it. This module is the *shape* that fold produces for the
//! timeline: clips on lanes, automation beside them, and the editing operations
//! that a user's gestures turn into.
//!
//! It deliberately does not persist anything and does not hold history. Undo,
//! versions and branching are properties of the log and would be worse if
//! reimplemented here — two mechanisms for going back in time is how a project
//! ends up able to reach a state neither of them believes in.
//!
//! # Editing snaps, because a mix is on a grid
//!
//! Every edit takes a [`BeatGrid`] and snaps to it. That is not a convenience
//! setting: a clip that lands three hundred samples off a bar line is a clip
//! whose transition does not land, and the whole reason the analysis engine
//! works so hard on beat accuracy is so that this module can round to it.
//!
//! Snapping is opt-out per edit rather than global, so a deliberate off-grid
//! placement is possible and explicit. A global toggle would make the same
//! gesture mean different things at different times, which is the property that
//! makes an editor feel unpredictable.

use std::collections::BTreeMap;

use prv_project::PlacementId;
use prv_time::{BeatGrid, Frames, SnapResolution};

use crate::automation::AutomationLane;
use crate::parameter::{ParameterAddress, ParameterOwner};

/// The most lanes a timeline may hold.
///
/// A DJ set is a handful of decks and their sends; a hundred and twenty-eight
/// is far past any real arrangement and bounds what a malformed or hostile
/// project file can make the application allocate.
pub const MAX_LANES: u32 = 128;

/// A track placed on the timeline.
///
/// The source region and the timeline region are separate, which is what makes
/// trimming non-destructive: shortening a clip moves its boundary, it does not
/// alter what the clip refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Clip {
    id: PlacementId,
    lane: u32,
    start: Frames,
    length: Frames,
    source_offset: Frames,
}

impl Clip {
    /// Creates a clip.
    #[must_use]
    pub const fn new(id: PlacementId, lane: u32, start: Frames, length: Frames) -> Self {
        Self {
            id,
            lane,
            start,
            length,
            source_offset: Frames::ZERO,
        }
    }

    /// Sets how far into the source the clip begins.
    #[must_use]
    pub const fn with_source_offset(mut self, offset: Frames) -> Self {
        self.source_offset = offset;
        self
    }

    /// The clip's identity, which is the log's placement identifier.
    #[must_use]
    pub const fn id(self) -> PlacementId {
        self.id
    }

    /// Which lane it sits on.
    #[must_use]
    pub const fn lane(self) -> u32 {
        self.lane
    }

    /// Where it begins on the timeline.
    #[must_use]
    pub const fn start(self) -> Frames {
        self.start
    }

    /// How long it plays for.
    #[must_use]
    pub const fn length(self) -> Frames {
        self.length
    }

    /// Where it ends on the timeline, exclusive.
    #[must_use]
    pub fn end(self) -> Frames {
        Frames::new(self.start.get().saturating_add(self.length.get()))
    }

    /// How far into the source material the clip begins.
    #[must_use]
    pub const fn source_offset(self) -> Frames {
        self.source_offset
    }

    /// Whether a position falls inside the clip.
    #[must_use]
    pub fn contains(self, position: Frames) -> bool {
        position >= self.start && position < self.end()
    }

    /// Whether two clips overlap in time.
    #[must_use]
    pub fn overlaps(self, other: Self) -> bool {
        self.start < other.end() && other.start < self.end()
    }
}

/// What went wrong with an edit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum EditError {
    /// No clip with that identifier is on the timeline.
    UnknownClip {
        /// The identifier that was not found.
        clip: PlacementId,
    },
    /// A lane number beyond [`MAX_LANES`].
    LaneOutOfRange {
        /// The lane requested.
        lane: u32,
        /// The highest permitted.
        maximum: u32,
    },
    /// A clip already occupies that span of that lane.
    ///
    /// Overlap is refused rather than resolved. A DAW that silently truncates
    /// one clip to fit another destroys work the user can only recover by
    /// undoing; refusing tells them immediately, while they still remember what
    /// they meant.
    Overlap {
        /// The clip already there.
        existing: PlacementId,
    },
    /// A length of zero or less.
    EmptyClip,
    /// A split position that is not inside the clip.
    SplitOutsideClip,
    /// The timeline already holds as many clips as it may.
    TooManyClips {
        /// The limit.
        maximum: usize,
    },
}

impl core::fmt::Display for EditError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::UnknownClip { clip } => write!(f, "no clip with identifier {}", clip.get()),
            Self::LaneOutOfRange { lane, maximum } => {
                write!(f, "lane {lane} is beyond the maximum of {maximum}")
            }
            Self::Overlap { existing } => {
                write!(f, "clip {} already occupies that span", existing.get())
            }
            Self::EmptyClip => f.write_str("a clip must have a positive length"),
            Self::SplitOutsideClip => f.write_str("the split position is not inside the clip"),
            Self::TooManyClips { maximum } => {
                write!(f, "a timeline may hold at most {maximum} clips")
            }
        }
    }
}

impl core::error::Error for EditError {}

/// The most clips a timeline may hold.
pub const MAX_CLIPS: usize = 4_096;

/// Whether an edit rounds to the grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Snap {
    /// Round to the given resolution.
    To(SnapResolution),
    /// Leave the position exactly as given.
    ///
    /// Explicit per edit rather than a global mode, so the same gesture always
    /// means the same thing.
    Off,
}

impl Snap {
    /// Applies the snap to a position.
    #[must_use]
    pub fn apply(self, grid: &BeatGrid, position: Frames) -> Frames {
        match self {
            Self::To(resolution) => grid.snap(position, resolution),
            Self::Off => position,
        }
    }
}

/// The timeline.
#[derive(Debug, Clone)]
pub struct Timeline {
    clips: BTreeMap<u64, Clip>,
    automation: Vec<AutomationLane>,
}

impl Default for Timeline {
    fn default() -> Self {
        Self::new()
    }
}

impl Timeline {
    /// Creates an empty timeline.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            // A `BTreeMap` rather than a hash map: iteration order is the
            // identifier order on every platform and every run, which ADR-0006
            // requires of anything a decision is derived from and ADR-0003
            // requires of anything that is synchronised.
            clips: BTreeMap::new(),
            automation: Vec::new(),
        }
    }

    /// Every clip, in identifier order.
    pub fn clips(&self) -> impl Iterator<Item = &Clip> {
        self.clips.values()
    }

    /// The number of clips.
    #[must_use]
    pub fn clip_count(&self) -> usize {
        self.clips.len()
    }

    /// A clip by identifier.
    #[must_use]
    pub fn clip(&self, id: PlacementId) -> Option<&Clip> {
        self.clips.get(&id.get())
    }

    /// Every automation lane.
    #[must_use]
    pub fn automation(&self) -> &[AutomationLane] {
        &self.automation
    }

    /// The clips on a lane, in time order.
    #[must_use]
    pub fn clips_on(&self, lane: u32) -> Vec<&Clip> {
        let mut found: Vec<&Clip> = self
            .clips
            .values()
            .filter(|clip| clip.lane == lane)
            .collect();
        found.sort_by_key(|clip| (clip.start.get(), clip.id.get()));
        found
    }

    /// Every clip sounding at a position.
    ///
    /// What the transport asks in order to know what to render, and what a
    /// playhead readout shows. Several clips can sound at once, which is the
    /// whole point of a mix.
    #[must_use]
    pub fn clips_at(&self, position: Frames) -> Vec<&Clip> {
        let mut found: Vec<&Clip> = self
            .clips
            .values()
            .filter(|clip| clip.contains(position))
            .collect();
        found.sort_by_key(|clip| (clip.lane, clip.id.get()));
        found
    }

    /// Where the last clip ends.
    #[must_use]
    pub fn end(&self) -> Frames {
        self.clips
            .values()
            .map(|clip| clip.end())
            .max_by_key(|end| end.get())
            .unwrap_or(Frames::ZERO)
    }

    /// Adds a clip, snapping its start to the grid.
    ///
    /// # Errors
    ///
    /// Returns [`EditError`] for an out-of-range lane, a non-positive length,
    /// an overlap with an existing clip on the same lane, or a full timeline.
    pub fn add(&mut self, clip: Clip, grid: &BeatGrid, snap: Snap) -> Result<Clip, EditError> {
        if clip.lane >= MAX_LANES {
            return Err(EditError::LaneOutOfRange {
                lane: clip.lane,
                maximum: MAX_LANES,
            });
        }
        if clip.length.get() <= 0 {
            return Err(EditError::EmptyClip);
        }
        if self.clips.len() >= MAX_CLIPS && !self.clips.contains_key(&clip.id.get()) {
            return Err(EditError::TooManyClips { maximum: MAX_CLIPS });
        }

        let snapped = Clip {
            start: snap.apply(grid, clip.start),
            ..clip
        };
        self.check_free(snapped)?;
        self.clips.insert(snapped.id.get(), snapped);
        Ok(snapped)
    }

    /// Moves a clip to a new lane and position.
    ///
    /// # Errors
    ///
    /// As [`Timeline::add`], plus [`EditError::UnknownClip`].
    pub fn move_clip(
        &mut self,
        id: PlacementId,
        lane: u32,
        start: Frames,
        grid: &BeatGrid,
        snap: Snap,
    ) -> Result<Clip, EditError> {
        let Some(&existing) = self.clips.get(&id.get()) else {
            return Err(EditError::UnknownClip { clip: id });
        };
        if lane >= MAX_LANES {
            return Err(EditError::LaneOutOfRange {
                lane,
                maximum: MAX_LANES,
            });
        }

        let moved = Clip {
            lane,
            start: snap.apply(grid, start),
            ..existing
        };
        self.check_free(moved)?;
        self.clips.insert(moved.id.get(), moved);
        Ok(moved)
    }

    /// Changes a clip's length, keeping its start.
    ///
    /// # Errors
    ///
    /// As [`Timeline::add`], plus [`EditError::UnknownClip`].
    pub fn trim_end(
        &mut self,
        id: PlacementId,
        end: Frames,
        grid: &BeatGrid,
        snap: Snap,
    ) -> Result<Clip, EditError> {
        let Some(&existing) = self.clips.get(&id.get()) else {
            return Err(EditError::UnknownClip { clip: id });
        };
        let snapped_end = snap.apply(grid, end);
        let length = snapped_end.get().saturating_sub(existing.start.get());
        if length <= 0 {
            return Err(EditError::EmptyClip);
        }

        let trimmed = Clip {
            length: Frames::new(length),
            ..existing
        };
        self.check_free(trimmed)?;
        self.clips.insert(trimmed.id.get(), trimmed);
        Ok(trimmed)
    }

    /// Moves a clip's start without moving what plays there.
    ///
    /// The source offset moves by the same amount as the start, so the audio
    /// under the remaining part of the clip does not shift. Trimming the front
    /// of a clip and having the music slide is the single most common way a
    /// timeline editor surprises someone.
    ///
    /// # Errors
    ///
    /// As [`Timeline::add`], plus [`EditError::UnknownClip`].
    pub fn trim_start(
        &mut self,
        id: PlacementId,
        start: Frames,
        grid: &BeatGrid,
        snap: Snap,
    ) -> Result<Clip, EditError> {
        let Some(&existing) = self.clips.get(&id.get()) else {
            return Err(EditError::UnknownClip { clip: id });
        };
        let snapped = snap.apply(grid, start);
        let delta = snapped.get().saturating_sub(existing.start.get());
        let length = existing.length.get().saturating_sub(delta);
        if length <= 0 {
            return Err(EditError::EmptyClip);
        }
        let source_offset = existing.source_offset.get().saturating_add(delta).max(0);

        let trimmed = Clip {
            start: snapped,
            length: Frames::new(length),
            source_offset: Frames::new(source_offset),
            ..existing
        };
        self.check_free(trimmed)?;
        self.clips.insert(trimmed.id.get(), trimmed);
        Ok(trimmed)
    }

    /// Splits a clip in two at a position.
    ///
    /// The second half is given `new_id` and keeps playing the same audio it
    /// would have played had the clip not been split — which is what makes a
    /// split a purely structural operation rather than an edit to the sound.
    ///
    /// # Errors
    ///
    /// Returns [`EditError::SplitOutsideClip`] when the position is not strictly
    /// inside the clip, plus the errors of [`Timeline::add`].
    pub fn split(
        &mut self,
        id: PlacementId,
        at: Frames,
        new_id: PlacementId,
        grid: &BeatGrid,
        snap: Snap,
    ) -> Result<(Clip, Clip), EditError> {
        let Some(&existing) = self.clips.get(&id.get()) else {
            return Err(EditError::UnknownClip { clip: id });
        };
        if self.clips.len() >= MAX_CLIPS {
            return Err(EditError::TooManyClips { maximum: MAX_CLIPS });
        }

        let position = snap.apply(grid, at);
        if position <= existing.start || position >= existing.end() {
            return Err(EditError::SplitOutsideClip);
        }

        let consumed = position.get().saturating_sub(existing.start.get());
        let first = Clip {
            length: Frames::new(consumed),
            ..existing
        };
        let second = Clip {
            id: new_id,
            start: position,
            length: Frames::new(existing.length.get().saturating_sub(consumed)),
            source_offset: Frames::new(existing.source_offset.get().saturating_add(consumed)),
            lane: existing.lane,
        };

        self.clips.insert(first.id.get(), first);
        self.clips.insert(second.id.get(), second);
        Ok((first, second))
    }

    /// Removes a clip and every automation lane that belonged only to it.
    ///
    /// Returns the clip that was removed, if there was one.
    ///
    /// Automation addressed to the clip goes with it, because an automation
    /// lane pointing at a placement that no longer exists is a lane that can
    /// never be edited and never be heard. Automation on the *lane* stays,
    /// because the lane is still there.
    pub fn remove(&mut self, id: PlacementId) -> Option<Clip> {
        let removed = self.clips.remove(&id.get())?;
        let owner = ParameterOwner::Placement(id);
        self.automation
            .retain(|lane| !lane.address().is_within(&owner));
        Some(removed)
    }

    /// Adds or replaces an automation lane.
    ///
    /// One lane per address: two lanes driving the same parameter would make
    /// the value depend on which was evaluated last, and no ordering of them is
    /// more correct than another.
    pub fn set_automation(&mut self, lane: AutomationLane) {
        if let Some(existing) = self
            .automation
            .iter_mut()
            .find(|existing| existing.address() == lane.address())
        {
            *existing = lane;
            return;
        }
        self.automation.push(lane);
        self.automation.sort_by(|a, b| a.address().cmp(b.address()));
    }

    /// The automation lane for an address, if there is one.
    #[must_use]
    pub fn automation_for(&self, address: &ParameterAddress) -> Option<&AutomationLane> {
        self.automation
            .iter()
            .find(|lane| lane.address() == address)
    }

    /// The automation lane for an address, for editing.
    pub fn automation_for_mut(
        &mut self,
        address: &ParameterAddress,
    ) -> Option<&mut AutomationLane> {
        self.automation
            .iter_mut()
            .find(|lane| lane.address() == address)
    }

    /// The value every automated parameter holds at a position.
    ///
    /// Ordered by address, so a caller applying them does so in the same order
    /// on every run — which is what makes a rendered export match a preview.
    #[must_use]
    pub fn automation_at(&self, position: Frames) -> Vec<(&ParameterAddress, f32)> {
        self.automation
            .iter()
            .filter_map(|lane| lane.value_at(position).map(|value| (lane.address(), value)))
            .collect()
    }

    /// Refuses an edit that would overlap an existing clip on the same lane.
    fn check_free(&self, clip: Clip) -> Result<(), EditError> {
        for existing in self.clips.values() {
            if existing.id == clip.id || existing.lane != clip.lane {
                continue;
            }
            if existing.overlaps(clip) {
                return Err(EditError::Overlap {
                    existing: existing.id,
                });
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::panic,
        clippy::float_cmp,
        reason = "a test that cannot build its own fixture should fail loudly"
    )]

    use super::*;
    use crate::automation::{AutomationPoint, Interpolation};
    use crate::parameter::ParameterKey;
    use prv_time::{SampleRate, Tempo, TimeSignature};

    /// A bar at 120 BPM and 44.1 kHz is 88 200 frames.
    const BAR: i64 = 88_200;

    fn grid() -> BeatGrid {
        BeatGrid::new(
            SampleRate::HZ_44100,
            Tempo::from_bpm(120.0).expect("valid"),
            TimeSignature::FOUR_FOUR,
            Frames::ZERO,
        )
    }

    fn clip(id: u64, lane: u32, start: i64, length: i64) -> Clip {
        Clip::new(
            PlacementId::new(id),
            lane,
            Frames::new(start),
            Frames::new(length),
        )
    }

    #[test]
    fn an_added_clip_snaps_to_the_grid() {
        // The reason the analysis engine works so hard on beat accuracy: so
        // that this rounding lands on the music rather than near it.
        let mut timeline = Timeline::new();
        let placed = timeline
            .add(
                clip(1, 0, BAR + 300, BAR * 4),
                &grid(),
                Snap::To(SnapResolution::Bar),
            )
            .expect("valid");
        assert_eq!(placed.start(), Frames::new(BAR));

        let exact = timeline
            .add(clip(2, 1, BAR + 300, BAR), &grid(), Snap::Off)
            .expect("valid");
        assert_eq!(
            exact.start(),
            Frames::new(BAR + 300),
            "snapping off must leave the position exactly as given"
        );
    }

    #[test]
    fn overlapping_clips_on_one_lane_are_refused_rather_than_truncated() {
        // A DAW that silently truncates one clip to fit another destroys work
        // the user can only recover by undoing. Refusing tells them while they
        // still remember what they meant.
        let mut timeline = Timeline::new();
        timeline
            .add(clip(1, 0, 0, BAR * 4), &grid(), Snap::Off)
            .expect("valid");

        let result = timeline.add(clip(2, 0, BAR * 2, BAR * 4), &grid(), Snap::Off);
        assert_eq!(
            result.err(),
            Some(EditError::Overlap {
                existing: PlacementId::new(1)
            })
        );
        assert_eq!(
            timeline.clip_count(),
            1,
            "the refused clip must not be added"
        );

        // The same span on a different lane is fine — that is what lanes are.
        assert!(timeline
            .add(clip(3, 1, BAR * 2, BAR * 4), &grid(), Snap::Off)
            .is_ok());
    }

    #[test]
    fn trimming_the_front_does_not_slide_the_audio() {
        // The single most common way a timeline editor surprises someone. The
        // source offset must move with the start, so what plays at a given
        // moment stays what played there.
        let mut timeline = Timeline::new();
        timeline
            .add(clip(1, 0, BAR, BAR * 4), &grid(), Snap::Off)
            .expect("valid");

        let trimmed = timeline
            .trim_start(
                PlacementId::new(1),
                Frames::new(BAR * 2),
                &grid(),
                Snap::Off,
            )
            .expect("valid");

        assert_eq!(trimmed.start(), Frames::new(BAR * 2));
        assert_eq!(trimmed.length(), Frames::new(BAR * 3));
        assert_eq!(
            trimmed.source_offset(),
            Frames::new(BAR),
            "the source did not follow the trim, so the audio slid"
        );
        assert_eq!(trimmed.end(), Frames::new(BAR * 5), "the end moved");
    }

    #[test]
    fn splitting_is_structural_and_changes_no_sound() {
        // Both halves must play exactly what the whole would have played. If
        // the second half's source offset were wrong, a split would be an edit
        // to the music rather than to the arrangement.
        let mut timeline = Timeline::new();
        timeline
            .add(
                clip(1, 0, BAR, BAR * 4).with_source_offset(Frames::new(1000)),
                &grid(),
                Snap::Off,
            )
            .expect("valid");

        let (first, second) = timeline
            .split(
                PlacementId::new(1),
                Frames::new(BAR * 3),
                PlacementId::new(2),
                &grid(),
                Snap::Off,
            )
            .expect("inside the clip");

        assert_eq!(first.start(), Frames::new(BAR));
        assert_eq!(first.end(), Frames::new(BAR * 3));
        assert_eq!(first.source_offset(), Frames::new(1000));

        assert_eq!(second.start(), Frames::new(BAR * 3));
        assert_eq!(second.end(), Frames::new(BAR * 5));
        assert_eq!(
            second.source_offset(),
            Frames::new(1000 + BAR * 2),
            "the second half plays the wrong audio"
        );

        // Together they cover exactly what the original did, with no gap.
        assert_eq!(first.end(), second.start());
        assert_eq!(timeline.clip_count(), 2);
    }

    #[test]
    fn a_split_outside_the_clip_is_refused() {
        let mut timeline = Timeline::new();
        timeline
            .add(clip(1, 0, BAR, BAR * 2), &grid(), Snap::Off)
            .expect("valid");

        for position in [0_i64, BAR, BAR * 3, BAR * 10] {
            assert_eq!(
                timeline
                    .split(
                        PlacementId::new(1),
                        Frames::new(position),
                        PlacementId::new(9),
                        &grid(),
                        Snap::Off,
                    )
                    .err(),
                Some(EditError::SplitOutsideClip),
                "a split at {position} should be refused"
            );
        }
        assert_eq!(timeline.clip_count(), 1);
    }

    #[test]
    fn removing_a_clip_takes_its_automation_and_leaves_the_lanes_alone() {
        // Automation pointing at a placement that no longer exists can never be
        // edited and never be heard; automation on the lane is still reachable.
        let mut timeline = Timeline::new();
        timeline
            .add(clip(1, 0, 0, BAR * 4), &grid(), Snap::Off)
            .expect("valid");

        let on_clip = ParameterAddress::new(
            ParameterOwner::Placement(PlacementId::new(1)).effect(0),
            ParameterKey::Filter,
        )
        .expect("valid");
        let on_lane =
            ParameterAddress::new(ParameterOwner::Lane(0), ParameterKey::Gain).expect("valid");

        timeline.set_automation(AutomationLane::new(on_clip.clone()));
        timeline.set_automation(AutomationLane::new(on_lane.clone()));
        assert_eq!(timeline.automation().len(), 2);

        let removed = timeline.remove(PlacementId::new(1)).expect("it was there");
        assert_eq!(removed.id(), PlacementId::new(1));
        assert!(timeline.automation_for(&on_clip).is_none());
        assert!(
            timeline.automation_for(&on_lane).is_some(),
            "lane automation belongs to the lane, not to the clip that was on it"
        );
        assert!(timeline.remove(PlacementId::new(1)).is_none());
    }

    #[test]
    fn one_lane_per_address_however_many_times_it_is_set() {
        // Two lanes driving one parameter would make the value depend on which
        // was evaluated last, and no ordering of them is more correct.
        let mut timeline = Timeline::new();
        let address =
            ParameterAddress::new(ParameterOwner::Master, ParameterKey::Gain).expect("valid");

        for value in [0.2_f32, 0.8] {
            let mut lane = AutomationLane::new(address.clone());
            lane.insert(AutomationPoint::new(
                Frames::ZERO,
                value,
                Interpolation::Linear,
            ))
            .expect("room");
            timeline.set_automation(lane);
        }

        assert_eq!(timeline.automation().len(), 1);
        assert_eq!(
            timeline
                .automation_for(&address)
                .and_then(|lane| lane.value_at(Frames::ZERO)),
            Some(0.8)
        );
    }

    #[test]
    fn clips_at_a_position_finds_everything_sounding_there() {
        // Several clips sound at once — that is what a mix is.
        let mut timeline = Timeline::new();
        timeline
            .add(clip(1, 0, 0, BAR * 8), &grid(), Snap::Off)
            .expect("valid");
        timeline
            .add(clip(2, 1, BAR * 4, BAR * 8), &grid(), Snap::Off)
            .expect("valid");

        let overlapping = timeline.clips_at(Frames::new(BAR * 5));
        assert_eq!(overlapping.len(), 2);

        let single = timeline.clips_at(Frames::new(BAR));
        assert_eq!(single.len(), 1);
        assert_eq!(
            single.first().map(|clip| clip.id()),
            Some(PlacementId::new(1))
        );

        // The boundary is half open, so a clip ending at a position is not
        // sounding at it — otherwise two abutting clips would both be reported
        // at the join.
        assert!(timeline.clips_at(Frames::new(BAR * 12)).is_empty());
        assert_eq!(timeline.end(), Frames::new(BAR * 12));
    }

    #[test]
    fn clips_on_a_lane_come_back_in_time_order() {
        let mut timeline = Timeline::new();
        for (id, start) in [(3_u64, BAR * 8), (1, 0), (2, BAR * 4)] {
            timeline
                .add(clip(id, 0, start, BAR * 2), &grid(), Snap::Off)
                .expect("valid");
        }
        let starts: Vec<i64> = timeline
            .clips_on(0)
            .iter()
            .map(|clip| clip.start().get())
            .collect();
        assert_eq!(starts, vec![0, BAR * 4, BAR * 8]);
        assert!(timeline.clips_on(5).is_empty());
    }

    #[test]
    fn automation_is_reported_in_a_stable_order() {
        // A rendered export must match a preview, which means the same
        // parameters must be applied in the same order on every run.
        let mut timeline = Timeline::new();
        for key in [
            ParameterKey::Filter,
            ParameterKey::Gain,
            ParameterKey::EqLow,
        ] {
            let address = ParameterAddress::new(ParameterOwner::Master, key).expect("valid");
            let mut lane = AutomationLane::new(address);
            lane.insert(AutomationPoint::new(
                Frames::ZERO,
                0.5,
                Interpolation::Linear,
            ))
            .expect("room");
            timeline.set_automation(lane);
        }

        let first: Vec<String> = timeline
            .automation_at(Frames::ZERO)
            .iter()
            .map(|(address, _)| address.to_string())
            .collect();
        let second: Vec<String> = timeline
            .automation_at(Frames::ZERO)
            .iter()
            .map(|(address, _)| address.to_string())
            .collect();
        assert_eq!(first, second, "two identical queries gave different orders");
        assert_eq!(first.len(), 3);

        // The order is the address type's own ordering, which is total and
        // therefore reproducible. It is deliberately not alphabetical by
        // display text: display text is localised, and an order that depended
        // on it would change with the user's language.
        let addresses: Vec<&ParameterAddress> = timeline
            .automation_at(Frames::ZERO)
            .into_iter()
            .map(|(address, _)| address)
            .collect();
        let mut sorted = addresses.clone();
        sorted.sort();
        assert_eq!(addresses, sorted, "automation is not in address order");
    }

    #[test]
    fn edits_to_a_clip_that_is_not_there_are_reported() {
        let mut timeline = Timeline::new();
        let missing = PlacementId::new(42);
        let expected = Err(EditError::UnknownClip { clip: missing });

        assert_eq!(
            timeline.move_clip(missing, 0, Frames::ZERO, &grid(), Snap::Off),
            expected
        );
        assert_eq!(
            timeline.trim_end(missing, Frames::new(BAR), &grid(), Snap::Off),
            expected
        );
        assert_eq!(
            timeline.trim_start(missing, Frames::new(BAR), &grid(), Snap::Off),
            expected
        );
        assert!(matches!(
            timeline.split(
                missing,
                Frames::new(BAR),
                PlacementId::new(1),
                &grid(),
                Snap::Off
            ),
            Err(EditError::UnknownClip { .. })
        ));
    }

    #[test]
    fn degenerate_clips_are_refused() {
        let mut timeline = Timeline::new();
        assert_eq!(
            timeline.add(clip(1, 0, 0, 0), &grid(), Snap::Off).err(),
            Some(EditError::EmptyClip)
        );
        assert_eq!(
            timeline.add(clip(2, 0, 0, -100), &grid(), Snap::Off).err(),
            Some(EditError::EmptyClip)
        );
        assert_eq!(
            timeline
                .add(clip(3, MAX_LANES, 0, BAR), &grid(), Snap::Off)
                .err(),
            Some(EditError::LaneOutOfRange {
                lane: MAX_LANES,
                maximum: MAX_LANES
            })
        );

        timeline
            .add(clip(4, 0, BAR, BAR * 2), &grid(), Snap::Off)
            .expect("valid");
        assert_eq!(
            timeline
                .trim_end(PlacementId::new(4), Frames::new(BAR), &grid(), Snap::Off)
                .err(),
            Some(EditError::EmptyClip),
            "trimming a clip to nothing is a mistake, not a deletion"
        );
    }

    #[test]
    fn moving_a_clip_onto_itself_is_allowed() {
        // The overlap check must exclude the clip being edited, or nudging a
        // clip by one bar would collide with where it currently is.
        let mut timeline = Timeline::new();
        timeline
            .add(clip(1, 0, BAR * 4, BAR * 4), &grid(), Snap::Off)
            .expect("valid");

        let moved = timeline
            .move_clip(
                PlacementId::new(1),
                0,
                Frames::new(BAR * 5),
                &grid(),
                Snap::Off,
            )
            .expect("a clip may overlap where it used to be");
        assert_eq!(moved.start(), Frames::new(BAR * 5));
        assert_eq!(timeline.clip_count(), 1);
    }

    #[test]
    fn a_timeline_cannot_grow_without_bound() {
        let mut timeline = Timeline::new();
        for index in 0..MAX_CLIPS {
            let start = i64::try_from(index).unwrap_or(0) * BAR;
            timeline
                .add(
                    clip(u64::try_from(index).unwrap_or(0), 0, start, BAR),
                    &grid(),
                    Snap::Off,
                )
                .expect("within the limit");
        }
        assert_eq!(
            timeline
                .add(clip(u64::MAX, 1, 0, BAR), &grid(), Snap::Off)
                .err(),
            Some(EditError::TooManyClips { maximum: MAX_CLIPS })
        );
    }
}
