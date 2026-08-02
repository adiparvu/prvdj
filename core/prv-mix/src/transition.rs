//! Scoring one move from one track to the next.
//!
//! # Constraints are not low scores
//!
//! ADR-0006 draws a line the rest of this module is built around: a candidate
//! that violates a hard constraint *is not a candidate*. It is not ranked last;
//! it is not generated. That is what makes Master Prompt #3B's safety rules
//! guarantees rather than tendencies, and it is why [`Rejection`] is a separate
//! type from a score rather than a score of zero.
//!
//! The distinction has a practical edge. A scoring system where everything is
//! comparable will, under enough pressure — a short library, a long set, a
//! demanding energy curve — eventually surface the least-bad clash. A system
//! where clashes are not in the candidate set cannot, however desperate the
//! search becomes.
//!
//! # The evidence is the explanation
//!
//! Every score is kept with its components. ADR-0006 requires the explanation
//! shown to a user to be a *rendering of the actual arithmetic* rather than a
//! story written afterwards, and [`TransitionScore`] is that arithmetic,
//! retained. Nothing downstream is permitted to introduce a claim that is not
//! in it.

use prv_harmony::{compatibility, HarmonicCompatibility, HarmonicSafety};

use crate::candidate::Candidate;
use crate::goal::Goal;
use crate::num::narrow;

/// Why a move was not generated.
///
/// Each variant names what was violated so an interface can say "no track in
/// your library is within 6 per cent of 174 BPM" rather than "no results".
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum Rejection {
    /// The keys clash. Never generated, at any creativity setting.
    ClashingKeys {
        /// How the keys relate.
        relation: prv_harmony::KeyRelation,
    },
    /// The harmonic move is risky and the creativity setting does not allow it.
    RiskyHarmonyNotAllowed {
        /// How the keys relate.
        relation: prv_harmony::KeyRelation,
    },
    /// The tempo change exceeds what the creativity setting permits.
    TempoChangeTooLarge {
        /// The change, as a fraction of the outgoing tempo.
        fraction: f32,
        /// The largest change allowed.
        allowed: f32,
    },
    /// The incoming track falls outside the tempo range the goal specified.
    OutsideTempoRange {
        /// The track's tempo, in beats per minute.
        bpm: f32,
    },
    /// A track cannot follow itself.
    SameTrack,
}

/// The components of a transition's score.
///
/// Kept separately rather than summed on the spot, because the components are
/// the explanation. Each is zero to one, higher being better.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScoreComponents {
    /// How well the keys sit together.
    pub harmonic: f32,
    /// How little the tempo has to move.
    pub tempo: f32,
    /// How closely the incoming track's energy matches what the set wants next.
    pub energy: f32,
    /// Whether both tracks offer a place for the transition to happen.
    pub structure: f32,
    /// How little the level has to jump.
    pub level: f32,
    /// How unlikely a vocal collision is.
    pub vocal: f32,
}

/// How much each component counts toward a transition's total.
///
/// # Why this is a value rather than a constant
///
/// ADR-0006 says the weights come from the scenario *and from the user's
/// learned profile*. A constant cannot do the second: two DJs disagree about
/// how much a slightly wrong energy matters, and the whole point of Master
/// Prompt #5 is that the system notices which one it is working for.
///
/// Making them a value also makes the default honest. [`Weights::DEFAULT`] is
/// where the provisional numbers live, in one place, with the argument for
/// their ordering — and a profile that has learned nothing yet returns exactly
/// them, so an untrained system behaves identically to one with no learning at
/// all.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Weights {
    /// How much harmonic compatibility counts.
    pub harmonic: f32,
    /// How much tempo distance counts.
    pub tempo: f32,
    /// How much energy fit counts.
    pub energy: f32,
    /// How much having somewhere to mix counts.
    pub structure: f32,
    /// How much level match counts.
    pub level: f32,
    /// How much vocal collision risk counts.
    pub vocal: f32,
}

