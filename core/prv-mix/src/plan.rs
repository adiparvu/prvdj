//! The search: building a set from candidates under constraints.
//!
//! # Beam search, and why not the obvious alternatives
//!
//! **Greedy** — take the best next track at each step — is what most automatic
//! playlist features do, and it fails in a specific, recognisable way: it spends
//! the strongest records early because they score well as *the next thing*, and
//! then has nothing left for the peak. A set is a shape over an hour, and a
//! decision that is locally optimal at minute ten can make minute fifty
//! impossible.
//!
//! **Exhaustive** is not available. A hundred tracks in a twenty-track set is
//! more orderings than there are atoms in the observable universe.
//!
//! **Beam search** keeps the best few hundred partial sets at each step and
//! extends all of them. It is not guaranteed optimal, and that is an honest
//! limitation rather than a hidden one — but it does look far enough ahead to
//! avoid the greedy failure, its cost is linear in the length of the set, and it
//! is *deterministic*, which ADR-0006 requires and which a sampling method
//! would not be.
//!
//! # Version A, B and C are different optima, not three samples
//!
//! Master Prompt #3B requires alternatives that are genuinely different rather
//! than three variations on one idea. A beam naturally converges: its top few
//! results usually share most of their tracks, differing only in the last
//! choice. Returning those as "three versions" would be a lie the user notices
//! in about ten seconds.
//!
//! So the beam is filtered by *distinctness* — two plans that share more than
//! [`SIMILARITY_LIMIT`] of their tracks are the same plan — and the best plan
//! from each distinct family is returned. Three genuinely different sets is a
//! smaller claim than three optimal sets, and it is the one worth making.

use std::collections::BTreeSet;

use prv_time::Frames;

use crate::candidate::{Candidate, TrackId};
use crate::goal::Goal;
use crate::num::{count_to_f64, narrow, signed_to_f64};
use crate::transition::{Rejection, TransitionScore};

/// How many partial sets the beam carries.
///
/// A hundred and twenty-eight. Wide enough that a locally poor choice which
/// pays off later survives to be evaluated, narrow enough that planning a
/// twenty-track set from a thousand candidates stays in the tens of
/// milliseconds. The cost is the beam width times the candidate count times the
/// set length, all of which are known in advance — so a plan never takes an
/// unpredictable amount of time, which matters because Master Prompt #12
/// requires recommendations to stream progressively.
const BEAM_WIDTH: usize = 128;

/// The fraction of shared tracks above which two plans are the same plan.
///
/// Two thirds. Below it a listener hears a different set; above it they hear the
/// same set with a couple of swaps, and calling that "Version B" is the kind of
/// false choice that teaches a user to ignore the feature.
const SIMILARITY_LIMIT: f64 = 2.0 / 3.0;

/// One track in a plan, with the move that led to it.
#[derive(Debug, Clone, PartialEq)]
pub struct PlannedTrack {
    id: TrackId,
    start: Frames,
    duration: Frames,
    transition: Option<TransitionScore>,
}

impl PlannedTrack {
    /// Which track it is.
    #[must_use]
    pub const fn id(&self) -> TrackId {
        self.id
    }

    /// Where it begins in the set.
    #[must_use]
    pub const fn start(&self) -> Frames {
        self.start
    }

    /// How long it plays for.
    #[must_use]
    pub const fn duration(&self) -> Frames {
        self.duration
    }

    /// The evidence for the move into this track.
    ///
    /// `None` for the opening track, which was chosen rather than transitioned
    /// into.
    #[must_use]
    pub const fn transition(&self) -> Option<&TransitionScore> {
        self.transition.as_ref()
    }
}

/// A complete set.
#[derive(Debug, Clone, PartialEq)]
pub struct MixPlan {
    tracks: Vec<PlannedTrack>,
    duration: Frames,
    score: f32,
}

impl MixPlan {
    /// The tracks, in order.
    #[must_use]
    pub fn tracks(&self) -> &[PlannedTrack] {
        &self.tracks
    }

    /// The total length.
    #[must_use]
    pub const fn duration(&self) -> Frames {
        self.duration
    }

    /// The mean transition score, from zero to one.
    ///
    /// The mean rather than the sum, so a long set is not preferred to a good
    /// one. Comparing a twelve-track plan with a twenty-track plan by total
    /// score would reward length, and the goal already says how long the set
    /// should be.
    #[must_use]
    pub const fn score(&self) -> f32 {
        self.score
    }

