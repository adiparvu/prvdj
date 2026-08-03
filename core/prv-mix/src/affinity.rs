//! How well two records sit together, with no set in mind.
//!
//! # A different question from the one the planner asks
//!
//! [`transition::score`](crate::transition::score) answers "how good is this
//! move *in this set, at this moment*". It needs a [`Goal`](crate::Goal),
//! because half of what makes a move good is where the evening is going: the
//! same two records are a great move at the peak and a poor one at the start.
//!
//! Master Prompt #20 asks for something the planner cannot give, because it is
//! asked before there is a set — at import, and every time somebody looks at a
//! record and wonders what goes with it. That question is about the *pair*:
//! their keys, their tempi, how far the level has to move, whether both offer
//! somewhere to mix. It has an answer without a journey, and this module is it.
//!
//! # It is not a prediction of what the planner will score
//!
//! Deliberately, and the distinction is worth keeping straight, because a
//! neighbour list that quietly disagreed with the planner would train people to
//! ignore one of them.
//!
//! Every component here is judged the same way the planner judges it, from the
//! same functions, except one. Energy is scored as *continuity* — how close the
//! incoming record's energy is to the outgoing one's — because with no set there
//! is nothing else it could mean. The planner scores energy against what the set
//! wants next, which is often deliberately not continuity: a set that is meant
//! to lift wants the next record higher, and the planner is right to prefer one.
//!
//! So affinity is the pair, and the plan is the journey. A record can be an
//! excellent neighbour and the wrong record for this moment, and both statements
//! are true at once.
//!
//! # Tempo is judged at the widest tolerance any setting allows
//!
//! There is no goal, so there is no creativity setting, so there is no tempo
//! allowance. Using the most permissive one makes the tempo component an *upper
//! bound*: no plan at any setting scores a pair's tempo higher than this does,
//! because a tighter allowance can only cost more. A neighbour list that
//! promised more than a set could deliver would be the worse mistake.
//!
//! # What is left out entirely
//!
//! A pair whose keys clash is not a neighbour at all. That is a musical fact
//! rather than a preference — `prv-harmony` separates clashing from merely risky
//! precisely so that this distinction can be made without a setting — and a list
//! that ranked unlistenable moves at the bottom would be a list nobody could
//! trust the top of.
//!
//! Everything else is scored and kept, with its weakest component named, because
//! "this works, but you will have to move the tempo four per cent" is far more
//! useful to a DJ than a number alone.

use prv_harmony::HarmonicSafety;

use crate::candidate::{Candidate, TrackId};
use crate::goal::Creativity;
use crate::num::narrow;
use crate::transition::{
    energy_component, harmonic_component, level_component, structure_component, tempo_component,
    vocal_component, Component, ScoreComponents, Weights,
};

use prv_harmony::compatibility;

/// The largest number of neighbours one call will return.
///
/// Sixty-four is far past what any interface shows and short enough that a
/// mistaken argument cannot ask for an allocation measured in gigabytes. A
/// caller wanting the whole library ranked is not asking for neighbours; it is
/// asking for a sort, and should say so.
pub const MAX_NEIGHBOURS: usize = 64;

/// One record's fit beside another.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Neighbour {
    track: TrackId,
    total: f32,
    components: ScoreComponents,
}

impl Neighbour {
    /// Which record.
    #[must_use]
    pub const fn track(&self) -> TrackId {
        self.track
    }

    /// The weighted total, zero to one.
    #[must_use]
    pub const fn score(&self) -> f32 {
        self.total
    }

    /// The components, which are the explanation.
    #[must_use]
    pub const fn components(&self) -> ScoreComponents {
        self.components
    }

    /// The component that costs this pairing the most.
    ///
    /// What an interface says out loud. A DJ told "0.71" learns nothing; a DJ
    /// told "the tempo is the hard part here" knows what to do about it.
    #[must_use]
    pub fn weakest(&self) -> Component {
        self.components.weakest()
    }
}

