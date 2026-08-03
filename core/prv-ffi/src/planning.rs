//! The planner, as a host sees it.
//!
//! # Why the candidates are pushed one at a time
//!
//! A library is thousands of tracks, each with a tempo, a key, an energy, a
//! loudness and a couple of exit points. Passing that across C as one array
//! would mean a `#[repr(C)]` struct — which is a layout promise, permanent, and
//! the first field anybody wants to add breaks every host compiled against it.
//!
//! So a candidate is *built* by a call with its facts as arguments. It costs one
//! call per track, which for a ten-thousand-track library is a few milliseconds
//! once, on the thread that was reading the library anyway. What it buys is a
//! boundary where adding "danceability" next year is a new function rather than
//! a new major version.
//!
//! # Why a plan is held rather than returned
//!
//! A plan is a variable number of tracks, each with a start, a length and a
//! score. Returning that means either allocating something the host frees — a
//! second ownership rule to get right — or a two-pass "ask the size, then ask
//! again" dance that races if anything changes between the passes.
//!
//! Instead the engine keeps the plan it just made and the host reads it back one
//! track at a time by index. Nothing is allocated on the host's behalf, nothing
//! has to be freed, and the count cannot change underneath a caller that is
//! reading it, because making a new plan is a different call.
//!
//! # Applying a plan is an ordinary edit
//!
//! [`Planner::apply`] turns the plan into operations and appends them to the
//! project's log, exactly as `prv-mix::render` produces them and exactly as a
//! hand-made edit would. Master Prompt #3B requires the user to be able to edit
//! everything the system decides; that is true here by construction rather than
//! by a promise, because after `apply` there is nothing to distinguish a
//! generated placement from one somebody dragged.

use prv_analysis::Confidence;
use prv_harmony::{Key, PitchClass};
use prv_mix::transition::Weights;
use prv_mix::{
    affinity, Candidate, Creativity, EnergyShape, Goal, MixPlan, Neighbour, PlacementIds, TrackId,
};
use prv_time::{Frames, SampleRate, Tempo};

use crate::status::Status;

/// The most candidates a host may add.
///
/// A hundred thousand. Far above any real library and low enough that a runaway
/// import loop fails with a status rather than by exhausting memory.
pub const MAX_CANDIDATES: usize = 100_000;

/// How many alternative sets the planner is asked for.
///
/// Three, which is what Master Prompt #3B calls Version A, B and C. Fixed rather
/// than a parameter because the planner filters for genuine distinctness and a
/// host asking for twenty would get three anyway.
pub const WANTED_PLANS: usize = 3;

/// The library and the plan made from it.
#[derive(Debug, Default)]
pub struct Planner {
    candidates: Vec<Candidate>,
    plans: Vec<MixPlan>,
    /// The answer to the last "what goes with this?", held so a host can read
    /// it one row at a time without the core ranking a library per row.
    neighbours: Vec<Neighbour>,
    /// Which of `plans` a host is currently reading.
    selected: usize,
}