    /// How far the set's length is from what was asked for, as a fraction.
    #[must_use]
    pub fn duration_error(&self, goal: &Goal) -> f32 {
        let target = signed_to_f64(goal.duration().get());
        if target <= 0.0 {
            return 0.0;
        }
        narrow(((signed_to_f64(self.duration.get()) - target) / target).abs())
    }

    /// The weakest move in the set, which is what a review should look at first.
    #[must_use]
    pub fn weakest_transition(&self) -> Option<&PlannedTrack> {
        self.tracks
            .iter()
            .filter(|track| track.transition.is_some())
            .reduce(|worst, track| {
                let worst_score = worst
                    .transition
                    .as_ref()
                    .map_or(1.0, TransitionScore::total);
                let score = track
                    .transition
                    .as_ref()
                    .map_or(1.0, TransitionScore::total);
                if score < worst_score {
                    track
                } else {
                    worst
                }
            })
    }

    /// The set of tracks used, for comparing plans.
    fn track_set(&self) -> BTreeSet<TrackId> {
        self.tracks.iter().map(|track| track.id).collect()
    }

    /// The fraction of tracks this plan shares with another.
    #[must_use]
    pub fn similarity(&self, other: &Self) -> f32 {
        let left = self.track_set();
        let right = other.track_set();
        let smaller = left.len().min(right.len());
        if smaller == 0 {
            return 0.0;
        }
        let shared = left.intersection(&right).count();
        narrow(count_to_f64(shared) / count_to_f64(smaller))
    }
}

/// Why no plan could be built.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum PlanError {
    /// There were no candidates to plan from.
    NoCandidates,
    /// No candidate could open the set within the goal's constraints.
    NoOpeningTrack,
    /// The set could not be extended far enough to approach the target length.
    ///
    /// Carries the last rejection encountered, so an interface can say *why* —
    /// "nothing in your library is within 6 per cent of 174 BPM" is actionable
    /// and "no plan found" is not.
    Exhausted {
        /// How much of the target length was reached, as a fraction.
        reached: f32,
        /// The last reason a move was refused, if any move was tried.
        last_rejection: Option<Rejection>,
    },
}

/// A partial set, during the search.
#[derive(Debug, Clone)]
struct Beam {
    tracks: Vec<PlannedTrack>,
    used: BTreeSet<TrackId>,
    elapsed: Frames,
    total_score: f64,
    transitions: usize,
}

impl Beam {
    fn mean_score(&self) -> f64 {
        if self.transitions == 0 {
            // A single-track set has made no moves, so it has nothing to be
            // judged on. Half rather than one, so that an unextendable plan
            // does not outrank a complete one.
            return 0.5;
        }
        self.total_score / count_to_f64(self.transitions)
    }
}

/// Plans up to `wanted` genuinely different sets.
///
/// # Errors
///
/// Returns [`PlanError`] when no set could be built, with enough detail for an
/// interface to explain what was missing.
pub fn plan(
    candidates: &[Candidate],
    goal: &Goal,
    wanted: usize,
) -> Result<Vec<MixPlan>, PlanError> {
    if candidates.is_empty() {
        return Err(PlanError::NoCandidates);
    }

    let mut beams = opening_beams(candidates, goal);
    if beams.is_empty() {
        return Err(PlanError::NoOpeningTrack);
    }

    let mut last_rejection: Option<Rejection> = None;
    let mut furthest = 0.0_f64;
    let mut finished: Vec<Beam> = Vec::new();

    // The set is built one track at a time until every beam has reached the
    // target length or cannot be extended. The loop is bounded by the candidate
    // count because no track repeats, so it always terminates.
    for _ in 0..candidates.len() {
        let mut next: Vec<Beam> = Vec::new();

        for beam in &beams {
            if beam.elapsed >= goal.duration() {
                finished.push(beam.clone());
                continue;
            }

            let Some(last) = beam.tracks.last() else {
                continue;
            };
            let Some(from) = find(candidates, last.id) else {
                continue;
            };
            let target_energy = goal.target_energy(beam.elapsed);

            let mut extended = false;
            for candidate in candidates {
                if beam.used.contains(&candidate.id()) {
                    continue;
                }
                match crate::transition::score(from, candidate, goal, target_energy) {
                    Ok(transition) => {
                        extended = true;
                        next.push(extend(beam, candidate, transition));
                    }
                    Err(rejection) => last_rejection = Some(rejection),
                }
            }

            // A beam that cannot be extended is kept as a finished plan rather
            // than discarded. A set that is eight per cent short is a result a
            // user can accept or ask to be lengthened; no result at all is not.
            if !extended {
                finished.push(beam.clone());
            }
        }

        for beam in &next {
            let reached = if goal.duration().get() > 0 {
                signed_to_f64(beam.elapsed.get()) / signed_to_f64(goal.duration().get())
            } else {
                1.0
            };
            if reached > furthest {
                furthest = reached;
            }
        }

        if next.is_empty() {
            break;
        }
        prune(&mut next);
        beams = next;
    }

    finished.extend(beams);
    if finished.is_empty() {
        return Err(PlanError::Exhausted {
            reached: narrow(furthest),
            last_rejection,
        });
    }

    let mut plans: Vec<MixPlan> = finished
        .into_iter()
        .map(|beam| MixPlan {
            score: narrow(beam.mean_score()),
            duration: beam.elapsed,
            tracks: beam.tracks,
        })
        .collect();

    // Ordered by score, then by how close the length came, then by the opening
    // track so the comparison is total and the result reproducible.
    plans.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(core::cmp::Ordering::Equal)
            .then_with(|| {
                a.duration_error(goal)
                    .partial_cmp(&b.duration_error(goal))
                    .unwrap_or(core::cmp::Ordering::Equal)
            })
            .then_with(|| {
                a.tracks
                    .first()
                    .map(|track| track.id)
                    .cmp(&b.tracks.first().map(|track| track.id))
            })
    });

    Ok(distinct(plans, wanted))
}