impl Weights {
    /// The weights a system that has learned nothing uses.
    ///
    /// **Provisional**, like every weight in this system, and calibrated in
    /// Phase 3 as ADR-0006 schedules. The ordering they induce reflects what
    /// breaks a mix most visibly: a harmonic clash and a tempo the deck cannot
    /// reach are noticed by everyone, a level jump by most, a slightly wrong
    /// energy by a DJ.
    pub const DEFAULT: Self = Self {
        harmonic: 0.05,
        tempo: 0.10,
        energy: 0.10,
        structure: 0.20,
        level: 0.25,
        vocal: 0.30,
    };

    /// The smallest weight any component may be reduced to.
    ///
    /// A tenth of its default. Learning adjusts how much a component counts; it
    /// must never be able to switch one off, because a user who has never
    /// happened to reject a transition for a level jump has not told the system
    /// that level does not matter — they have told it nothing about level.
    pub const FLOOR: f32 = 0.1;

    /// The largest multiple of its default any component may reach.
    pub const CEILING: f32 = 3.0;

    /// The sum of every weight.
    #[must_use]
    pub fn total(&self) -> f32 {
        self.harmonic + self.tempo + self.energy + self.structure + self.level + self.vocal
    }

    /// Returns these weights with each component scaled, then bounded.
    ///
    /// The bounds are the whole safety argument. Learning changes how much
    /// something counts, never whether it counts, so no amount of observation
    /// can make the system stop caring about a clash — and the hard constraints
    /// are not weighted at all, so no amount of learning can reach them either.
    #[must_use]
    pub fn scaled(&self, by: Self) -> Self {
        let bound = |weight: f32, scale: f32, default: f32| -> f32 {
            if !weight.is_finite() || !scale.is_finite() {
                return default;
            }
            (weight * scale).clamp(default * Self::FLOOR, default * Self::CEILING)
        };
        Self {
            harmonic: bound(self.harmonic, by.harmonic, Self::DEFAULT.harmonic),
            tempo: bound(self.tempo, by.tempo, Self::DEFAULT.tempo),
            energy: bound(self.energy, by.energy, Self::DEFAULT.energy),
            structure: bound(self.structure, by.structure, Self::DEFAULT.structure),
            level: bound(self.level, by.level, Self::DEFAULT.level),
            vocal: bound(self.vocal, by.vocal, Self::DEFAULT.vocal),
        }
    }
}

impl Default for Weights {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl ScoreComponents {
    /// The weighted total under the default weights, from zero to one.
    #[must_use]
    pub fn total(&self) -> f32 {
        self.total_with(Weights::DEFAULT)
    }

    /// The weighted total under given weights, from zero to one.
    #[must_use]
    pub fn total_with(&self, weights: Weights) -> f32 {
        let sum = f64::from(self.harmonic) * f64::from(weights.harmonic)
            + f64::from(self.tempo) * f64::from(weights.tempo)
            + f64::from(self.energy) * f64::from(weights.energy)
            + f64::from(self.structure) * f64::from(weights.structure)
            + f64::from(self.level) * f64::from(weights.level)
            + f64::from(self.vocal) * f64::from(weights.vocal);
        let total_weight = f64::from(weights.total());
        if total_weight <= 0.0 {
            return 0.0;
        }
        narrow((sum / total_weight).clamp(0.0, 1.0))
    }

    /// The component that contributed least, which is what an explanation
    /// should lead with when a score is low.
    #[must_use]
    pub fn weakest(&self) -> Component {
        let entries = [
            (Component::Harmonic, self.harmonic),
            (Component::Tempo, self.tempo),
            (Component::Energy, self.energy),
            (Component::Structure, self.structure),
            (Component::Level, self.level),
            (Component::Vocal, self.vocal),
        ];
        let mut worst = Component::Harmonic;
        let mut lowest = f32::INFINITY;
        for (component, value) in entries {
            if value < lowest {
                lowest = value;
                worst = component;
            }
        }
        worst
    }
}

/// Names a score component, for explanations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Component {
    /// Harmonic compatibility.
    Harmonic,
    /// Tempo distance.
    Tempo,
    /// Energy fit.
    Energy,
    /// Availability of a place to mix.
    Structure,
    /// Level match.
    Level,
    /// Vocal collision risk.
    Vocal,
}

