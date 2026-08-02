//! What the system has learned about one user.
//!
//! # How a preference is inferred
//!
//! Each observation says what the system offered — six component scores — and
//! what the user did with it. A component earns weight when the user's
//! decisions *correlate* with it: if every transition they kept scored well on
//! harmony and every one they rejected scored badly on it, harmony is a thing
//! this user cares about, and the objective should count it for more.
//!
//! Correlation rather than average is the whole of it. A user who only ever
//! sees harmonically excellent suggestions and keeps them all has told the
//! system nothing about harmony — every score was high, so the high scores do
//! not distinguish the kept from the rejected. Averaging would read that as a
//! strong preference and would then over-weight the one component the user was
//! never given a choice about.
//!
//! # Learning is bounded, and the bound is the safety argument
//!
//! A learned weight can move a component's importance up or down within fixed
//! limits, and can never switch one off. Nothing here touches the *constraints*
//! — a clashing key is still not a candidate, at any profile, after any amount
//! of observation. Master Prompt #3B's safety rules are guarantees, and a
//! guarantee that a sufficiently unusual user could train away is not one.
//!
//! # Confidence gates the effect
//!
//! A preference inferred from four observations is a coincidence. The profile
//! therefore blends toward the default in proportion to how much evidence it
//! has, so a new user's first sets are planned exactly as an untrained system
//! would plan them and the influence arrives gradually. There is no moment at
//! which the recommendations change character.
//!
//! # The user owns this, and can see and delete it
//!
//! Master Prompt #5 requires the profile to be explainable and correctable;
//! Master Prompt #26 requires it to be the user's. [`Profile::explain`] returns
//! what was learned and how strongly, [`Profile::forget`] removes one inference,
//! and [`Profile::forget_all`] removes all of it. None of it leaves the device
//! unless something above this layer sends it, and nothing here is used for
//! training anything shared.

use prv_mix::{Component, Weights};

use crate::observation::{Observation, Outcome, COMPONENTS};

/// How many observations a component needs before its inference reaches full
/// strength.
///
/// Thirty-two. Below it the inference is blended toward the default in
/// proportion, so influence arrives gradually rather than switching on. The
/// number is provisional and the *shape* is not: whatever the threshold, a
/// profile must not change character at a single observation.
pub const FULL_CONFIDENCE_OBSERVATIONS: usize = 32;

/// The most observations a profile keeps.
///
/// Four thousand and ninety-six. A bound is required because this is a document
/// that grows with use (Master Prompt #26), and it also serves the purpose:
/// taste drifts, and a profile that remembers a user's first evening forever
/// would keep arguing with who they have become. The oldest are dropped first.
pub const MAX_OBSERVATIONS: usize = 4_096;

/// How strongly a component's correlation can move its weight.
///
/// A correlation of one — every kept suggestion scored high on this component
/// and every rejected one scored low — doubles the weight. Perfect
/// anti-correlation halves it. The bounds in [`Weights::scaled`] then apply on
/// top, so no sequence of observations can push a component out of its range.
const MAX_SCALE: f32 = 2.0;

/// What was learned about one component.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Inference {
    component: Component,
    correlation: f32,
    observations: usize,
}

impl Inference {
    /// Which component.
    #[must_use]
    pub const fn component(self) -> Component {
        self.component
    }

    /// How strongly the user's decisions tracked this component, from minus one
    /// to one.
    ///
    /// Positive means they kept suggestions that scored well on it. Negative
    /// means they kept ones that scored badly — which is real and worth
    /// reporting rather than clamping away: a DJ who deliberately mixes across
    /// keys is telling the system something true about themselves.
    #[must_use]
    pub const fn correlation(self) -> f32 {
        self.correlation
    }

    /// How many observations it rests on.
    #[must_use]
    pub const fn observations(self) -> usize {
        self.observations
    }

    /// How much of the inference is applied, from zero to one.
    ///
    /// This is the gate. It is exposed rather than kept private because an
    /// explanation that says "based on 6 of the 32 observations needed" is
    /// enormously more trustworthy than one that says "we have learned you
    /// prefer harmonic mixing".
    #[must_use]
    pub fn strength(self) -> f32 {
        if FULL_CONFIDENCE_OBSERVATIONS == 0 {
            return 1.0;
        }
        let ratio = crate::num::ratio(self.observations, FULL_CONFIDENCE_OBSERVATIONS);
        crate::num::narrow(ratio.min(1.0))
    }