/// How well `to` sits after `from`, with no set in mind.
///
/// `None` when the pair is not a pairing at all: a record cannot follow itself,
/// and keys that clash are a fact rather than a preference.
///
/// # Order matters
///
/// This is not symmetric and must not be read as though it were. Going up four
/// per cent in tempo is not the same move as coming down four; a quiet outro
/// meeting a loud intro is not the same as the reverse; and energy continuity is
/// measured from the outgoing record. `affinity(a, b)` and `affinity(b, a)` are
/// two different questions with two different answers.
#[must_use]
pub fn affinity(from: &Candidate, to: &Candidate, weights: Weights) -> Option<Neighbour> {
    if from.id() == to.id() {
        return None;
    }

    let harmony = match (from.key(), to.key()) {
        (Some(left), Some(right)) => {
            let result = compatibility(left, right);
            if result.safety == HarmonicSafety::Clashing {
                return None;
            }
            Some(result)
        }
        // An unknown key is not a clash. Scoring it as one would bury every
        // record whose analysis has not finished, which is most of a library on
        // the day somebody imports it.
        _ => None,
    };

    let from_bpm = from.tempo().bpm();
    let tempo_change = if from_bpm > 0.0 {
        narrow((to.tempo().bpm() - from_bpm) / from_bpm)
    } else {
        0.0
    };

    let components = ScoreComponents {
        harmonic: harmonic_component(harmony, from, to),
        tempo: tempo_component(tempo_change, WIDEST_TOLERANCE),
        // Continuity, because with no set there is nothing else it could mean.
        energy: energy_component(to.energy(), from.energy()),
        structure: structure_component(from, to),
        level: level_component(from, to),
        vocal: vocal_component(from, to),
    };

    Some(Neighbour {
        track: to.id(),
        total: components.total_with(weights),
        components,
    })
}

/// The tempo allowance affinity judges against.
///
/// The most permissive any creativity setting offers, which is what makes the
/// tempo component an upper bound over every setting.
const WIDEST_TOLERANCE: f32 = Creativity::Adventurous.max_tempo_change();

/// The records that sit best *after* this one.
///
/// What "what mixes out of this?" means. Ranked best first, with ties broken by
/// track identity so the same library and the same record always produce the
/// same list — a shortlist that reshuffled between two identical questions would
/// be worse than no shortlist.
#[must_use]
pub fn what_follows(
    from: &Candidate,
    among: &[Candidate],
    weights: Weights,
    limit: usize,
) -> Vec<Neighbour> {
    rank(
        among.iter().filter_map(|to| affinity(from, to, weights)),
        limit,
    )
}

/// The records that sit best *before* this one.
///
/// The other direction, and genuinely a different list. A record that follows
/// this one beautifully may lead into it badly, because every component that
/// depends on direction — tempo, level, structure, energy continuity — is
/// measured the other way round.
#[must_use]
pub fn what_precedes(
    into: &Candidate,
    among: &[Candidate],
    weights: Weights,
    limit: usize,
) -> Vec<Neighbour> {
    rank(
        among.iter().filter_map(|from| {
            affinity(from, into, weights).map(|found| found.with_track(from.id()))
        }),
        limit,
    )
}

impl Neighbour {
    /// The same judgement, attributed to the other record of the pair.
    ///
    /// [`what_precedes`] asks about the record *before* this one, so the
    /// neighbour it reports is the outgoing record rather than the incoming one.
    const fn with_track(mut self, track: TrackId) -> Self {
        self.track = track;
        self
    }
}