/// The beams the search starts from: every candidate that could open the set.
fn opening_beams(candidates: &[Candidate], goal: &Goal) -> Vec<Beam> {
    let opening_energy = goal.target_energy(Frames::ZERO);
    let mut beams: Vec<Beam> = Vec::new();

    for candidate in candidates {
        if let Some(floor) = goal.tempo_floor() {
            if narrow(candidate.tempo().bpm()) < floor {
                continue;
            }
        }
        if let Some(ceiling) = goal.tempo_ceiling() {
            if narrow(candidate.tempo().bpm()) > ceiling {
                continue;
            }
        }

        let mut used = BTreeSet::new();
        used.insert(candidate.id());
        beams.push(Beam {
            tracks: vec![PlannedTrack {
                id: candidate.id(),
                start: Frames::ZERO,
                duration: candidate.duration(),
                transition: None,
            }],
            used,
            elapsed: candidate.duration(),
            // The opening track is scored on how well its energy suits the
            // start of the set, since there is no transition to judge.
            total_score: f64::from(opening_fit(candidate.energy(), opening_energy)),
            transitions: 0,
        });
    }

    // The opening choice is pruned like any other step, so a large library does
    // not start the search with a thousand beams.
    beams.sort_by(|a, b| {
        b.total_score
            .partial_cmp(&a.total_score)
            .unwrap_or(core::cmp::Ordering::Equal)
            .then_with(|| {
                a.tracks
                    .first()
                    .map(|track| track.id)
                    .cmp(&b.tracks.first().map(|track| track.id))
            })
    });
    beams.truncate(BEAM_WIDTH);
    beams
}

/// How well an opening track's energy suits the start of the set.
fn opening_fit(energy: f32, target: f32) -> f32 {
    narrow((1.0 - f64::from((energy - target).abs())).clamp(0.0, 1.0))
}

/// Extends a beam with one more track.
fn extend(beam: &Beam, candidate: &Candidate, transition: TransitionScore) -> Beam {
    let mut tracks = beam.tracks.clone();
    let start = beam.elapsed;
    let score = f64::from(transition.total());
    tracks.push(PlannedTrack {
        id: candidate.id(),
        start,
        duration: candidate.duration(),
        transition: Some(transition),
    });

    let mut used = beam.used.clone();
    used.insert(candidate.id());

    Beam {
        tracks,
        used,
        elapsed: start
            .checked_add(candidate.duration())
            .unwrap_or(Frames::new(i64::MAX)),
        total_score: beam.total_score + score,
        transitions: beam.transitions + 1,
    }
}

