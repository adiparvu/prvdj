//! What the system can do, and where each of those things happens.
//!
//! # No musical decision is ever generated
//!
//! ADR-0006 divides the system in one place: musical decisions are *computed*
//! under hard constraints, and language is used only to translate intent inward
//! and evidence outward. [`AgentKind::decides_musically`] marks the agents on
//! the computing side, and [`AgentKind::location`] puts every one of them on the
//! device. A test asserts the pairing, so an agent added later that both decided
//! musically and ran on a server would fail the build.
//!
//! This is not a preference about privacy, though it is good for privacy. It is
//! that a musical decision has to be reproducible, explainable, and the same
//! this evening as it was this afternoon, and a request to a model is none of
//! those things.
//!
//! # Losing the cloud costs fluency, never capability
//!
//! Every essential capability has an on-device agent. Withdraw every consent and
//! the product still analyses tracks, plans sets, explains its choices and
//! exports them — it simply stops being able to read a sentence and stops
//! writing its explanations in prose. There is a test that withdraws everything
//! and checks that every essential capability is still reachable, because
//! "graceful degradation" is a phrase that means nothing until something
//! measures it.

use core::fmt;

use prv_security::{Consents, ProcessingLocation, Purpose};

/// One thing the system can do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum AgentKind {
    /// Turn a written sentence into an intent, on a server.
    ///
    /// The only agent that reads what the user wrote, and the reason the
    /// consent for it is separate from the one for audio: a sentence may
    /// mention anything at all.
    LanguageInterpretation,

    /// Turn a form's answers into an intent, on the device.
    ///
    /// The alternative to the one above, and not a lesser one. The planner
    /// cannot tell which produced its goal.
    FormInterpretation,

    /// Measure tempo, key, structure and loudness, on the device.
    TrackAnalysis,

    /// Measure the same things on a server, for a library too large to work
    /// through locally in reasonable time.
    CloudTrackAnalysis,

    /// Search the library.
    LibrarySearch,

    /// Plan a set.
    ///
    /// Deterministic, on the device, always. This is the agent ADR-0006 is
    /// about.
    SetPlanning,

    /// Choose how to get from one record to the next.
    TransitionPlanning,

    /// Turn retained evidence into a sentence, on a server.
    ///
    /// Outward translation. It never *decides* anything: the decision and its
    /// evidence already exist, and this renders them.
    LanguageExplanation,

    /// Turn retained evidence into a structured explanation, on the device.
    ///
    /// The offline form of the above — the same facts, shown rather than
    /// narrated.
    StructuredExplanation,

    /// Separate a track into stems, on the device.
    StemSeparation,

    /// Separate a track into stems on a server.
    CloudStemSeparation,

    /// Work out whether a mix meets its delivery target.
    ExportPreparation,
}

impl AgentKind {
    /// Every agent.
    pub const ALL: [Self; 12] = [
        Self::LanguageInterpretation,
        Self::FormInterpretation,
        Self::TrackAnalysis,
        Self::CloudTrackAnalysis,
        Self::LibrarySearch,
        Self::SetPlanning,
        Self::TransitionPlanning,
        Self::LanguageExplanation,
        Self::StructuredExplanation,
        Self::StemSeparation,
        Self::CloudStemSeparation,
        Self::ExportPreparation,
    ];

    /// Where this agent runs.
    #[must_use]
    pub const fn location(self) -> ProcessingLocation {
        match self {
            Self::LanguageInterpretation
            | Self::CloudTrackAnalysis
            | Self::LanguageExplanation
            | Self::CloudStemSeparation => ProcessingLocation::Cloud,
            Self::FormInterpretation
            | Self::TrackAnalysis
            | Self::LibrarySearch
            | Self::SetPlanning
            | Self::TransitionPlanning
            | Self::StructuredExplanation
            | Self::StemSeparation
            | Self::ExportPreparation => ProcessingLocation::OnDevice,
        }
    }

    /// What the user must have agreed to before this may run.
    ///
    /// `None` for everything on the device except learning, which has its own
    /// purpose in `prv-security` and is not an agent.
    #[must_use]
    pub const fn purpose(self) -> Option<Purpose> {
        match self {
            // Reading a sentence and writing one are the same material and
            // the same agreement: whatever the user typed, and whatever the
            // evidence about their library is turned into prose about.
            Self::LanguageInterpretation | Self::LanguageExplanation => {
                Some(Purpose::CloudLanguage)
            }
            Self::CloudTrackAnalysis => Some(Purpose::CloudAnalysis),
            Self::CloudStemSeparation => Some(Purpose::CloudStemSeparation),
            Self::FormInterpretation
            | Self::TrackAnalysis
            | Self::LibrarySearch
            | Self::SetPlanning
            | Self::TransitionPlanning
            | Self::StructuredExplanation
            | Self::StemSeparation
            | Self::ExportPreparation => None,
        }
    }