impl Planner {
    /// An empty library.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            candidates: Vec::new(),
            plans: Vec::new(),
            neighbours: Vec::new(),
            selected: 0,
        }
    }

    /// Adds one track to the library the planner chooses from.
    ///
    /// Optional facts are passed as sentinels rather than as separate calls,
    /// because a host that has to remember which of five setters to call for
    /// each track will eventually forget one. A key confidence of zero or below
    /// means the key is unknown; `has_vocals` is `-1` for unknown, `0` for no
    /// and `1` for yes.
    ///
    /// # Errors
    ///
    /// [`Status::InvalidArgument`] for a tempo, duration or energy the planner
    /// cannot use, and [`Status::Refused`] once the library is full.
    #[allow(
        clippy::too_many_arguments,
        reason = "a candidate is what the analysis found out about a track, and \
                  a repr(C) struct here would be a permanent layout promise for \
                  the sake of a shorter signature"
    )]
    pub fn add_candidate(
        &mut self,
        track: u64,
        duration: i64,
        bpm: f64,
        energy: f32,
        key_semitones: i32,
        key_is_minor: bool,
        key_confidence: f32,
        loudness_lufs: f32,
        has_vocals: i32,
    ) -> Result<(), Status> {
        if self.candidates.len() >= MAX_CANDIDATES {
            return Err(Status::Refused);
        }
        if duration <= 0 || !energy.is_finite() || !loudness_lufs.is_finite() {
            return Err(Status::InvalidArgument);
        }
        let Ok(tempo) = Tempo::from_bpm(bpm) else {
            return Err(Status::InvalidArgument);
        };

        let mut candidate = Candidate::new(
            TrackId::new(track),
            Frames::new(duration),
            tempo,
            energy.clamp(0.0, 1.0),
        )
        .with_loudness(loudness_lufs);

        if key_confidence > 0.0 && key_confidence.is_finite() {
            // Semitones wrap, so a host that counts from a different C still
            // names a pitch class rather than falling off the end.
            let Ok(semitones) = u8::try_from(key_semitones.rem_euclid(12)) else {
                return Err(Status::InvalidArgument);
            };
            let tonic = PitchClass::from_semitones(semitones);
            let key = if key_is_minor {
                Key::minor(tonic)
            } else {
                Key::major(tonic)
            };
            candidate = candidate.with_key(key, Confidence::new(key_confidence));
        }

        match has_vocals {
            0 => candidate = candidate.with_vocals(false),
            1 => candidate = candidate.with_vocals(true),
            // Anything else means "nobody has looked", which is a different
            // thing from "no vocals" and scores differently.
            _ => {}
        }

        self.candidates.push(candidate);
        Ok(())
    }

    /// Adds a place the analysis says a track can be left or entered.
    ///
    /// `is_exit` distinguishes the two. A track with no exit point is mixed out
    /// of near its end, which the planner treats as a real answer rather than a
    /// missing one.
    ///
    /// # Errors
    ///
    /// [`Status::InvalidHandle`] when no candidate with that identity has been
    /// added, which is a host calling in the wrong order rather than a bad
    /// value.
    pub fn add_mix_point(
        &mut self,
        track: u64,
        position: i64,
        energy: f32,
        is_exit: bool,
    ) -> Result<(), Status> {
        if position < 0 || !energy.is_finite() {
            return Err(Status::InvalidArgument);
        }
        let role = if is_exit {
            prv_mix::MixPointRole::Exit
        } else {
            prv_mix::MixPointRole::Entry
        };
        let point = prv_mix::MixPoint::new(Frames::new(position), energy.clamp(0.0, 1.0), role);

        let id = TrackId::new(track);
        let Some(slot) = self
            .candidates
            .iter_mut()
            .find(|candidate| candidate.id() == id)
        else {
            return Err(Status::InvalidHandle);
        };
        // `with_point` consumes, so the candidate is taken out and put back.
        // Cheap, and it keeps the builder's shape rather than adding a mutating
        // twin that could drift from it.
        let replaced = core::mem::replace(
            slot,
            Candidate::new(id, Frames::new(1), Tempo::BPM_120, 0.5),
        );
        *slot = replaced.with_point(point);
        Ok(())
    }

    /// How many candidates the library holds.
    #[must_use]
    pub fn candidate_count(&self) -> u64 {
        self.candidates.len().try_into().unwrap_or(u64::MAX)
    }

    /// The records that sit best after — or before — a given one.
    ///
    /// # Why this is on the planner rather than on the collection
    ///
    /// It needs the same facts a plan needs: tempo, key, energy, level, and
    /// where a record can be mixed. The planner's library already holds exactly
    /// those, assembled by exactly the calls that assemble them for planning. A
    /// second path would mean a second place for a host to describe its records,
    /// and the two would drift.
    ///
    /// # Errors
    ///
    /// [`Status::InvalidArgument`] when `track` is not in the library, which is
    /// worth reporting rather than answering with an empty list: "nothing goes
    /// with this" and "I have never heard of this" are different answers.
    pub fn neighbours(&mut self, track: u64, following: bool, limit: u64) -> Result<usize, Status> {
        let subject = self
            .candidates
            .iter()
            .find(|candidate| candidate.id() == TrackId::new(track))
            .ok_or(Status::InvalidArgument)?;

        let limit = usize::try_from(limit).unwrap_or(usize::MAX);
        self.neighbours = if following {
            affinity::what_follows(subject, &self.candidates, Weights::DEFAULT, limit)
        } else {
            affinity::what_precedes(subject, &self.candidates, Weights::DEFAULT, limit)
        };
        Ok(self.neighbours.len())
    }

    /// One neighbour from the last call to [`Planner::neighbours`].
    ///
    /// Returns the record, its total, and the component that costs the pairing
    /// the most — because a DJ told "0.71" learns nothing and a DJ told "the
    /// tempo is the hard part" knows what to do about it.
    ///
    /// # Errors
    ///
    /// [`Status::InvalidArgument`] for an index past the end.
    pub fn neighbour(&self, index: u64) -> Result<(u64, f32, i32), Status> {
        let index = usize::try_from(index).map_err(|_| Status::InvalidArgument)?;
        let found = self.neighbours.get(index).ok_or(Status::InvalidArgument)?;
        Ok((
            found.track().get(),
            found.score(),
            crate::mapping::component_code(found.weakest()),
        ))
    }

    /// Forgets the library and any plan made from it.
    pub fn clear(&mut self) {
        self.neighbours.clear();
        self.candidates.clear();
        self.plans.clear();
        self.selected = 0;
    }

    /// Plans up to three genuinely different sets.
    ///
    /// # Errors
    ///
    /// [`Status::InvalidArgument`] for a target length or shape the planner
    /// cannot use, and [`Status::Refused`] when no set could be built — which is
    /// a real answer about the library, not a malfunction.
    pub fn plan(
        &mut self,
        target_frames: i64,
        sample_rate: u32,
        shape_code: i32,
        creativity_code: i32,
        tempo_floor: f32,
        tempo_ceiling: f32,
    ) -> Result<u64, Status> {
        if target_frames <= 0 {
            return Err(Status::InvalidArgument);
        }
        let Ok(rate) = SampleRate::new(sample_rate) else {
            return Err(Status::InvalidArgument);
        };
        let Some(shape) = shape_from_code(shape_code) else {
            return Err(Status::InvalidArgument);
        };
        let Some(creativity) = creativity_from_code(creativity_code) else {
            return Err(Status::InvalidArgument);
        };

        let mut goal =
            Goal::new(Frames::new(target_frames), rate, shape).with_creativity(creativity);
        // A range is set only when both ends are given. Half a range is a host
        // mistake, and honouring it would silently constrain a set in a way
        // nobody asked for.
        if tempo_floor > 0.0
            && tempo_ceiling > 0.0
            && tempo_floor.is_finite()
            && tempo_ceiling.is_finite()
        {
            goal = goal.with_tempo_range(tempo_floor, tempo_ceiling);
        }

        let plans =
            prv_mix::plan(&self.candidates, &goal, WANTED_PLANS).map_err(|_| Status::Refused)?;
        self.plans = plans;
        self.selected = 0;
        Ok(self.plans.len().try_into().unwrap_or(0))
    }

    /// Chooses which alternative subsequent reads describe.
    ///
    /// # Errors
    ///
    /// [`Status::InvalidArgument`] when there is no such alternative.
    pub fn select(&mut self, index: u64) -> Result<(), Status> {
        let index = usize::try_from(index).map_err(|_| Status::InvalidArgument)?;
        if index >= self.plans.len() {
            return Err(Status::InvalidArgument);
        }
        self.selected = index;
        Ok(())
    }

    /// The selected plan, if one has been made.
    fn selected_plan(&self) -> Result<&MixPlan, Status> {
        self.plans.get(self.selected).ok_or(Status::InvalidState)
    }

    /// How many tracks the selected plan holds.
    ///
    /// # Errors
    ///
    /// [`Status::InvalidState`] when nothing has been planned yet.
    pub fn track_count(&self) -> Result<u64, Status> {
        Ok(self
            .selected_plan()?
            .tracks()
            .len()
            .try_into()
            .unwrap_or(u64::MAX))
    }

    /// How long the selected plan runs for, in frames.
    ///
    /// # Errors
    ///
    /// [`Status::InvalidState`] when nothing has been planned yet.
    pub fn duration(&self) -> Result<i64, Status> {
        Ok(self.selected_plan()?.duration().get())
    }

    /// The selected plan's mean transition score, from zero to one.
    ///
    /// # Errors
    ///
    /// [`Status::InvalidState`] when nothing has been planned yet.
    pub fn score(&self) -> Result<f32, Status> {
        Ok(self.selected_plan()?.score())
    }

    /// One track of the selected plan.
    ///
    /// Returns its library identity, where it starts, how long it plays and the
    /// score of the move into it — which is `1.0` for the opening track, because
    /// it was chosen rather than transitioned into.
    ///
    /// # Errors
    ///
    /// [`Status::InvalidState`] when nothing has been planned, and
    /// [`Status::InvalidArgument`] when there is no track at that index.
    pub fn track(&self, index: u64) -> Result<(u64, i64, i64, f32), Status> {
        let plan = self.selected_plan()?;
        let index = usize::try_from(index).map_err(|_| Status::InvalidArgument)?;
        let track = plan.tracks().get(index).ok_or(Status::InvalidArgument)?;
        Ok((
            track.id().get(),
            track.start().get(),
            track.duration().get(),
            track
                .transition()
                .map_or(1.0, prv_mix::TransitionScore::total),
        ))
    }

    /// Turns the selected plan into operations for the project's log.
    ///
    /// # Errors
    ///
    /// [`Status::InvalidState`] when nothing has been planned, and
    /// [`Status::Refused`] when the plan names a track the library no longer
    /// holds — which happens if a host clears the library between planning and
    /// applying.
    pub fn operations(
        &self,
        sample_rate: SampleRate,
        first_placement: u64,
    ) -> Result<Vec<prv_project::OperationPayload>, Status> {
        let plan = self.selected_plan()?;
        let mut ids = PlacementIds::starting_at(first_placement);
        let mix = prv_mix::render(plan, &self.candidates, sample_rate, &mut ids)
            .map_err(|_| Status::Refused)?;
        Ok(mix.operations().to_vec())
    }
}