/// Keeps the best beams, with a spread of opening tracks.
///
/// A plain top-`n` prune converges: after a few steps every surviving beam
/// shares an opening and the search has quietly become greedy with extra
/// bookkeeping. Reserving room for beams from different openings is what keeps
/// genuinely different sets alive long enough to be compared at the end.
fn prune(beams: &mut Vec<Beam>) {
    /// How many beams any one opening track may hold.
    const PER_OPENING: usize = 8;

    beams.sort_by(|a, b| {
        b.mean_score()
            .partial_cmp(&a.mean_score())
            .unwrap_or(core::cmp::Ordering::Equal)
            .then_with(|| {
                a.tracks
                    .first()
                    .map(|track| track.id)
                    .cmp(&b.tracks.first().map(|track| track.id))
            })
    });

    let mut kept: Vec<Beam> = Vec::with_capacity(BEAM_WIDTH);
    let mut counts: std::collections::BTreeMap<TrackId, usize> = std::collections::BTreeMap::new();

    for beam in beams.iter() {
        if kept.len() >= BEAM_WIDTH {
            break;
        }
        let Some(opening) = beam.tracks.first().map(|track| track.id) else {
            continue;
        };
        let count = counts.entry(opening).or_insert(0);
        if *count >= PER_OPENING {
            continue;
        }
        *count += 1;
        kept.push(beam.clone());
    }

    // If the quota left the beam under-full — a small library with few
    // openings — fill the remainder by score alone rather than search a
    // narrower space than necessary.
    if kept.len() < BEAM_WIDTH {
        for beam in beams.iter() {
            if kept.len() >= BEAM_WIDTH {
                break;
            }
            let already = kept.iter().any(|existing| {
                existing.tracks.len() == beam.tracks.len()
                    && existing
                        .tracks
                        .iter()
                        .zip(beam.tracks.iter())
                        .all(|(a, b)| a.id == b.id)
            });
            if !already {
                kept.push(beam.clone());
            }
        }
    }

    *beams = kept;
}

/// Takes the best plan from each distinct family.
fn distinct(plans: Vec<MixPlan>, wanted: usize) -> Vec<MixPlan> {
    let mut chosen: Vec<MixPlan> = Vec::new();
    for plan in plans {
        if chosen.len() >= wanted {
            break;
        }
        let too_similar = chosen
            .iter()
            .any(|existing| f64::from(existing.similarity(&plan)) > SIMILARITY_LIMIT);
        if !too_similar {
            chosen.push(plan);
        }
    }
    chosen
}