    /// Whether this agent's output is a musical decision.
    ///
    /// Never true for anything that runs on a server. A musical decision has to
    /// be reproducible, explainable and the same this evening as it was this
    /// afternoon, and a request to a model is none of those.
    #[must_use]
    pub const fn decides_musically(self) -> bool {
        matches!(self, Self::SetPlanning | Self::TransitionPlanning)
    }

    /// The capability this agent serves.
    ///
    /// Two agents may serve the same capability by different means, which is
    /// what makes an offline path a path rather than an absence.
    #[must_use]
    pub const fn capability(self) -> Capability {
        match self {
            Self::LanguageInterpretation | Self::FormInterpretation => Capability::Interpretation,
            Self::TrackAnalysis | Self::CloudTrackAnalysis => Capability::Analysis,
            Self::LibrarySearch => Capability::Search,
            Self::SetPlanning => Capability::Planning,
            Self::TransitionPlanning => Capability::TransitionChoice,
            Self::LanguageExplanation | Self::StructuredExplanation => Capability::Explanation,
            Self::StemSeparation | Self::CloudStemSeparation => Capability::Stems,
            Self::ExportPreparation => Capability::Delivery,
        }
    }

    /// What this agent needs the machine itself to provide.
    ///
    /// `None` for everything that ships as code. The exception is the on-device
    /// stem separator, which ADR-0004 puts behind a model that has to be present
    /// and a machine that can run it in reasonable time.
    #[must_use]
    pub const fn needs(self) -> Option<DeviceFeature> {
        match self {
            Self::StemSeparation => Some(DeviceFeature::LocalStemModel),
            _ => None,
        }
    }

    /// Whether this agent may run here, now.
    ///
    /// Two questions, not one: has the user agreed to it, and can this machine
    /// do it. Conflating them would tell someone on a modest laptop that they
    /// had withheld a permission they never withheld.
    #[must_use]
    pub fn is_available(self, consents: &Consents, device: Device) -> bool {
        let agreed = self
            .purpose()
            .is_none_or(|purpose| consents.allows(purpose));
        let supported = self.needs().is_none_or(|feature| device.has(feature));
        agreed && supported
    }

    /// A stable identifier, for storage and localisation.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::LanguageInterpretation => "agent.language_interpretation",
            Self::FormInterpretation => "agent.form_interpretation",
            Self::TrackAnalysis => "agent.track_analysis",
            Self::CloudTrackAnalysis => "agent.cloud_track_analysis",
            Self::LibrarySearch => "agent.library_search",
            Self::SetPlanning => "agent.set_planning",
            Self::TransitionPlanning => "agent.transition_planning",
            Self::LanguageExplanation => "agent.language_explanation",
            Self::StructuredExplanation => "agent.structured_explanation",
            Self::StemSeparation => "agent.stem_separation",
            Self::CloudStemSeparation => "agent.cloud_stem_separation",
            Self::ExportPreparation => "agent.export_preparation",
        }
    }
}

impl fmt::Display for AgentKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.key())
    }
}

/// Something a machine either can or cannot do for itself.
///
/// Not everything on-device is available on every device. ADR-0004 puts stem
/// separation behind a model that has to be present and a machine that can run
/// it in reasonable time, and pretending otherwise would make the offline path a
/// promise this crate cannot keep on a five-year-old laptop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum DeviceFeature {
    /// A stem separation model, and the compute to run it.
    LocalStemModel,
}

impl DeviceFeature {
    /// A stable identifier, for storage and localisation.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::LocalStemModel => "device.local_stem_model",
        }
    }
}

/// What this machine can do for itself.
///
/// Supplied by the platform, which is the only layer that can answer. Defaults
/// to *not* capable, so a host that has not answered yet is treated as a machine
/// that cannot — the direction that fails toward asking rather than toward
/// promising.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Device {
    local_stem_model: bool,
}

impl Device {
    /// A machine with everything.
    #[must_use]
    pub const fn capable() -> Self {
        Self {
            local_stem_model: true,
        }
    }

