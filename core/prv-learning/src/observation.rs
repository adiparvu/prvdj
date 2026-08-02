//! What the user did, recorded as evidence.
//!
//! # Behaviour, not opinions
//!
//! Master Prompt #5 asks the system to learn a user's taste. The tempting
//! implementation asks them: a preferences screen with six sliders. It does not
//! work, for a reason worth stating — people are poor at introspecting about
//! taste and good at exercising it. A DJ who would tell you they never cut
//! between records cuts between records all evening when the harmony is wrong.
//!
//! So the profile is built from what was *done*: which suggestion was kept,
//! which was replaced, which transition was edited afterwards. Each of those is
//! an act with a clear meaning, which is what makes the learned result something
//! the system can explain back.
//!
//! # No clock, deliberately
//!
//! ADR-0001 keeps the core free of input, output and operating-system calls,
//! and that includes reading the time. An observation therefore carries a
//! caller-supplied ordinal rather than a timestamp — the same discipline the
//! operation log uses for logical time, and for the same reason: an ordering
//! the core can reason about without asking the platform anything.
//!
//! It also happens to be better for the purpose. Recency should be measured in
//! *decisions made*, not in days elapsed: a user who has not opened the
//! application for a month has not changed their taste, and a user who has
//! planned forty sets this afternoon has told the system a great deal.
//!
//! # Nothing here identifies anyone
//!
//! An observation holds component scores and an outcome. It does not hold a
//! track name, an artist, a file path or a device. Master Prompt #26 requires
//! privacy by design, and the cheapest way to keep a profile from becoming
//! personal data is for it never to contain any.

use prv_mix::{Component, ScoreComponents};

/// What the user did with something the system proposed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Outcome {
    /// Kept as offered, and played.
    ///
    /// The strongest positive signal available, and the rarest: most
    /// suggestions are adjusted before they are used.
    Kept,

    /// Kept after being adjusted.
    ///
    /// Weakly positive about the choice and *informative about what was wrong*,
    /// which is often more useful than either a clean acceptance or a rejection.
    Adjusted,

    /// Replaced with something else.
    Rejected,

    /// Offered and never acted on.
    ///
    /// Deliberately recorded and deliberately weighted at nothing. A suggestion
    /// the user scrolled past tells us they did not choose it, which is not the
    /// same as not wanting it — they may never have seen it. Counting silence
    /// as rejection is how a recommender talks itself into a narrower and
    /// narrower corner.
    Ignored,
}

impl Outcome {
    /// How strongly this outcome argues for the choice, from minus one to one.
    #[must_use]
    pub const fn valence(self) -> f32 {
        match self {
            Self::Kept => 1.0,
            Self::Adjusted => 0.25,
            Self::Rejected => -1.0,
            Self::Ignored => 0.0,
        }
    }

    /// A stable identifier, for storage and localisation.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Kept => "outcome.kept",
            Self::Adjusted => "outcome.adjusted",
            Self::Rejected => "outcome.rejected",
            Self::Ignored => "outcome.ignored",
        }
    }
}

/// One thing the user did, with the evidence the system had at the time.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Observation {
    ordinal: u64,
    components: ScoreComponents,
    outcome: Outcome,
}

impl Observation {
    /// Records an outcome.
    ///
    /// `ordinal` is a caller-supplied sequence number, increasing over the life
    /// of a profile. It is what recency is measured in.
    #[must_use]
    pub const fn new(ordinal: u64, components: ScoreComponents, outcome: Outcome) -> Self {
        Self {
            ordinal,
            components,
            outcome,
        }
    }

    /// Where this sits in the sequence of decisions.
    #[must_use]
    pub const fn ordinal(self) -> u64 {
        self.ordinal
    }

    /// The evidence the system had when it proposed this.
    #[must_use]
    pub const fn components(self) -> ScoreComponents {
        self.components
    }

    /// What the user did.
    #[must_use]
    pub const fn outcome(self) -> Outcome {
        self.outcome
    }

    /// The value of one component of the evidence.
    #[must_use]
    pub fn component(self, component: Component) -> f32 {
        match component {
            Component::Harmonic => self.components.harmonic,
            Component::Tempo => self.components.tempo,
            Component::Energy => self.components.energy,
            Component::Structure => self.components.structure,
            Component::Level => self.components.level,
            Component::Vocal => self.components.vocal,
        }
    }
}

/// Every component, in one place, so that a caller iterating them cannot miss
/// one when a seventh is added.
pub const COMPONENTS: [Component; 6] = [
    Component::Harmonic,
    Component::Tempo,
    Component::Energy,
    Component::Structure,
    Component::Level,
    Component::Vocal,
];

#[cfg(test)]
mod tests {
    #![allow(clippy::float_cmp, reason = "these values are exact by construction")]

    use super::*;

    fn components() -> ScoreComponents {
        ScoreComponents {
            harmonic: 0.1,
            tempo: 0.2,
            energy: 0.3,
            structure: 0.4,
            level: 0.5,
            vocal: 0.6,
        }
    }

    #[test]
    fn an_ignored_suggestion_argues_for_nothing() {
        // The decision that keeps a recommender from narrowing. A suggestion
        // the user scrolled past tells us they did not choose it, which is not
        // the same as not wanting it — they may never have seen it.
        assert_eq!(Outcome::Ignored.valence(), 0.0);
        assert!(Outcome::Kept.valence() > 0.0);
        assert!(Outcome::Adjusted.valence() > 0.0);
        assert!(Outcome::Rejected.valence() < 0.0);
        assert!(
            Outcome::Adjusted.valence() < Outcome::Kept.valence(),
            "an adjusted suggestion should argue less strongly than an untouched one"
        );
    }

    #[test]
    fn every_component_is_readable_and_the_list_is_complete() {
        // A caller iterating the components must not be able to miss one when a
        // seventh is added, and the values must line up with the names.
        let observation = Observation::new(1, components(), Outcome::Kept);
        assert_eq!(observation.component(Component::Harmonic), 0.1);
        assert_eq!(observation.component(Component::Vocal), 0.6);
        assert_eq!(COMPONENTS.len(), 6);

        let mut seen = COMPONENTS.to_vec();
        seen.dedup();
        assert_eq!(seen.len(), COMPONENTS.len(), "a component is listed twice");
    }

    #[test]
    fn outcome_keys_are_distinct() {
        let keys = [
            Outcome::Kept.key(),
            Outcome::Adjusted.key(),
            Outcome::Rejected.key(),
            Outcome::Ignored.key(),
        ];
        for (index, key) in keys.iter().enumerate() {
            for (other, value) in keys.iter().enumerate() {
                assert!(index == other || key != value, "two outcomes share {key}");
            }
        }
    }
}