impl Component {
    /// A stable identifier.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Harmonic => "component.harmonic",
            Self::Tempo => "component.tempo",
            Self::Energy => "component.energy",
            Self::Structure => "component.structure",
            Self::Level => "component.level",
            Self::Vocal => "component.vocal",
        }
    }
}

/// A permitted move, with the evidence for it.
#[derive(Debug, Clone, PartialEq)]
pub struct TransitionScore {
    components: ScoreComponents,
    total: f32,
    harmony: Option<HarmonicCompatibility>,
    tempo_change: f32,
    level_change: f32,
}

impl TransitionScore {
    /// The individual components.
    #[must_use]
    pub const fn components(&self) -> ScoreComponents {
        self.components
    }

    /// The weighted total.
    #[must_use]
    pub const fn total(&self) -> f32 {
        self.total
    }

    /// The harmonic relation and its safety, when both keys were known.
    ///
    /// `None` means at least one track's key was not detected. The planner
    /// treats that as unconstrained rather than compatible, and an explanation
    /// should say so rather than omitting the harmony line — a user reading
    /// "keys: —" learns something true, and a user reading nothing assumes it
    /// was checked.
    #[must_use]
    pub const fn harmony(&self) -> Option<HarmonicCompatibility> {
        self.harmony
    }

    /// The tempo change, as a signed fraction of the outgoing tempo.
    #[must_use]
    pub const fn tempo_change(&self) -> f32 {
        self.tempo_change
    }

    /// The level change, in decibels.
    #[must_use]
    pub const fn level_change(&self) -> f32 {
        self.level_change
    }
}

/// Scores the move from `from` to `to`, or says why it is not a candidate.
///
/// `target_energy` is what the set wants next, from [`Goal::target_energy`].
///
/// # Errors
///
/// Returns [`Rejection`] when a hard constraint is violated. This is not a
/// failure to report to a user as an error; it is the search discovering that a
/// move is not available, which happens millions of times in a normal plan.
pub fn score(
    from: &Candidate,
    to: &Candidate,
    goal: &Goal,
    target_energy: f32,
) -> Result<TransitionScore, Rejection> {
    if from.id() == to.id() {
        return Err(Rejection::SameTrack);
    }

    let to_bpm = narrow(to.tempo().bpm());
    if let Some(floor) = goal.tempo_floor() {
        if to_bpm < floor {
            return Err(Rejection::OutsideTempoRange { bpm: to_bpm });
        }
    }
    if let Some(ceiling) = goal.tempo_ceiling() {
        if to_bpm > ceiling {
            return Err(Rejection::OutsideTempoRange { bpm: to_bpm });
        }
    }

    let from_bpm = from.tempo().bpm();
    let tempo_change = if from_bpm > 0.0 {
        narrow((to.tempo().bpm() - from_bpm) / from_bpm)
    } else {
        0.0
    };
    let allowed = goal.creativity().max_tempo_change();
    if tempo_change.abs() > allowed {
        return Err(Rejection::TempoChangeTooLarge {
            fraction: tempo_change,
            allowed,
        });
    }

    let harmony = match (from.key(), to.key()) {
        (Some(left), Some(right)) => {
            let result = compatibility(left, right);
            match result.safety {
                HarmonicSafety::Clashing => {
                    return Err(Rejection::ClashingKeys {
                        relation: result.relation,
                    })
                }
                HarmonicSafety::Risky if !goal.creativity().allows_risky_harmony() => {
                    return Err(Rejection::RiskyHarmonyNotAllowed {
                        relation: result.relation,
                    })
                }
                HarmonicSafety::Risky | HarmonicSafety::Safe => Some(result),
            }
        }
        _ => None,
    };

    let components = ScoreComponents {
        harmonic: harmonic_component(harmony, from, to),
        tempo: tempo_component(tempo_change, allowed),
        energy: energy_component(to.energy(), target_energy),
        structure: structure_component(from, to),
        level: level_component(from, to),
        vocal: vocal_component(from, to),
    };

    Ok(TransitionScore {
        total: components.total_with(goal.weights()),
        components,
        harmony,
        tempo_change,
        level_change: narrow(f64::from(to.loudness_lufs()) - f64::from(from.loudness_lufs())),
    })
}