/// The energy shape a code names.
#[must_use]
pub const fn shape_from_code(code: i32) -> Option<EnergyShape> {
    match code {
        0 => Some(EnergyShape::Rising),
        1 => Some(EnergyShape::Arc),
        2 => Some(EnergyShape::Plateau),
        3 => Some(EnergyShape::Wave),
        4 => Some(EnergyShape::Falling),
        _ => None,
    }
}

/// Every energy shape with its C spelling, in code order.
pub const ENERGY_SHAPES: &[(EnergyShape, &str)] = &[
    (EnergyShape::Rising, "PRV_ENERGY_RISING"),
    (EnergyShape::Arc, "PRV_ENERGY_ARC"),
    (EnergyShape::Plateau, "PRV_ENERGY_PLATEAU"),
    (EnergyShape::Wave, "PRV_ENERGY_WAVE"),
    (EnergyShape::Falling, "PRV_ENERGY_FALLING"),
];

/// The creativity setting a code names.
#[must_use]
pub const fn creativity_from_code(code: i32) -> Option<Creativity> {
    match code {
        0 => Some(Creativity::Conservative),
        1 => Some(Creativity::Balanced),
        2 => Some(Creativity::Adventurous),
        _ => None,
    }
}

/// Every creativity setting with its C spelling, in code order.
pub const CREATIVITY_SETTINGS: &[(Creativity, &str)] = &[
    (Creativity::Conservative, "PRV_CREATIVITY_CONSERVATIVE"),
    (Creativity::Balanced, "PRV_CREATIVITY_BALANCED"),
    (Creativity::Adventurous, "PRV_CREATIVITY_ADVENTUROUS"),
];

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::panic,
        clippy::integer_division,
        reason = "a test that cannot build its own fixture should fail loudly, and \
                  a fixture's exit point at a quarter of a track is exact arithmetic"
    )]

    use super::*;

    /// Five minutes at 44.1 kHz.
    const TRACK: i64 = 44_100 * 300;

    fn library(planner: &mut Planner, count: u64) {
        for index in 0..count {
            // Tempi within a couple of per cent and keys adjacent on the wheel,
            // so neither is the binding constraint.
            let bpm = 126.0 + f64::from(u32::try_from(index % 3).unwrap_or(0));
            let semitones = match index % 3 {
                0 => 9, // A
                1 => 4, // E
                _ => 2, // D
            };
            let energy = 0.3 + 0.05 * f32::from(u8::try_from(index % 12).unwrap_or(0));
            planner
                .add_candidate(index, TRACK, bpm, energy, semitones, true, 1.0, -8.0, 0)
                .expect("a reasonable candidate");
        }
    }

    #[test]
    fn a_host_can_build_a_library_plan_a_set_and_read_it_back() {
        // The headline loop of the whole product, across the boundary.
        let mut planner = Planner::new();
        library(&mut planner, 20);
        assert_eq!(planner.candidate_count(), 20);

        let count = planner
            .plan(TRACK * 8, 44_100, 0, 1, 0.0, 0.0)
            .expect("a compatible library plans");
        assert!(count >= 1, "no alternatives were produced");

        let tracks = planner.track_count().expect("a plan was made");
        assert!(tracks >= 2, "a set of one track is not a set");

        let mut seen = std::collections::BTreeSet::new();
        let mut previous_start = -1_i64;
        for index in 0..tracks {
            let (id, start, duration, score) = planner.track(index).expect("in range");
            assert!(seen.insert(id), "a track was used twice");
            assert!(start > previous_start, "the set did not move forward");
            assert!(duration > 0);
            assert!(
                (0.0..=1.0).contains(&score),
                "score {score} is out of range"
            );
            previous_start = start;
        }
    }

    #[test]
    fn planning_with_an_empty_library_is_refused_rather_than_returning_nothing() {
        // A real answer about the library. Returning an empty plan would make
        // "your library has nothing that fits" indistinguishable from success.
        let mut planner = Planner::new();
        assert_eq!(
            planner.plan(TRACK * 4, 44_100, 0, 1, 0.0, 0.0),
            Err(Status::Refused)
        );
    }

    #[test]
    fn reading_a_plan_before_making_one_says_so() {
        let planner = Planner::new();
        assert_eq!(planner.track_count(), Err(Status::InvalidState));
        assert_eq!(planner.duration(), Err(Status::InvalidState));
        assert_eq!(planner.track(0), Err(Status::InvalidState));
    }

    #[test]
    fn the_alternatives_are_selectable_and_a_missing_one_is_refused() {
        let mut planner = Planner::new();
        library(&mut planner, 24);
        let count = planner
            .plan(TRACK * 8, 44_100, 1, 1, 0.0, 0.0)
            .expect("plans");

        for index in 0..count {
            planner.select(index).expect("an alternative that exists");
            assert!(planner.track_count().expect("selected") > 0);
        }
        assert_eq!(planner.select(count), Err(Status::InvalidArgument));
    }

    #[test]
    fn a_plan_becomes_ordinary_operations_on_the_log() {
        // Master Prompt #3B: the user can edit everything the system decides.
        // True by construction, because after this there is nothing to tell a
        // generated placement from one somebody dragged.
        let mut planner = Planner::new();
        library(&mut planner, 16);
        planner
            .plan(TRACK * 6, 44_100, 2, 1, 0.0, 0.0)
            .expect("plans");

        let operations = planner
            .operations(SampleRate::HZ_44100, 1)
            .expect("a plan renders");
        assert!(!operations.is_empty());

        let placements = operations
            .iter()
            .filter(|operation| {
                matches!(operation, prv_project::OperationPayload::PlaceTrack { .. })
            })
            .count();
        assert_eq!(
            u64::try_from(placements).unwrap_or(0),
            planner.track_count().expect("planned"),
            "the plan and the operations disagree about how many tracks there are"
        );
    }

    #[test]
    fn an_unusable_fact_about_a_track_is_refused_at_the_door() {
        let mut planner = Planner::new();
        // A tempo that is not a tempo.
        assert_eq!(
            planner.add_candidate(1, TRACK, 0.0, 0.5, 0, false, 0.0, -8.0, -1),
            Err(Status::InvalidArgument)
        );
        // A duration that is not a duration.
        assert_eq!(
            planner.add_candidate(1, 0, 128.0, 0.5, 0, false, 0.0, -8.0, -1),
            Err(Status::InvalidArgument)
        );
        // A loudness that is not a number.
        assert_eq!(
            planner.add_candidate(1, TRACK, 128.0, 0.5, 0, false, 0.0, f32::NAN, -1),
            Err(Status::InvalidArgument)
        );
        assert_eq!(planner.candidate_count(), 0, "a refused candidate was kept");
    }

    #[test]
    fn a_key_semitone_from_a_host_that_counts_differently_still_names_a_pitch() {
        // Wrapping rather than refusing, because a host counting from a
        // different C is using a convention, not making a mistake.
        let mut planner = Planner::new();
        for semitones in [-13, -1, 0, 11, 12, 25] {
            planner
                .add_candidate(
                    u64::try_from(semitones + 100).unwrap_or(0),
                    TRACK,
                    128.0,
                    0.5,
                    semitones,
                    false,
                    1.0,
                    -8.0,
                    -1,
                )
                .expect("a wrapped semitone is still a pitch class");
        }
        assert_eq!(planner.candidate_count(), 6);
    }

    #[test]
    fn a_mix_point_for_a_track_nobody_added_is_refused() {
        // A host calling in the wrong order. Silently ignoring it would lose the
        // analysis and nothing would ever say why the transitions were worse.
        let mut planner = Planner::new();
        assert_eq!(
            planner.add_mix_point(99, 1_000, 0.1, true),
            Err(Status::InvalidHandle)
        );
    }

    #[test]
    fn an_exit_point_shortens_the_set_because_records_hand_over_early() {
        // The pacing rule from Sprint 28, now reachable from a host. Without
        // this the planner and the renderer would disagree again, across C,
        // where it is much harder to see.
        let mut planner = Planner::new();
        library(&mut planner, 12);
        planner
            .plan(TRACK * 6, 44_100, 2, 1, 0.0, 0.0)
            .expect("plans");
        let without = planner.duration().expect("planned");

        let mut with_exits = Planner::new();
        library(&mut with_exits, 12);
        for index in 0..12 {
            with_exits
                .add_mix_point(index, TRACK / 4, 0.05, true)
                .expect("the track exists");
        }
        with_exits
            .plan(TRACK * 6, 44_100, 2, 1, 0.0, 0.0)
            .expect("plans");
        let with = with_exits.duration().expect("planned");

        assert!(
            with < without,
            "records that hand over a quarter of the way in produced a set no shorter ({with} vs {without})"
        );
    }

    #[test]
    fn an_undefined_shape_or_creativity_is_refused_rather_than_guessed() {
        let mut planner = Planner::new();
        library(&mut planner, 8);
        assert_eq!(
            planner.plan(TRACK * 4, 44_100, 99, 1, 0.0, 0.0),
            Err(Status::InvalidArgument)
        );
        assert_eq!(
            planner.plan(TRACK * 4, 44_100, 0, 99, 0.0, 0.0),
            Err(Status::InvalidArgument)
        );
    }

    #[test]
    fn codes_are_dense_and_names_are_distinct() {
        for (index, (shape, name)) in ENERGY_SHAPES.iter().enumerate() {
            let code = i32::try_from(index).expect("small");
            assert_eq!(shape_from_code(code), Some(*shape));
            assert!(name.starts_with("PRV_"));
        }
        for (index, (setting, name)) in CREATIVITY_SETTINGS.iter().enumerate() {
            let code = i32::try_from(index).expect("small");
            assert_eq!(creativity_from_code(code), Some(*setting));
            assert!(name.starts_with("PRV_"));
        }
        assert_eq!(shape_from_code(-1), None);
        assert_eq!(creativity_from_code(-1), None);
    }

    #[test]
    fn clearing_forgets_the_library_and_the_plan_together() {
        // Keeping a plan that names tracks the library no longer holds would be
        // a handle to something that no longer exists, which is the class of bug
        // this whole boundary is arranged to avoid.
        let mut planner = Planner::new();
        library(&mut planner, 10);
        planner
            .plan(TRACK * 4, 44_100, 0, 1, 0.0, 0.0)
            .expect("plans");

        planner.clear();
        assert_eq!(planner.candidate_count(), 0);
        assert_eq!(planner.track_count(), Err(Status::InvalidState));
    }
}
