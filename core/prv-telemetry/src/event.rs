//! The things worth counting.
//!
//! # A closed list, and a short one
//!
//! Every event is a variant here, so adding one is a change a reviewer sees.
//! The alternative — a string-keyed counter anyone can call — produces an
//! analytics surface nobody can enumerate, and a surface nobody can enumerate
//! cannot be described honestly to the person it is about.
//!
//! # An event carries no value
//!
//! Not a track, not a project, not a duration, not a position. It is a *kind*
//! and it is counted. That is a real constraint on what can ever be learned from
//! this — deliberately, because the useful questions ("is anybody using the
//! second version of a plan?") are answerable from counts, and the questions
//! that need more are questions about a person.

use core::fmt;

/// Something that happened, worth knowing the rate of.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum Event {
    /// A set was planned.
    PlanRequested,
    /// A planned set was kept as offered.
    PlanKept,
    /// A planned set was adjusted before use.
    PlanAdjusted,
    /// A planned set was thrown away.
    PlanRejected,
    /// An alternative version was chosen over the first.
    AlternativeChosen,

    /// A track was analysed.
    TrackAnalysed,
    /// A track could not be analysed.
    AnalysisFailed,
    /// A tempo or key was corrected by hand.
    AnalysisCorrected,

    /// A mix was exported.
    ExportCompleted,
    /// An export was stopped because it would not meet its target.
    ExportBlocked,

    /// A plugin was passed over after misbehaving.
    PluginBypassed,
    /// A plugin stopped working.
    PluginCrashed,

    /// A cloud step could not reach its server.
    CloudUnreachable,
    /// Work was postponed because a set was playing.
    WorkDeferred,
}

impl Event {
    /// Every event, so a report cannot omit one and a user cannot be shown a
    /// partial list of what is counted.
    pub const ALL: [Self; 14] = [
        Self::PlanRequested,
        Self::PlanKept,
        Self::PlanAdjusted,
        Self::PlanRejected,
        Self::AlternativeChosen,
        Self::TrackAnalysed,
        Self::AnalysisFailed,
        Self::AnalysisCorrected,
        Self::ExportCompleted,
        Self::ExportBlocked,
        Self::PluginBypassed,
        Self::PluginCrashed,
        Self::CloudUnreachable,
        Self::WorkDeferred,
    ];

    /// Whether this event reports something going wrong.
    ///
    /// What separates the two agreements. A crash report is a different bargain
    /// from usage counting: one is offered to get something fixed, the other to
    /// help decide what to build. Master Prompt #26 keeps them separate, and
    /// this is how an event knows which it belongs to.
    #[must_use]
    pub const fn is_a_fault(self) -> bool {
        matches!(
            self,
            Self::AnalysisFailed
                | Self::ExportBlocked
                | Self::PluginBypassed
                | Self::PluginCrashed
                | Self::CloudUnreachable
        )
    }

    /// A stable identifier, for storage and localisation.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::PlanRequested => "event.plan_requested",
            Self::PlanKept => "event.plan_kept",
            Self::PlanAdjusted => "event.plan_adjusted",
            Self::PlanRejected => "event.plan_rejected",
            Self::AlternativeChosen => "event.alternative_chosen",
            Self::TrackAnalysed => "event.track_analysed",
            Self::AnalysisFailed => "event.analysis_failed",
            Self::AnalysisCorrected => "event.analysis_corrected",
            Self::ExportCompleted => "event.export_completed",
            Self::ExportBlocked => "event.export_blocked",
            Self::PluginBypassed => "event.plugin_bypassed",
            Self::PluginCrashed => "event.plugin_crashed",
            Self::CloudUnreachable => "event.cloud_unreachable",
            Self::WorkDeferred => "event.work_deferred",
        }
    }
}

impl fmt::Display for Event {
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

    #[test]
    fn faults_and_usage_are_separable() {
        // They are two different bargains: one is offered to get something
        // fixed, the other to help decide what to build.
        assert!(Event::PluginCrashed.is_a_fault());
        assert!(Event::AnalysisFailed.is_a_fault());
        assert!(!Event::PlanKept.is_a_fault());
        assert!(!Event::ExportCompleted.is_a_fault());

        let faults = Event::ALL.iter().filter(|e| e.is_a_fault()).count();
        assert!(
            faults > 0 && faults < Event::ALL.len(),
            "the split is empty"
        );
    }

    #[test]
    fn every_event_is_a_kind_and_nothing_else() {
        // The type has no payload. If a variant ever gained one, this stops
        // compiling — which is the point, because the useful questions are
        // answerable from counts and the rest are questions about a person.
        for event in Event::ALL {
            let copied: Event = event;
            assert_eq!(copied, event);
            assert!(event.key().starts_with("event."));
        }
    }

    #[test]
    fn event_keys_are_distinct() {
        let keys: Vec<&str> = Event::ALL.iter().map(|e| e.key()).collect();
        for (index, key) in keys.iter().enumerate() {
            for (other, value) in keys.iter().enumerate() {
                assert!(index == other || key != value, "two events share {key}");
            }
        }
    }
}