/// How well the keys sit together.
///
/// When a key is unknown the component is *neutral*, not perfect and not zero.
/// Scoring it as perfect would make undetected tracks preferred over correctly
/// analysed ones — the planner would learn to like the tracks it knows least
/// about. Scoring it as zero would bury them below genuinely poor matches. A
/// neutral value says what is true: this move was not evaluated harmonically.
///
/// A key detected with low confidence is discounted toward neutral in
/// proportion to that confidence, which is the only use of a confidence that
/// does not require a threshold.
fn harmonic_component(
    harmony: Option<HarmonicCompatibility>,
    from: &Candidate,
    to: &Candidate,
) -> f32 {
    /// What an unevaluated harmonic move is worth.
    const NEUTRAL: f64 = 0.5;

    let Some(result) = harmony else {
        return narrow(NEUTRAL);
    };
    let certainty = f64::from(from.key_confidence().and_then(to.key_confidence()).value());
    let measured = f64::from(result.score);
    narrow(NEUTRAL + (measured - NEUTRAL) * certainty)
}

/// How little the tempo has to move.
///
/// Linear in the fraction of the allowance used, so a move at the limit scores
/// zero and a move that needs no change scores one. Linear rather than a curve
/// because the underlying quantity — audible pitch shift — is itself roughly
/// linear in the fraction over this range.
fn tempo_component(change: f32, allowed: f32) -> f32 {
    if allowed <= 0.0 {
        return if change.abs() <= 0.0 { 1.0 } else { 0.0 };
    }
    narrow((1.0 - f64::from(change.abs()) / f64::from(allowed)).clamp(0.0, 1.0))
}

/// How closely the incoming track's energy matches what the set wants next.
///
/// The penalty is asymmetric, and the asymmetry is the point. Coming in a
/// little *above* the curve is a lift; coming in a little below is a drop, and
/// a room notices a drop far more than a lift. So undershooting costs twice
/// what overshooting does.
fn energy_component(energy: f32, target: f32) -> f32 {
    let difference = f64::from(energy) - f64::from(target);
    let penalty = if difference < 0.0 {
        -difference * 2.0
    } else {
        difference
    };
    narrow((1.0 - penalty).clamp(0.0, 1.0))
}

/// Whether both tracks offer somewhere for the transition to happen.
///
/// A track with a known quiet outro and one with a known quiet intro can be
/// mixed properly; two tracks that both run at full tilt from first sample to
/// last can only be cut. Both are legitimate, and one is much easier, so this
/// is a scored preference rather than a constraint.
fn structure_component(from: &Candidate, to: &Candidate) -> f32 {
    match (from.best_exit(), to.best_entry()) {
        (Some(exit), Some(entry)) => {
            // Quieter is better on both sides: the less arrangement is exposed
            // during the overlap, the less there is to collide.
            let exposure = (f64::from(exit.energy()) + f64::from(entry.energy())) / 2.0;
            narrow((1.0 - exposure).clamp(0.0, 1.0))
        }
        // One side known is better than neither, and neither is not a failure.
        (Some(_), None) | (None, Some(_)) => 0.4,
        (None, None) => 0.25,
    }
}