/// Sorts and truncates, deterministically.
fn rank(found: impl Iterator<Item = Neighbour>, limit: usize) -> Vec<Neighbour> {
    let mut ranked: Vec<Neighbour> = found.collect();
    ranked.sort_by(|left, right| {
        right
            .total
            .partial_cmp(&left.total)
            .unwrap_or(core::cmp::Ordering::Equal)
            .then_with(|| left.track.get().cmp(&right.track.get()))
    });
    ranked.truncate(limit.min(MAX_NEIGHBOURS));
    ranked
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        reason = "a test that cannot build its own fixture should fail loudly"
    )]

    use super::*;
    use crate::candidate::{MixPoint, MixPointRole};
    use prv_analysis::Confidence;
    use prv_harmony::{Key, PitchClass};
    use prv_time::{Frames, Tempo};

    /// An analysed record: a quiet intro and a quiet outro, as most have.
    ///
    /// The mix points matter. Without them every pairing's weakest component is
    /// structure — truthfully, since nothing is known about where to mix — and a
    /// test meaning to say something about tempo would be saying nothing.
    fn track(id: u64, bpm: f64, energy: f32) -> Candidate {
        Candidate::new(
            TrackId::new(id),
            Frames::new(48_000 * 300),
            Tempo::from_bpm(bpm).expect("a tempo"),
            energy,
        )
        .with_point(MixPoint::new(
            Frames::new(48_000 * 8),
            0.2,
            MixPointRole::Entry,
        ))
        .with_point(MixPoint::new(
            Frames::new(48_000 * 270),
            0.2,
            MixPointRole::Exit,
        ))
    }

    fn keyed(candidate: Candidate, tonic: PitchClass, minor: bool) -> Candidate {
        let key = if minor {
            Key::minor(tonic)
        } else {
            Key::major(tonic)
        };
        candidate.with_key(key, Confidence::CERTAIN)
    }

    #[test]
    fn a_record_is_never_its_own_neighbour() {
        let record = track(1, 128.0, 0.5);
        assert_eq!(affinity(&record, &record, Weights::DEFAULT), None);

        let library = vec![record.clone()];
        assert!(what_follows(&record, &library, Weights::DEFAULT, 8).is_empty());
    }

    #[test]
    fn a_closer_tempo_is_a_better_neighbour() {
        let from = track(1, 128.0, 0.5);
        let near = track(2, 129.0, 0.5);
        let far = track(3, 138.0, 0.5);

        let near_score = affinity(&from, &near, Weights::DEFAULT).expect("a neighbour");
        let far_score = affinity(&from, &far, Weights::DEFAULT).expect("a neighbour");
        assert!(near_score.score() > far_score.score());
        assert_eq!(far_score.weakest(), Component::Tempo);
    }

    #[test]
    fn keys_that_clash_are_not_neighbours_at_all() {
        // A musical fact rather than a preference. A list that ranked
        // unlistenable moves at the bottom would be a list nobody could trust
        // the top of.
        let from = keyed(track(1, 128.0, 0.5), PitchClass::C, false);
        let clashing = keyed(track(2, 128.0, 0.5), PitchClass::CSharp, false);

        assert_eq!(affinity(&from, &clashing, Weights::DEFAULT), None);
        assert!(what_follows(&from, &[clashing], Weights::DEFAULT, 8).is_empty());
    }

    #[test]
    fn an_unknown_key_is_not_treated_as_a_clash() {
        // Most of a library, on the day somebody imports it.
        let from = keyed(track(1, 128.0, 0.5), PitchClass::C, false);
        let unanalysed = track(2, 128.0, 0.5);

        let found = affinity(&from, &unanalysed, Weights::DEFAULT).expect("a neighbour");
        assert!(found.components().harmonic > 0.0);
    }

    #[test]
    fn the_two_directions_are_different_questions() {
        // Every component that depends on direction is measured the other way
        // round, so a record that follows this one well may lead into it badly.
        let quiet = track(1, 128.0, 0.3).with_loudness(-20.0);
        let loud = track(2, 128.0, 0.9).with_loudness(-6.0);

        let forwards = affinity(&quiet, &loud, Weights::DEFAULT).expect("a neighbour");
        let backwards = affinity(&loud, &quiet, Weights::DEFAULT).expect("a neighbour");

        assert!(
            (forwards.score() - backwards.score()).abs() > f32::EPSILON,
            "affinity was symmetric, which it must not be"
        );
        // Coming down in energy costs more than going up: a room notices a drop.
        assert!(forwards.components().energy > backwards.components().energy);
    }

    #[test]
    fn what_follows_and_what_precedes_report_the_other_record() {
        let subject = track(1, 128.0, 0.5);
        let other = track(2, 128.0, 0.5);

        let after = what_follows(&subject, core::slice::from_ref(&other), Weights::DEFAULT, 8);
        let before = what_precedes(&subject, core::slice::from_ref(&other), Weights::DEFAULT, 8);

        assert_eq!(after[0].track(), TrackId::new(2));
        assert_eq!(before[0].track(), TrackId::new(2), "the wrong record named");
    }

    #[test]
    fn the_same_library_always_gives_the_same_answer() {
        let from = track(1, 128.0, 0.5);
        // Two records that score identically, so only the tiebreak separates
        // them. A shortlist that reshuffled between identical questions would be
        // worse than no shortlist.
        let library = vec![track(9, 128.0, 0.5), track(4, 128.0, 0.5)];

        let first = what_follows(&from, &library, Weights::DEFAULT, 8);
        let reversed: Vec<Candidate> = library.into_iter().rev().collect();
        let second = what_follows(&from, &reversed, Weights::DEFAULT, 8);

        assert_eq!(first, second);
        assert_eq!(first[0].track(), TrackId::new(4));
    }

    #[test]
    fn the_list_is_bounded_however_much_is_asked_for() {
        let from = track(1, 128.0, 0.5);
        let library: Vec<Candidate> = (2..500).map(|id| track(id, 128.0, 0.5)).collect();

        assert_eq!(
            what_follows(&from, &library, Weights::DEFAULT, 10).len(),
            10
        );
        assert_eq!(
            what_follows(&from, &library, Weights::DEFAULT, usize::MAX).len(),
            MAX_NEIGHBOURS
        );
    }

    #[test]
    fn the_best_neighbour_of_a_mixed_library_is_the_one_a_dj_would_pick() {
        // The whole module, end to end: a record at 128 in A minor, and a
        // library holding one obvious answer and several plausible distractions.
        let from = keyed(track(1, 128.0, 0.6), PitchClass::A, true).with_loudness(-8.0);
        let library = vec![
            // Same key, same tempo, same level — the obvious answer.
            keyed(track(2, 128.0, 0.62), PitchClass::A, true).with_loudness(-8.0),
            // Right key, wrong tempo.
            keyed(track(3, 140.0, 0.6), PitchClass::A, true).with_loudness(-8.0),
            // Right tempo, distant key.
            keyed(track(4, 128.0, 0.6), PitchClass::DSharp, true).with_loudness(-8.0),
            // Right everything, far too quiet.
            keyed(track(5, 128.0, 0.6), PitchClass::A, true).with_loudness(-24.0),
            // Right everything, far too low in energy.
            keyed(track(6, 128.0, 0.1), PitchClass::A, true).with_loudness(-8.0),
        ];

        let ranked = what_follows(&from, &library, Weights::DEFAULT, 8);
        assert_eq!(ranked[0].track(), TrackId::new(2));
        assert!(ranked[0].score() > 0.8);

        // And each of the distractions is weak for the reason it was built to be
        // weak, which is what makes the explanation worth showing.
        let weakest = |id: u64| {
            ranked
                .iter()
                .find(|found| found.track() == TrackId::new(id))
                .expect("a neighbour")
                .weakest()
        };
        assert_eq!(weakest(3), Component::Tempo);
        assert_eq!(weakest(5), Component::Level);
        assert_eq!(weakest(6), Component::Energy);
    }

    #[test]
    fn no_plan_scores_a_pair_higher_on_tempo_than_affinity_does() {
        // Affinity judges tempo at the widest tolerance any setting allows, so
        // it is an upper bound: a tighter allowance can only cost more. A
        // shortlist that promised more than a set could deliver would be the
        // worse mistake.
        let from = track(1, 128.0, 0.5);
        let to = track(2, 132.0, 0.5);
        let found = affinity(&from, &to, Weights::DEFAULT).expect("a neighbour");

        // From the candidates' own tempi, not from the numbers they were built
        // with. A tempo is stored as microseconds per beat, so 132 beats per
        // minute comes back as very nearly 132 — and a test that compared the
        // ideal against the stored value would be measuring that rounding
        // rather than the property it means to check.
        let change = narrow((to.tempo().bpm() - from.tempo().bpm()) / from.tempo().bpm());
        for creativity in [
            Creativity::Conservative,
            Creativity::Balanced,
            Creativity::Adventurous,
        ] {
            let planned = tempo_component(change, creativity.max_tempo_change());
            assert!(
                found.components().tempo >= planned,
                "{creativity:?} scored a pair's tempo above its upper bound"
            );
        }
    }
}