    /// The factor this inference applies to the component's default weight.
    #[must_use]
    pub fn scale(self) -> f32 {
        let effect = f64::from(self.correlation) * f64::from(self.strength());
        // A correlation of one doubles; minus one halves. Expressed as a power
        // so the two directions are symmetric in ratio rather than in
        // difference — halving and doubling are the same size of change to a
        // weight, and an additive form would make one of them larger.
        crate::num::narrow(f64::from(MAX_SCALE).powf(effect))
    }
}

/// Everything the system has learned about one user.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Profile {
    observations: Vec<Observation>,
    forgotten: Vec<Component>,
}

impl Profile {
    /// Creates a profile that has learned nothing.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            observations: Vec::new(),
            forgotten: Vec::new(),
        }
    }

    /// Records what the user did.
    ///
    /// Observations beyond [`MAX_OBSERVATIONS`] drop the oldest. Taste drifts,
    /// and a profile that remembered a user's first evening forever would keep
    /// arguing with who they have become.
    pub fn observe(&mut self, observation: Observation) {
        self.observations.push(observation);
        if self.observations.len() > MAX_OBSERVATIONS {
            let excess = self.observations.len() - MAX_OBSERVATIONS;
            self.observations.drain(0..excess);
        }
    }

    /// How many observations the profile holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.observations.len()
    }

    /// Whether the profile has learned anything.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.observations.is_empty()
    }

    /// What was learned about each component, strongest inference first.
    ///
    /// This is what an interface shows when a user asks what the system thinks
    /// it knows about them, and it is ordered so that the answer leads with the
    /// thing that actually changes their sets.
    #[must_use]
    pub fn explain(&self) -> Vec<Inference> {
        let mut inferences: Vec<Inference> = COMPONENTS
            .iter()
            .filter(|component| !self.forgotten.contains(component))
            .map(|&component| self.infer(component))
            .collect();

        // Ordered by how much the inference actually moves the weight, then by
        // the component itself so the order is total and the same every run.
        inferences.sort_by(|a, b| {
            let left = (f64::from(a.correlation) * f64::from(a.strength())).abs();
            let right = (f64::from(b.correlation) * f64::from(b.strength())).abs();
            right
                .partial_cmp(&left)
                .unwrap_or(core::cmp::Ordering::Equal)
                .then_with(|| a.component.key().cmp(b.component.key()))
        });
        inferences
    }

    /// The weights this profile produces.
    ///
    /// A profile that has learned nothing returns [`Weights::DEFAULT`] exactly,
    /// which is what makes it safe to ship learning switched on from the first
    /// day: an untrained system is indistinguishable from one with no learning.
    #[must_use]
    pub fn weights(&self) -> Weights {
        self.weights_from(Weights::DEFAULT)
    }

    /// The weights this profile produces from a given starting point.
    ///
    /// The starting point is the scenario's weights: ADR-0006 says the two
    /// sources combine, and this is where. The scenario says a wedding cares
    /// less about tempo than a festival; the profile says *this* user cares
    /// more about harmony than most.
    #[must_use]
    pub fn weights_from(&self, base: Weights) -> Weights {
        let scale = Weights {
            harmonic: self.scale_for(Component::Harmonic),
            tempo: self.scale_for(Component::Tempo),
            energy: self.scale_for(Component::Energy),
            structure: self.scale_for(Component::Structure),
            level: self.scale_for(Component::Level),
            vocal: self.scale_for(Component::Vocal),
        };
        base.scaled(scale)
    }

    /// Removes what was learned about one component.
    ///
    /// The observations stay — they are evidence about the others too — but this
    /// component stops being inferred from them. Master Prompt #5 requires the
    /// profile to be correctable, and "you are wrong about this one thing" is
    /// the correction a user actually wants to make.
    pub fn forget(&mut self, component: Component) {
        if !self.forgotten.contains(&component) {
            self.forgotten.push(component);
            self.forgotten.sort_by_key(|entry| entry.key());
        }
    }

    /// Restores a component that was forgotten.
    pub fn remember(&mut self, component: Component) {
        self.forgotten.retain(|entry| *entry != component);
    }

    /// Whether a component is currently being inferred.
    #[must_use]
    pub fn is_forgotten(&self, component: Component) -> bool {
        self.forgotten.contains(&component)
    }

    /// Discards everything.
    ///
    /// Master Prompt #26 requires the user to be able to delete what the system
    /// holds about them, and a profile that could only be *reduced* would not
    /// satisfy that. Afterwards the profile is indistinguishable from a new one.
    pub fn forget_all(&mut self) {
        self.observations.clear();
        self.forgotten.clear();
    }

    /// The factor a component's weight is multiplied by.
    fn scale_for(&self, component: Component) -> f32 {
        if self.forgotten.contains(&component) {
            return 1.0;
        }
        self.infer(component).scale()
    }

    /// What the observations say about one component.
    ///
    /// The correlation between the component's score and how the user reacted.
    /// Both sides are centred on their own mean first, which is what makes this
    /// measure *distinguishing power* rather than level: a component that
    /// scored 0.9 on everything the user ever saw correlates with nothing,
    /// however good those scores were.
    fn infer(&self, component: Component) -> Inference {
        // Ignored outcomes carry no valence and are excluded outright rather
        // than folded in as zeros, which would drag every correlation toward
        // nothing in proportion to how much the user scrolled past.
        let considered: Vec<&Observation> = self
            .observations
            .iter()
            .filter(|observation| observation.outcome() != Outcome::Ignored)
            .collect();

        let count = considered.len();
        if count < 2 {
            return Inference {
                component,
                correlation: 0.0,
                observations: count,
            };
        }

        let total = crate::num::count_to_f64(count);
        let score_mean = considered
            .iter()
            .map(|observation| f64::from(observation.component(component)))
            .sum::<f64>()
            / total;
        let valence_mean = considered
            .iter()
            .map(|observation| f64::from(observation.outcome().valence()))
            .sum::<f64>()
            / total;

        let mut covariance = 0.0_f64;
        let mut score_variance = 0.0_f64;
        let mut valence_variance = 0.0_f64;
        for observation in &considered {
            let score = f64::from(observation.component(component)) - score_mean;
            let valence = f64::from(observation.outcome().valence()) - valence_mean;
            covariance += score * valence;
            score_variance += score * score;
            valence_variance += valence * valence;
        }

        let denominator = (score_variance * valence_variance).sqrt();
        let correlation = if denominator <= 0.0 {
            // Either every score was identical or every outcome was. In both
            // cases the user has been shown no contrast, and the honest answer
            // is that nothing was learned — not that the correlation is
            // undefined and can therefore be anything.
            0.0
        } else {
            (covariance / denominator).clamp(-1.0, 1.0)
        };

        Inference {
            component,
            correlation: crate::num::narrow(correlation),
            observations: count,
        }
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
    use prv_mix::ScoreComponents;

    fn components(harmonic: f32, tempo: f32) -> ScoreComponents {
        ScoreComponents {
            harmonic,
            tempo,
            energy: 0.5,
            structure: 0.5,
            level: 0.5,
            vocal: 0.5,
        }
    }

    /// A user who keeps harmonically strong suggestions and rejects weak ones,
    /// while tempo varies independently of what they choose.
    fn harmonic_purist(observations: usize) -> Profile {
        let mut profile = Profile::new();
        for index in 0..observations {
            let keeps = index % 2 == 0;
            let harmonic = if keeps { 0.9 } else { 0.2 };
            // Tempo alternates on a different cycle, so it carries no
            // information about what the user chose.
            let tempo = if index % 3 == 0 { 0.9 } else { 0.3 };
            profile.observe(Observation::new(
                u64::try_from(index).unwrap_or(0),
                components(harmonic, tempo),
                if keeps {
                    Outcome::Kept
                } else {
                    Outcome::Rejected
                },
            ));
        }
        profile
    }

    #[test]
    fn a_profile_that_has_learned_nothing_changes_nothing() {
        // What makes it safe to ship learning switched on from the first day.
        let profile = Profile::new();
        assert!(profile.is_empty());
        assert_eq!(profile.weights(), Weights::DEFAULT);
    }

    #[test]
    fn a_preference_is_learned_from_what_distinguishes_choices() {
        let profile = harmonic_purist(64);
        let weights = profile.weights();

        assert!(
            weights.harmonic > Weights::DEFAULT.harmonic,
            "harmony was not learned: {} against a default of {}",
            weights.harmonic,
            Weights::DEFAULT.harmonic
        );

        let inferences = profile.explain();
        let leading = inferences.first().expect("something was inferred");
        assert_eq!(
            leading.component(),
            Component::Harmonic,
            "the explanation does not lead with what actually changed the weights"
        );
        assert!(leading.correlation() > 0.5);
        assert_eq!(leading.strength(), 1.0);
    }

    #[test]
    fn a_component_the_user_was_never_given_a_choice_about_is_not_learned() {
        // The reason for correlation rather than average. A user who only ever
        // saw harmonically excellent suggestions and kept them all has told the
        // system nothing about harmony — averaging would read it as a strong
        // preference and over-weight the one thing they had no say in.
        let mut profile = Profile::new();
        for index in 0..64 {
            let keeps = index % 2 == 0;
            profile.observe(Observation::new(
                index,
                // Harmony is always excellent; only tempo distinguishes.
                components(0.95, if keeps { 0.9 } else { 0.2 }),
                if keeps {
                    Outcome::Kept
                } else {
                    Outcome::Rejected
                },
            ));
        }

        let weights = profile.weights();
        assert_eq!(
            weights.harmonic,
            Weights::DEFAULT.harmonic,
            "a component with no contrast was still learned from"
        );
        assert!(
            weights.tempo > Weights::DEFAULT.tempo,
            "the component that actually distinguished the choices was not learned"
        );
    }

    #[test]
    fn influence_arrives_gradually_rather_than_switching_on() {
        // There must be no observation at which the recommendations change
        // character. This walks the whole run-up and checks the effect only
        // ever grows.
        let mut previous = Weights::DEFAULT.harmonic;
        for count in (2..=FULL_CONFIDENCE_OBSERVATIONS * 2).step_by(2) {
            let weights = harmonic_purist(count).weights();
            assert!(
                weights.harmonic >= previous - 1e-6,
                "influence went backwards at {count} observations"
            );
            // And no single step is a jump.
            assert!(
                weights.harmonic - previous < Weights::DEFAULT.harmonic * 0.25,
                "influence jumped at {count} observations"
            );
            previous = weights.harmonic;
        }
    }

    #[test]
    fn a_handful_of_observations_barely_moves_anything() {
        // A preference inferred from four observations is a coincidence.
        let barely = harmonic_purist(4).weights();
        let settled = harmonic_purist(FULL_CONFIDENCE_OBSERVATIONS * 2).weights();

        let small_move = f64::from(barely.harmonic - Weights::DEFAULT.harmonic);
        let full_move = f64::from(settled.harmonic - Weights::DEFAULT.harmonic);
        assert!(
            small_move < full_move * 0.35,
            "four observations moved the weight {small_move}, against {full_move} when settled"
        );
    }

    #[test]
    fn no_amount_of_observation_can_switch_a_component_off() {
        // The safety argument. Learning changes how much something counts,
        // never whether it counts — a user who has never happened to reject a
        // transition over a level jump has told the system nothing about level,
        // not that level does not matter.
        let mut profile = Profile::new();
        for index in 0..2_000 {
            let keeps = index % 2 == 0;
            profile.observe(Observation::new(
                index,
                ScoreComponents {
                    harmonic: 0.5,
                    tempo: 0.5,
                    energy: 0.5,
                    structure: 0.5,
                    // Perfectly anti-correlated: the user keeps everything with
                    // a bad level match and rejects everything with a good one.
                    level: if keeps { 0.0 } else { 1.0 },
                    vocal: 0.5,
                },
                if keeps {
                    Outcome::Kept
                } else {
                    Outcome::Rejected
                },
            ));
        }

        let weights = profile.weights();
        assert!(
            weights.level >= Weights::DEFAULT.level * Weights::FLOOR - 1e-6,
            "level was pushed below its floor: {}",
            weights.level
        );
        assert!(weights.level > 0.0, "a component was switched off entirely");
        assert!(
            weights.level < Weights::DEFAULT.level,
            "an anti-correlation should still reduce the weight"
        );
    }

    #[test]
    fn an_ignored_suggestion_teaches_nothing() {
        // Counting silence as rejection is how a recommender talks itself into
        // a narrower and narrower corner.
        let mut with_noise = harmonic_purist(64);
        let learned = with_noise.weights();
        for index in 1_000..1_200 {
            with_noise.observe(Observation::new(
                index,
                components(0.1, 0.1),
                Outcome::Ignored,
            ));
        }
        assert_eq!(
            with_noise.weights(),
            learned,
            "ignored suggestions changed what was learned"
        );
    }

    #[test]
    fn the_user_can_correct_one_inference_and_delete_all_of_them() {
        // Master Prompt #5 requires the profile to be correctable and Master
        // Prompt #26 requires it to be deletable. A profile that could only be
        // reduced would satisfy neither.
        let mut profile = harmonic_purist(64);
        assert!(profile.weights().harmonic > Weights::DEFAULT.harmonic);

        profile.forget(Component::Harmonic);
        assert!(profile.is_forgotten(Component::Harmonic));
        assert_eq!(
            profile.weights().harmonic,
            Weights::DEFAULT.harmonic,
            "forgetting a component did not stop it being applied"
        );
        assert!(
            !profile
                .explain()
                .iter()
                .any(|inference| inference.component() == Component::Harmonic),
            "a forgotten component still appears in the explanation"
        );
        assert!(
            !profile.is_empty(),
            "forgetting one inference should not discard the evidence about the others"
        );

        profile.remember(Component::Harmonic);
        assert!(profile.weights().harmonic > Weights::DEFAULT.harmonic);

        profile.forget_all();
        assert!(profile.is_empty());
        assert_eq!(
            profile.weights(),
            Weights::DEFAULT,
            "a cleared profile is not indistinguishable from a new one"
        );
    }

    #[test]
    fn a_profile_cannot_grow_without_bound_and_forgets_the_oldest_first() {
        let mut profile = Profile::new();
        for index in 0..(MAX_OBSERVATIONS + 500) {
            profile.observe(Observation::new(
                u64::try_from(index).unwrap_or(0),
                components(0.5, 0.5),
                Outcome::Kept,
            ));
        }
        assert_eq!(profile.len(), MAX_OBSERVATIONS);
    }

    #[test]
    fn learning_starts_from_the_scenario_rather_than_replacing_it() {
        // ADR-0006 says the two sources combine. The scenario says a wedding
        // cares less about tempo than a festival; the profile says this user
        // cares more about harmony than most.
        let scenario = Weights {
            tempo: Weights::DEFAULT.tempo * 0.5,
            ..Weights::DEFAULT
        };
        let combined = harmonic_purist(64).weights_from(scenario);

        assert!(
            combined.tempo < Weights::DEFAULT.tempo,
            "the scenario's adjustment was discarded"
        );
        assert!(
            combined.harmonic > scenario.harmonic,
            "the profile's adjustment was discarded"
        );
    }

    #[test]
    fn a_profile_with_one_observation_infers_nothing() {
        // A correlation needs two points. Reporting one from a single
        // observation would be reporting the sign of a coincidence.
        let mut profile = Profile::new();
        profile.observe(Observation::new(0, components(0.9, 0.1), Outcome::Kept));
        assert_eq!(profile.weights(), Weights::DEFAULT);
        for inference in profile.explain() {
            assert_eq!(inference.correlation(), 0.0);
        }
    }

    #[test]
    fn learning_is_reproducible_and_explanations_are_ordered_stably() {
        let first = harmonic_purist(50);
        let second = harmonic_purist(50);
        assert_eq!(first.weights(), second.weights());

        let left: Vec<Component> = first
            .explain()
            .into_iter()
            .map(Inference::component)
            .collect();
        let right: Vec<Component> = second
            .explain()
            .into_iter()
            .map(Inference::component)
            .collect();
        assert_eq!(left, right);
        assert_eq!(left.len(), COMPONENTS.len());
    }
}