/// How little the level has to jump.
///
/// Three decibels is where a listener starts to hear a transition as a level
/// change rather than as a new track, so the component reaches zero there.
fn level_component(from: &Candidate, to: &Candidate) -> f32 {
    /// The level change, in decibels, at which the component reaches zero.
    const AUDIBLE_JUMP_DB: f64 = 3.0;

    let difference = (f64::from(to.loudness_lufs()) - f64::from(from.loudness_lufs())).abs();
    narrow((1.0 - difference / AUDIBLE_JUMP_DB).clamp(0.0, 1.0))
}

/// How unlikely a vocal collision is.
///
/// Two vocals over each other is the single most recognisable amateur mistake,
/// so two known vocal tracks score zero. An unknown scores in between rather
/// than clean: "we do not know" should cost a little, or the planner would
/// prefer tracks whose vocal status was never determined.
fn vocal_component(from: &Candidate, to: &Candidate) -> f32 {
    match (from.has_vocals(), to.has_vocals()) {
        (Some(true), Some(true)) => 0.0,
        (Some(false), _) | (_, Some(false)) => 1.0,
        _ => 0.6,
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
    use crate::candidate::{MixPoint, MixPointRole, TrackId};
    use crate::goal::{EnergyShape, Goal};
    use prv_analysis::Confidence;
    use prv_harmony::{Key, PitchClass};
    use prv_time::{Frames, SampleRate, Tempo};

    fn candidate(id: u64, bpm: f64, energy: f32) -> Candidate {
        Candidate::new(
            TrackId::new(id),
            Frames::new(44_100 * 300),
            Tempo::from_bpm(bpm).expect("valid"),
            energy,
        )
    }

    fn goal() -> Goal {
        Goal::new(
            Frames::new(44_100 * 3600),
            SampleRate::HZ_44100,
            EnergyShape::Arc,
        )
    }

    #[test]
    fn a_clashing_key_is_not_a_low_score_but_no_candidate_at_all() {
        // The property that makes Master Prompt #3B's safety rules guarantees
        // rather than tendencies. Under enough pressure a scoring system will
        // surface the least-bad clash; a system where clashes are not in the
        // candidate set cannot.
        let from =
            candidate(1, 128.0, 0.5).with_key(Key::major(PitchClass::C), Confidence::CERTAIN);
        let to =
            candidate(2, 128.0, 0.5).with_key(Key::major(PitchClass::FSharp), Confidence::CERTAIN);

        let result = score(&from, &to, &goal(), 0.5);
        assert!(
            matches!(result, Err(Rejection::ClashingKeys { .. })),
            "a tritone apart was scored rather than rejected: {result:?}"
        );

        // And it stays rejected at the boldest setting, which is what "never
        // generated, at any creativity setting" has to mean.
        let bold = goal().with_creativity(crate::goal::Creativity::Adventurous);
        assert!(matches!(
            score(&from, &to, &bold, 0.5),
            Err(Rejection::ClashingKeys { .. })
        ));
    }

    #[test]
    fn creativity_unlocks_risky_harmony_and_nothing_beyond_it() {
        let from =
            candidate(1, 128.0, 0.5).with_key(Key::minor(PitchClass::A), Confidence::CERTAIN);
        // Two steps around the wheel: risky, not clashing.
        let to = candidate(2, 128.0, 0.5).with_key(Key::minor(PitchClass::D), Confidence::CERTAIN);

        let conservative = goal().with_creativity(crate::goal::Creativity::Conservative);
        let balanced = goal().with_creativity(crate::goal::Creativity::Balanced);

        let strict = score(&from, &to, &conservative, 0.5);
        let relaxed = score(&from, &to, &balanced, 0.5);
        // Whichever way `prv-harmony` classifies this pair, the two settings
        // must not disagree about a *clash*, only about a risk.
        if matches!(strict, Err(Rejection::RiskyHarmonyNotAllowed { .. })) {
            assert!(relaxed.is_ok(), "balanced did not unlock a risky move");
        } else {
            assert!(strict.is_ok() && relaxed.is_ok());
        }
    }

    #[test]
    fn a_tempo_jump_beyond_the_allowance_is_rejected_with_its_numbers() {
        let from = candidate(1, 128.0, 0.5);
        let to = candidate(2, 145.0, 0.5);
        match score(&from, &to, &goal(), 0.5) {
            Err(Rejection::TempoChangeTooLarge { fraction, allowed }) => {
                assert!(fraction > 0.12, "the reported change {fraction} is wrong");
                assert_eq!(
                    allowed,
                    crate::goal::Creativity::Balanced.max_tempo_change()
                );
            }
            other => panic!("expected a tempo rejection, got {other:?}"),
        }
    }

    #[test]
    fn a_tempo_range_in_the_goal_is_a_hard_constraint() {
        let from = candidate(1, 124.0, 0.5);
        let to = candidate(2, 127.0, 0.5);
        let restricted = goal().with_tempo_range(120.0, 126.0);
        assert!(matches!(
            score(&from, &to, &restricted, 0.5),
            Err(Rejection::OutsideTempoRange { .. })
        ));
        assert!(score(&from, &to, &goal(), 0.5).is_ok());
    }

    #[test]
    fn a_track_cannot_follow_itself() {
        let track = candidate(1, 128.0, 0.5);
        assert_eq!(
            score(&track, &track, &goal(), 0.5).err(),
            Some(Rejection::SameTrack)
        );
    }

    #[test]
    fn an_unknown_key_is_neutral_rather_than_perfect() {
        // The failure this prevents is subtle and would have been invisible: if
        // an unknown key scored perfectly, the planner would systematically
        // prefer the tracks it knows least about.
        let unknown_from = candidate(1, 128.0, 0.5);
        let unknown_to = candidate(2, 128.0, 0.5);
        let unknown = score(&unknown_from, &unknown_to, &goal(), 0.5).expect("permitted");

        let known_from =
            candidate(3, 128.0, 0.5).with_key(Key::minor(PitchClass::A), Confidence::CERTAIN);
        let known_to =
            candidate(4, 128.0, 0.5).with_key(Key::minor(PitchClass::A), Confidence::CERTAIN);
        let known = score(&known_from, &known_to, &goal(), 0.5).expect("permitted");

        assert!(
            known.components().harmonic > unknown.components().harmonic,
            "an identical-key move scored {} against an unknown pair's {}",
            known.components().harmonic,
            unknown.components().harmonic
        );
        assert!(unknown.harmony().is_none());
        assert!(known.harmony().is_some());
    }

    #[test]
    fn a_low_confidence_key_is_discounted_toward_neutral() {
        let from =
            candidate(1, 128.0, 0.5).with_key(Key::minor(PitchClass::A), Confidence::new(0.2));
        let to = candidate(2, 128.0, 0.5).with_key(Key::minor(PitchClass::A), Confidence::new(0.2));
        let uncertain = score(&from, &to, &goal(), 0.5).expect("permitted");

        let sure_from =
            candidate(3, 128.0, 0.5).with_key(Key::minor(PitchClass::A), Confidence::CERTAIN);
        let sure_to =
            candidate(4, 128.0, 0.5).with_key(Key::minor(PitchClass::A), Confidence::CERTAIN);
        let certain = score(&sure_from, &sure_to, &goal(), 0.5).expect("permitted");

        assert!(
            certain.components().harmonic > uncertain.components().harmonic,
            "confidence had no effect on the harmonic component"
        );
    }

    #[test]
    fn dropping_below_the_curve_costs_more_than_rising_above_it() {
        // The asymmetry a room actually hears. Two moves equally far from the
        // target, one under and one over, must not score the same.
        let from = candidate(1, 128.0, 0.5);
        let under = candidate(2, 128.0, 0.4);
        let over = candidate(3, 128.0, 0.6);

        let low = score(&from, &under, &goal(), 0.5).expect("permitted");
        let high = score(&from, &over, &goal(), 0.5).expect("permitted");
        assert!(
            high.components().energy > low.components().energy,
            "a drop scored {} and a lift {}; the asymmetry is missing",
            low.components().energy,
            high.components().energy
        );
    }

    #[test]
    fn two_vocal_tracks_score_worst_and_an_unknown_scores_between() {
        let base = candidate(1, 128.0, 0.5).with_vocals(true);
        let vocal = candidate(2, 128.0, 0.5).with_vocals(true);
        let instrumental = candidate(3, 128.0, 0.5).with_vocals(false);
        let unknown = candidate(4, 128.0, 0.5);

        let both = score(&base, &vocal, &goal(), 0.5).expect("permitted");
        let clean = score(&base, &instrumental, &goal(), 0.5).expect("permitted");
        let maybe = score(&base, &unknown, &goal(), 0.5).expect("permitted");

        assert_eq!(both.components().vocal, 0.0);
        assert_eq!(clean.components().vocal, 1.0);
        assert!(maybe.components().vocal > 0.0 && maybe.components().vocal < 1.0);
    }

    #[test]
    fn known_quiet_mix_points_score_better_than_none() {
        let plain_from = candidate(1, 128.0, 0.5);
        let plain_to = candidate(2, 128.0, 0.5);
        let plain = score(&plain_from, &plain_to, &goal(), 0.5).expect("permitted");

        let from = candidate(3, 128.0, 0.5).with_point(MixPoint::new(
            Frames::new(9_000_000),
            0.1,
            MixPointRole::Exit,
        ));
        let to = candidate(4, 128.0, 0.5).with_point(MixPoint::new(
            Frames::ZERO,
            0.1,
            MixPointRole::Entry,
        ));
        let structured = score(&from, &to, &goal(), 0.5).expect("permitted");

        assert!(
            structured.components().structure > plain.components().structure,
            "known mix points did not improve the structural score"
        );
    }

    #[test]
    fn the_total_is_the_weighted_sum_and_stays_in_range() {
        let components = ScoreComponents {
            harmonic: 1.0,
            tempo: 1.0,
            energy: 1.0,
            structure: 1.0,
            level: 1.0,
            vocal: 1.0,
        };
        assert!((components.total() - 1.0).abs() < 1e-6);

        let empty = ScoreComponents {
            harmonic: 0.0,
            tempo: 0.0,
            energy: 0.0,
            structure: 0.0,
            level: 0.0,
            vocal: 0.0,
        };
        assert_eq!(empty.total(), 0.0);
    }

    #[test]
    fn the_weakest_component_is_what_an_explanation_would_lead_with() {
        let components = ScoreComponents {
            harmonic: 0.9,
            tempo: 0.8,
            energy: 0.2,
            structure: 0.7,
            level: 0.9,
            vocal: 1.0,
        };
        assert_eq!(components.weakest(), Component::Energy);
    }

    #[test]
    fn a_level_jump_beyond_three_decibels_scores_zero() {
        let from = candidate(1, 128.0, 0.5).with_loudness(-14.0);
        let to = candidate(2, 128.0, 0.5).with_loudness(-6.0);
        let result = score(&from, &to, &goal(), 0.5).expect("permitted");
        assert_eq!(result.components().level, 0.0);
        assert!((result.level_change() - 8.0).abs() < 1e-4);
    }

    #[test]
    fn component_keys_are_distinct() {
        let keys = [
            Component::Harmonic.key(),
            Component::Tempo.key(),
            Component::Energy.key(),
            Component::Structure.key(),
            Component::Level.key(),
            Component::Vocal.key(),
        ];
        for (index, key) in keys.iter().enumerate() {
            for (other, value) in keys.iter().enumerate() {
                assert!(index == other || key != value, "two components share {key}");
            }
        }
    }
}