    /// A machine with nothing beyond the code that ships in every build.
    #[must_use]
    pub const fn modest() -> Self {
        Self {
            local_stem_model: false,
        }
    }

    /// Whether this machine has a feature.
    #[must_use]
    pub const fn has(self, feature: DeviceFeature) -> bool {
        match feature {
            DeviceFeature::LocalStemModel => self.local_stem_model,
        }
    }
}

/// Something the product can do, independently of what does it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum Capability {
    /// Understand what was asked for.
    Interpretation,
    /// Measure a track.
    Analysis,
    /// Find music in the library.
    Search,
    /// Arrange a set.
    Planning,
    /// Choose how one record becomes the next.
    TransitionChoice,
    /// Say why.
    Explanation,
    /// Take a track apart.
    Stems,
    /// Get finished work out.
    Delivery,
}

impl Capability {
    /// Every capability.
    pub const ALL: [Self; 8] = [
        Self::Interpretation,
        Self::Analysis,
        Self::Search,
        Self::Planning,
        Self::TransitionChoice,
        Self::Explanation,
        Self::Stems,
        Self::Delivery,
    ];

    /// Whether the product is materially unusable without this.
    ///
    /// Stem separation is the one that is not: it is a real feature and nobody
    /// is prevented from planning, mixing or exporting a set by its absence.
    /// Everything else on this list is a thing the product *is*.
    #[must_use]
    pub const fn is_essential(self) -> bool {
        !matches!(self, Self::Stems)
    }

    /// Every agent that serves this capability.
    #[must_use]
    pub fn agents(self) -> Vec<AgentKind> {
        AgentKind::ALL
            .into_iter()
            .filter(|agent| agent.capability() == self)
            .collect()
    }

    /// An agent that can serve this capability under the given agreements.
    ///
    /// Prefers the on-device agent, always. Not for privacy — though it is
    /// better for privacy — but because it is the one that works with no
    /// network, in a basement, forty minutes into a set.
    #[must_use]
    pub fn available_agent(self, consents: &Consents, device: Device) -> Option<AgentKind> {
        let mut candidates = self.agents();
        candidates.sort_by_key(|agent| agent.location().leaves_the_device());
        candidates
            .into_iter()
            .find(|agent| agent.is_available(consents, device))
    }

    /// A stable identifier, for storage and localisation.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Interpretation => "capability.interpretation",
            Self::Analysis => "capability.analysis",
            Self::Search => "capability.search",
            Self::Planning => "capability.planning",
            Self::TransitionChoice => "capability.transition_choice",
            Self::Explanation => "capability.explanation",
            Self::Stems => "capability.stems",
            Self::Delivery => "capability.delivery",
        }
    }
}