/// Finds a candidate by identifier.
fn find(candidates: &[Candidate], id: TrackId) -> Option<&Candidate> {
    candidates.iter().find(|candidate| candidate.id() == id)
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::panic,
        clippy::float_cmp,
        clippy::cast_precision_loss,
        reason = "a test that cannot build its own fixture should fail loudly"
    )]

    use super::*;
    use crate::candidate::TrackId;
    use crate::goal::{Creativity, EnergyShape};
    use prv_analysis::Confidence;
    use prv_harmony::{Key, PitchClass};
    use prv_time::Tempo;

    /// Five minutes at 44.1 kHz.
    const TRACK_FRAMES: i64 = 44_100 * 300;

    fn track(id: u64, bpm: f64, energy: f32, tonic: PitchClass) -> Candidate {
        Candidate::new(
            TrackId::new(id),
            Frames::new(TRACK_FRAMES),
            Tempo::from_bpm(bpm).expect("valid"),
            energy,
        )
        .with_key(Key::minor(tonic), Confidence::CERTAIN)
        .with_loudness(-8.0)
    }

    /// A library whose tracks are all compatible, spread across the energy
    /// range so an energy curve has something to follow.
    fn library(count: u64) -> Vec<Candidate> {
        (0..count)
            .map(|index| {
                let steps = f64::from(u32::try_from(count.max(2) - 1).unwrap_or(1));
                let energy =
                    crate::num::narrow(f64::from(u32::try_from(index).unwrap_or(0)) / steps);
                // Tempi within a couple of per cent of each other, so tempo is
                // never the binding constraint in these tests.
                let bpm = 126.0 + f64::from(u32::try_from(index % 3).unwrap_or(0));
                // A, E and D minor: all adjacent on the wheel.
                let tonic = match index % 3 {
                    0 => PitchClass::A,
                    1 => PitchClass::E,
                    _ => PitchClass::D,
                };
                track(index, bpm, energy, tonic)
            })
            .collect()
    }

    fn goal_for(tracks: i64, shape: EnergyShape) -> Goal {
        Goal::new(Frames::new(TRACK_FRAMES * tracks), shape)
    }

    #[test]
    fn a_plan_reaches_the_requested_length_without_repeating_a_track() {
        let candidates = library(20);
        let goal = goal_for(8, EnergyShape::Rising);
        let plans = plan(&candidates, &goal, 3).expect("a compatible library plans");

        let first = plans.first().expect("at least one plan");
        assert!(
            first.tracks().len() >= 7,
            "only {} tracks for an eight-track set",
            first.tracks().len()
        );

        let mut seen = BTreeSet::new();
        for track in first.tracks() {
            assert!(seen.insert(track.id()), "a track was used twice");
        }
        assert!(first.duration_error(&goal) < 0.2);
    }

    #[test]
    fn tracks_are_laid_end_to_end_without_gaps() {
        // The property a timeline is drawn from. A gap or an overlap here would
        // put every subsequent track at the wrong time.
        let candidates = library(15);
        let goal = goal_for(6, EnergyShape::Arc);
        let plans = plan(&candidates, &goal, 1).expect("plans");
        let first = plans.first().expect("at least one plan");

        let mut expected = Frames::ZERO;
        for track in first.tracks() {
            assert_eq!(track.start(), expected, "a gap or overlap in the timeline");
            expected = expected
                .checked_add(track.duration())
                .expect("no overflow in a test-sized set");
        }
        assert_eq!(first.duration(), expected);
    }

    #[test]
    fn the_set_follows_the_energy_shape_it_was_asked_for() {
        // The difference between a plan and a shuffle. A rising set must
        // actually rise, and this is the assertion a greedy planner fails
        // because it spends its strongest records early.
        let candidates = library(24);
        let rising = plan(&candidates, &goal_for(10, EnergyShape::Rising), 1).expect("plans");
        let falling = plan(&candidates, &goal_for(10, EnergyShape::Falling), 1).expect("plans");

        let energy_of = |plan: &MixPlan| -> Vec<f32> {
            plan.tracks()
                .iter()
                .filter_map(|track| {
                    candidates
                        .iter()
                        .find(|candidate| candidate.id() == track.id())
                        .map(Candidate::energy)
                })
                .collect()
        };

        let up = energy_of(rising.first().expect("plans"));
        let down = energy_of(falling.first().expect("plans"));

        let mean = |values: &[f32]| -> f64 {
            if values.is_empty() {
                return 0.0;
            }
            values.iter().map(|&v| f64::from(v)).sum::<f64>() / values.len() as f64
        };
        let half = up.len() >> 1;
        assert!(
            mean(up.get(half..).unwrap_or(&[])) > mean(up.get(..half).unwrap_or(&[])),
            "a rising set did not rise: {up:?}"
        );
        let half = down.len() >> 1;
        assert!(
            mean(down.get(half..).unwrap_or(&[])) < mean(down.get(..half).unwrap_or(&[])),
            "a falling set did not fall: {down:?}"
        );
    }

    #[test]
    fn the_alternatives_are_genuinely_different_sets() {
        // Master Prompt #3B requires alternatives that differ, not three
        // variations on one idea. A beam converges by nature, so this is the
        // assertion that the distinctness filter is doing real work.
        let candidates = library(30);
        let goal = goal_for(8, EnergyShape::Arc);
        let plans = plan(&candidates, &goal, 3).expect("plans");

        assert!(plans.len() >= 2, "only {} plans offered", plans.len());
        for (index, plan) in plans.iter().enumerate() {
            for other in plans.iter().skip(index + 1) {
                assert!(
                    f64::from(plan.similarity(other)) <= SIMILARITY_LIMIT,
                    "two offered plans share {} of their tracks",
                    plan.similarity(other)
                );
            }
        }
    }

    #[test]
    fn no_plan_contains_a_move_that_violates_a_hard_constraint() {
        // The guarantee, checked over a whole search rather than over one
        // scoring call. A constraint that leaked would appear here and nowhere
        // else, because the search is where pressure to compromise comes from.
        let candidates = library(24);
        let goal = goal_for(10, EnergyShape::Arc).with_creativity(Creativity::Conservative);
        let plans = plan(&candidates, &goal, 3).expect("plans");

        for plan in &plans {
            for track in plan.tracks() {
                let Some(transition) = track.transition() else {
                    continue;
                };
                if let Some(harmony) = transition.harmony() {
                    assert!(
                        !harmony.is_clashing(),
                        "a clashing move survived into a plan"
                    );
                    assert_ne!(
                        harmony.safety,
                        prv_harmony::HarmonicSafety::Risky,
                        "a risky move survived at the conservative setting"
                    );
                }
                assert!(
                    transition.tempo_change().abs() <= Creativity::Conservative.max_tempo_change(),
                    "a tempo change of {} survived a {} allowance",
                    transition.tempo_change(),
                    Creativity::Conservative.max_tempo_change()
                );
            }
        }
    }

    #[test]
    fn an_empty_library_is_reported_rather_than_returning_an_empty_plan() {
        let goal = goal_for(4, EnergyShape::Arc);
        assert_eq!(plan(&[], &goal, 3).err(), Some(PlanError::NoCandidates));
    }

    #[test]
    fn a_library_outside_the_tempo_range_says_so() {
        let candidates = library(10);
        let goal = goal_for(4, EnergyShape::Arc).with_tempo_range(170.0, 180.0);
        assert_eq!(
            plan(&candidates, &goal, 3).err(),
            Some(PlanError::NoOpeningTrack)
        );
    }

    #[test]
    fn a_short_set_is_returned_rather_than_refused() {
        // A library that runs out is a result a user can act on — accept it, or
        // ask for more music. No result at all is not.
        let candidates = library(4);
        let goal = goal_for(20, EnergyShape::Rising);
        let plans = plan(&candidates, &goal, 1).expect("a short set is still a set");
        let first = plans.first().expect("one plan");
        assert!(first.tracks().len() <= 4);
        assert!(first.duration_error(&goal) > 0.5, "it should be short");
    }

    #[test]
    fn planning_is_reproducible() {
        // ADR-0006 requires it, and a beam search with a non-total comparator
        // is the usual way this quietly stops being true.
        let candidates = library(20);
        let goal = goal_for(8, EnergyShape::Arc);
        let first = plan(&candidates, &goal, 3).expect("plans");
        let second = plan(&candidates, &goal, 3).expect("plans");

        assert_eq!(first.len(), second.len());
        for (a, b) in first.iter().zip(second.iter()) {
            let left: Vec<TrackId> = a.tracks().iter().map(PlannedTrack::id).collect();
            let right: Vec<TrackId> = b.tracks().iter().map(PlannedTrack::id).collect();
            assert_eq!(left, right);
            assert_eq!(a.score(), b.score());
        }
    }

    #[test]
    fn every_move_carries_the_evidence_that_produced_it() {
        // ADR-0006 requires explanations to render the actual arithmetic. That
        // is only possible if the arithmetic survives the search.
        let candidates = library(16);
        let goal = goal_for(6, EnergyShape::Arc);
        let plans = plan(&candidates, &goal, 1).expect("plans");
        let first = plans.first().expect("one plan");

        assert!(
            first
                .tracks()
                .first()
                .and_then(PlannedTrack::transition)
                .is_none(),
            "the opening track was not transitioned into and should carry no transition"
        );
        for track in first.tracks().iter().skip(1) {
            let transition = track
                .transition()
                .expect("every move after the first has evidence");
            let components = transition.components();
            assert!((transition.total() - components.total()).abs() < 1e-6);
            // The weakest component is what an explanation leads with, so it
            // must be derivable without recomputing anything.
            let _ = components.weakest();
        }
    }

    #[test]
    fn the_weakest_transition_is_the_lowest_scoring_one() {
        let candidates = library(16);
        let goal = goal_for(6, EnergyShape::Arc);
        let plans = plan(&candidates, &goal, 1).expect("plans");
        let first = plans.first().expect("one plan");

        let Some(weakest) = first.weakest_transition() else {
            return;
        };
        let worst = weakest.transition().map_or(1.0, TransitionScore::total);
        for track in first.tracks() {
            if let Some(transition) = track.transition() {
                assert!(transition.total() >= worst);
            }
        }
    }

    #[test]
    fn similarity_is_symmetric_and_bounded() {
        let candidates = library(12);
        let goal = goal_for(5, EnergyShape::Arc);
        let plans = plan(&candidates, &goal, 3).expect("plans");
        for (index, plan) in plans.iter().enumerate() {
            assert_eq!(plan.similarity(plan), 1.0);
            for other in plans.iter().skip(index + 1) {
                assert!((plan.similarity(other) - other.similarity(plan)).abs() < 1e-6);
                assert!((0.0..=1.0).contains(&plan.similarity(other)));
            }
        }
    }
}