impl fmt::Display for Capability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.key())
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

    fn everything_agreed() -> Consents {
        let mut consents = Consents::none();
        let mut ordinal = 0;
        for purpose in Purpose::ALL {
            ordinal += 1;
            consents.grant(purpose, ordinal);
        }
        consents
    }

    #[test]
    fn no_musical_decision_is_ever_made_on_a_server() {
        // ADR-0006's central division, as a property. A musical decision has to
        // be reproducible, explainable and the same this evening as it was this
        // afternoon; a request to a model is none of those.
        for agent in AgentKind::ALL {
            if agent.decides_musically() {
                assert_eq!(
                    agent.location(),
                    ProcessingLocation::OnDevice,
                    "{agent} decides musically and runs on a server"
                );
                assert_eq!(
                    agent.purpose(),
                    None,
                    "{agent} decides musically and needs an agreement to run"
                );
            }
        }
    }

    #[test]
    fn withdrawing_every_agreement_leaves_every_essential_capability_reachable() {
        // "Graceful degradation" means nothing until something measures it.
        // With no consent at all the product still interprets, analyses,
        // searches, plans, chooses transitions, explains and delivers.
        let nothing = Consents::none();
        for capability in Capability::ALL {
            if !capability.is_essential() {
                continue;
            }
            let agent = capability.available_agent(&nothing, Device::capable());
            assert!(
                agent.is_some(),
                "{capability} is unreachable with no agreements"
            );
            assert_eq!(
                agent.map(AgentKind::location),
                Some(ProcessingLocation::OnDevice),
                "{capability} fell back to something that leaves the device"
            );
        }
    }

    #[test]
    fn the_on_device_agent_is_preferred_even_when_the_cloud_is_available() {
        // Not for privacy — though it is better for privacy — but because it
        // is the one that works with no network, in a basement, forty minutes
        // into a set.
        let everything = everything_agreed();
        for capability in Capability::ALL {
            let chosen = capability.available_agent(&everything, Device::capable());
            assert_eq!(
                chosen.map(AgentKind::location),
                Some(ProcessingLocation::OnDevice),
                "{capability} chose a server agent when a local one existed"
            );
        }
    }

    #[test]
    fn a_modest_machine_falls_back_to_the_cloud_and_only_with_an_agreement() {
        // The case that makes "on device" honest. Stem separation needs a model
        // and the compute to run it; a machine without either is not a machine
        // whose user withheld a permission.
        let modest = Device::modest();
        let nothing = Consents::none();

        assert!(!AgentKind::StemSeparation.is_available(&nothing, modest));
        assert_eq!(
            Capability::Stems.available_agent(&nothing, modest),
            None,
            "a machine that cannot separate locally, with no agreement, has no path"
        );

        let mut agreed = Consents::none();
        agreed.grant(Purpose::CloudStemSeparation, 1);
        assert_eq!(
            Capability::Stems.available_agent(&agreed, modest),
            Some(AgentKind::CloudStemSeparation)
        );

        // And the same machine still does everything essential, locally.
        for capability in Capability::ALL {
            if !capability.is_essential() {
                continue;
            }
            assert_eq!(
                capability
                    .available_agent(&nothing, modest)
                    .map(AgentKind::location),
                Some(ProcessingLocation::OnDevice),
                "{capability} needed a capable machine or an agreement"
            );
        }
    }

    #[test]
    fn a_capability_with_no_local_path_is_never_an_essential_one() {
        // If a capability could only ever be served from a server, this is the
        // test that would say so honestly rather than letting the product claim
        // an offline path it does not have.
        for capability in Capability::ALL {
            let agents = capability.agents();
            assert!(!agents.is_empty(), "{capability} has no agent at all");

            let has_unconditional_local = agents
                .iter()
                .any(|agent| !agent.location().leaves_the_device() && agent.needs().is_none());
            if !has_unconditional_local {
                assert!(
                    !capability.is_essential(),
                    "{capability} is essential and has no unconditional local agent"
                );
            }
        }
    }

    #[test]
    fn every_cloud_agent_names_the_agreement_it_needs() {
        // An agent that reached a server without a purpose would be one nobody
        // could withdraw consent for.
        for agent in AgentKind::ALL {
            assert_eq!(
                agent.location().leaves_the_device(),
                agent.purpose().is_some(),
                "{agent} disagrees about whether it needs an agreement"
            );
        }
    }

    #[test]
    fn an_agreement_makes_exactly_the_agent_it_names_available() {
        let mut consents = Consents::none();
        assert!(!AgentKind::CloudTrackAnalysis.is_available(&consents, Device::capable()));
        assert!(AgentKind::TrackAnalysis.is_available(&consents, Device::capable()));

        consents.grant(Purpose::CloudAnalysis, 1);
        assert!(AgentKind::CloudTrackAnalysis.is_available(&consents, Device::capable()));
        assert!(
            !AgentKind::LanguageInterpretation.is_available(&consents, Device::capable()),
            "agreeing to analysis agreed to reading what the user wrote"
        );
    }

    #[test]
    fn reading_a_sentence_and_hearing_a_record_are_separate_agreements() {
        // The material is different in kind: one is a recording the user owns,
        // the other is something they wrote, which may mention anything at all.
        assert_eq!(
            AgentKind::LanguageInterpretation.purpose(),
            Some(Purpose::CloudLanguage)
        );
        assert_eq!(
            AgentKind::CloudTrackAnalysis.purpose(),
            Some(Purpose::CloudAnalysis)
        );
    }

    #[test]
    fn agent_and_capability_keys_are_distinct() {
        let agents: Vec<&str> = AgentKind::ALL.iter().map(|a| a.key()).collect();
        for (index, key) in agents.iter().enumerate() {
            for (other, value) in agents.iter().enumerate() {
                assert!(index == other || key != value, "two agents share {key}");
            }
        }

        let capabilities: Vec<&str> = Capability::ALL.iter().map(|c| c.key()).collect();
        for (index, key) in capabilities.iter().enumerate() {
            for (other, value) in capabilities.iter().enumerate() {
                assert!(
                    index == other || key != value,
                    "two capabilities share {key}"
                );
            }
        }
    }
}
