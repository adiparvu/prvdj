//! The track profile: everything the system knows about one recording.
//!
//! # Stages are versioned independently, and that is the whole design
//!
//! Master Prompt #20 requires each analysis stage to carry its own version so
//! that improving one does not invalidate the others. The consequence is
//! concrete and large: when key detection improves, a hundred thousand tracks
//! need their key recomputed and nothing else. Re-running tempo, beats,
//! loudness and structure as well would turn a background task into an
//! overnight one, and users would decline the upgrade.
//!
//! So a profile is not one result with one version. It is a set of independently
//! versioned results, each of which can be present, absent, or stale on its own,
//! and [`TrackProfile::stale_stages`] is what an upgrade path is built from.
//!
//! # A stage that failed is recorded as having failed
//!
//! Every stage is an [`Option`], and absence means something specific: the stage
//! ran and could not produce a result. A track of applause has no tempo — not a
//! low-confidence tempo, none — and `None` says that where a number with a low
//! confidence would not. Master Prompt #25 requires the product to be honest
//! about uncertainty, and the first part of that is distinguishing "unsure" from
//! "not applicable".
//!
//! # This is the boundary between analysis and everything else
//!
//! `prv-library` holds an `AnalysisRef` rather than a copy of these values, and
//! ADR-0006's planner reads a profile rather than raw audio. The profile is
//! therefore the contract: it is what gets stored, what gets synchronised, and
//! what an explanation is rendered from.

use prv_time::{BeatGrid, SampleRate, TimeSignature};

use crate::beats::BeatEstimate;
use crate::chroma::Chroma;
use crate::confidence::Confidence;
use crate::error::AnalysisError;
use crate::key::KeyEstimate;
use crate::loudness::Loudness;
use crate::onset::NoveltyCurve;
use crate::spectrum::Stft;
use crate::structure::Structure;
use crate::tempo::TempoEstimate;

/// The analysis stages, each versioned on its own.
///
/// The list is closed rather than open because a stage is not a plugin: adding
/// one changes what a profile means and needs a migration, so it should be a
/// change to this enum that a reviewer sees.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Stage {
    /// Onset detection and the novelty curve everything rhythmic rests on.
    Rhythm,
    /// Tempo and its octave alternatives.
    Tempo,
    /// Beat positions and downbeats.
    Beats,
    /// Chroma and key.
    Key,
    /// Loudness, range and true peak.
    Loudness,
    /// Section boundaries.
    Structure,
}

impl Stage {
    /// Every stage, in the order they are run.
    ///
    /// The order is a dependency order, not a preference: beats need a tempo,
    /// structure needs beats. A caller re-running one stage must re-run those
    /// after it, and this list is what says which those are.
    pub const ALL: [Self; 6] = [
        Self::Rhythm,
        Self::Tempo,
        Self::Beats,
        Self::Key,
        Self::Loudness,
        Self::Structure,
    ];

    /// The current version of this stage's algorithm.
    ///
    /// Incremented whenever a change alters the result for the same input. That
    /// is a deliberate discipline rather than a convention: a stored profile
    /// carries the version it was produced by, so a stage whose version was not
    /// incremented after a behavioural change leaves every library in the field
    /// holding results that disagree with what the code would now produce, with
    /// nothing to detect it.
    #[must_use]
    pub const fn version(self) -> u32 {
        // Every stage starts at one. They are listed separately rather than
        // collapsed into a single arm so that incrementing one is a one-line
        // change to the line that names it, which is what makes the discipline
        // survive contact with a hurried afternoon.
        #[allow(
            clippy::match_same_arms,
            reason = "one arm per stage so a version bump edits the stage's own line"
        )]
        match self {
            Self::Rhythm => 1,
            Self::Tempo => 1,
            Self::Beats => 1,
            Self::Key => 1,
            Self::Loudness => 1,
            Self::Structure => 1,
        }
    }

    /// The stages whose results become invalid if this one changes.
    #[must_use]
    pub const fn dependents(self) -> &'static [Self] {
        match self {
            Self::Rhythm => &[Self::Tempo, Self::Beats, Self::Structure],
            Self::Tempo => &[Self::Beats, Self::Structure],
            Self::Beats => &[Self::Structure],
            Self::Key | Self::Loudness | Self::Structure => &[],
        }
    }

    /// A stable identifier, for storage and for localisation.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Rhythm => "stage.rhythm",
            Self::Tempo => "stage.tempo",
            Self::Beats => "stage.beats",
            Self::Key => "stage.key",
            Self::Loudness => "stage.loudness",
            Self::Structure => "stage.structure",
        }
    }
}

/// A result together with the version of the code that produced it.
#[derive(Debug, Clone, PartialEq)]
pub struct Versioned<T> {
    value: T,
    version: u32,
}

impl<T> Versioned<T> {
    /// Wraps a value with a stage's current version.
    pub fn current(stage: Stage, value: T) -> Self {
        Self {
            value,
            version: stage.version(),
        }
    }

    /// The value.
    pub const fn value(&self) -> &T {
        &self.value
    }

    /// The version that produced it.
    pub const fn version(&self) -> u32 {
        self.version
    }

    /// Whether this result was produced by an older version of its stage.
    pub const fn is_stale(&self, stage: Stage) -> bool {
        self.version < stage.version()
    }

    /// Unwraps the value, discarding the version.
    pub fn into_value(self) -> T {
        self.value
    }
}

/// Everything the analysis knows about one track.
#[derive(Debug, Clone)]
pub struct TrackProfile {
    sample_rate: SampleRate,
    duration: usize,
    tempo: Option<Versioned<TempoEstimate>>,
    beats: Option<Versioned<BeatEstimate>>,
    key: Option<Versioned<KeyEstimate>>,
    loudness: Option<Versioned<Loudness>>,
    structure: Option<Versioned<Structure>>,
}

impl TrackProfile {
    /// The sample rate the analysis was run at.
    #[must_use]
    pub const fn sample_rate(&self) -> SampleRate {
        self.sample_rate
    }

    /// The track's length in sample frames.
    #[must_use]
    pub const fn duration(&self) -> usize {
        self.duration
    }

    /// The tempo, if one was found.
    #[must_use]
    pub fn tempo(&self) -> Option<&TempoEstimate> {
        self.tempo.as_ref().map(Versioned::value)
    }

    /// The beat grid, if one was found.
    #[must_use]
    pub fn beats(&self) -> Option<&BeatEstimate> {
        self.beats.as_ref().map(Versioned::value)
    }

    /// The key, if one was found.
    #[must_use]
    pub fn key(&self) -> Option<&KeyEstimate> {
        self.key.as_ref().map(Versioned::value)
    }

    /// The loudness, if it could be measured.
    #[must_use]
    pub fn loudness(&self) -> Option<&Loudness> {
        self.loudness.as_ref().map(Versioned::value)
    }

    /// The structure, if sections were found.
    #[must_use]
    pub fn structure(&self) -> Option<&Structure> {
        self.structure.as_ref().map(Versioned::value)
    }

    /// The chroma the key was derived from.
    #[must_use]
    pub fn chroma(&self) -> Option<Chroma> {
        self.key().map(KeyEstimate::chroma)
    }

    /// A beat grid ready for `prv-time`, if beats were found.
    #[must_use]
    pub fn beat_grid(&self, signature: TimeSignature) -> Option<BeatGrid> {
        self.beats().map(|beats| beats.to_beat_grid(signature))
    }

    /// The stages that ran and produced a result.
    #[must_use]
    pub fn completed_stages(&self) -> Vec<Stage> {
        let mut stages = Vec::new();
        if self.tempo.is_some() {
            stages.push(Stage::Tempo);
        }
        if self.beats.is_some() {
            stages.push(Stage::Beats);
        }
        if self.key.is_some() {
            stages.push(Stage::Key);
        }
        if self.loudness.is_some() {
            stages.push(Stage::Loudness);
        }
        if self.structure.is_some() {
            stages.push(Stage::Structure);
        }
        stages
    }

    /// The stages whose stored result predates the current algorithm.
    ///
    /// This is what a background re-analysis works through after an upgrade.
    /// A stage whose own version is current but whose *dependency* is stale is
    /// included, because a beat grid computed from a superseded tempo is stale
    /// whatever its own version says.
    #[must_use]
    pub fn stale_stages(&self) -> Vec<Stage> {
        let mut stale = Vec::new();
        let mut push = |stage: Stage, outdated: bool| {
            if outdated {
                stale.push(stage);
            }
        };

        push(
            Stage::Tempo,
            self.tempo
                .as_ref()
                .is_some_and(|value| value.is_stale(Stage::Tempo)),
        );
        push(
            Stage::Beats,
            self.beats
                .as_ref()
                .is_some_and(|value| value.is_stale(Stage::Beats)),
        );
        push(
            Stage::Key,
            self.key
                .as_ref()
                .is_some_and(|value| value.is_stale(Stage::Key)),
        );
        push(
            Stage::Loudness,
            self.loudness
                .as_ref()
                .is_some_and(|value| value.is_stale(Stage::Loudness)),
        );
        push(
            Stage::Structure,
            self.structure
                .as_ref()
                .is_some_and(|value| value.is_stale(Stage::Structure)),
        );

        // Anything downstream of a stale stage is stale too.
        let direct = stale.clone();
        for stage in direct {
            for &dependent in stage.dependents() {
                if !stale.contains(&dependent) && self.has(dependent) {
                    stale.push(dependent);
                }
            }
        }
        stale.sort_unstable();
        stale.dedup();
        stale
    }

    /// Whether a stage produced a result.
    #[must_use]
    pub fn has(&self, stage: Stage) -> bool {
        match stage {
            Stage::Rhythm => true,
            Stage::Tempo => self.tempo.is_some(),
            Stage::Beats => self.beats.is_some(),
            Stage::Key => self.key.is_some(),
            Stage::Loudness => self.loudness.is_some(),
            Stage::Structure => self.structure.is_some(),
        }
    }

    /// The confidence of the profile as a whole.
    ///
    /// The weakest of the stages that produced a result, because a profile is
    /// only as trustworthy as the least trustworthy thing in it. A stage that
    /// found nothing does not lower this — it is absent rather than uncertain,
    /// and treating "no key" as "an unreliable key" would make every percussion
    /// track look like a failed analysis.
    #[must_use]
    pub fn confidence(&self) -> Confidence {
        let mut overall: Option<Confidence> = None;
        let mut fold = |value: Confidence| {
            overall = Some(match overall {
                Some(existing) => existing.and_then(value),
                None => value,
            });
        };

        if let Some(tempo) = self.tempo() {
            fold(tempo.confidence());
        }
        if let Some(beats) = self.beats() {
            fold(beats.beat_confidence());
        }
        if let Some(key) = self.key() {
            fold(key.confidence());
        }
        if let Some(structure) = self.structure() {
            fold(structure.confidence());
        }
        overall.unwrap_or(Confidence::NONE)
    }
}

/// Runs every analysis stage over a mono signal.
///
/// # Partial results are the normal case
///
/// A stage that fails does not fail the analysis. Applause has no tempo, a
/// field recording has no key, and a fifteen-second sample has no structure —
/// all of those are tracks a user may legitimately have in their library, and
/// refusing to analyse them at all would leave a hole where a loudness figure
/// and a waveform would have been perfectly useful.
///
/// The one genuine failure is a signal so short that nothing can be measured,
/// and that is reported.
///
/// # Errors
///
/// Returns [`AnalysisError::NotEnoughAudio`] when no stage could run at all,
/// and propagates transform construction errors.
pub fn analyse(samples: &[f32], sample_rate: SampleRate) -> Result<TrackProfile, AnalysisError> {
    let mut profile = TrackProfile {
        sample_rate,
        duration: samples.len(),
        tempo: None,
        beats: None,
        key: None,
        loudness: None,
        structure: None,
    };

    let mut rhythm = Stft::for_rhythm()?;
    let curve = NoveltyCurve::compute(&mut rhythm, samples, sample_rate)?;

    if let Ok(tempo) = crate::tempo::estimate(&curve) {
        if let Ok(beats) = crate::beats::track(&curve, &tempo) {
            let mut tonal = Stft::for_tone()?;
            if let Ok(structure) =
                crate::structure::detect(&mut tonal, samples, sample_rate, &beats)
            {
                profile.structure = Some(Versioned::current(Stage::Structure, structure));
            }
            profile.beats = Some(Versioned::current(Stage::Beats, beats));
        }
        profile.tempo = Some(Versioned::current(Stage::Tempo, tempo));
    }

    let mut tonal = Stft::for_tone()?;
    if let Ok(key) = crate::key::detect_signal(&mut tonal, samples, sample_rate) {
        profile.key = Some(Versioned::current(Stage::Key, key));
    }

    if let Ok(loudness) = crate::loudness::measure(samples, sample_rate) {
        profile.loudness = Some(Versioned::current(Stage::Loudness, loudness));
    }

    if profile.completed_stages().is_empty() {
        return Err(AnalysisError::NotEnoughAudio {
            frames: samples.len(),
            minimum: rhythm.window_size(),
        });
    }

    Ok(profile)
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::float_cmp,
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a test that cannot build its own fixture should fail loudly, and the signal \
                  generators here work in exact, bounded quantities"
    )]

    use super::*;
    use crate::testing::{samples, tone, write_click};

    /// A track with a beat, a key and an arrangement.
    fn musical_track(rate: SampleRate) -> Vec<f32> {
        let period = samples(0.5, rate);
        let bar = period * 4;
        let bars = 32_usize;
        let mut signal = vec![0.0_f32; bar * bars];

        for index in 0..(bars * 4) {
            write_click(&mut signal, index * period, 0x7A11 + index as u64, rate);
        }
        // An A minor triad held through the track, so the key is determinate.
        for frequency in [110.0, 261.63, 329.63] {
            let partial = tone(frequency, signal.len(), 0.2, rate);
            for (slot, &value) in signal.iter_mut().zip(partial.iter()) {
                *slot += value;
            }
        }
        // A loud second half, so there is an arrangement to find.
        let low_note = tone(55.0, period, 0.8, rate);
        for bar_index in 16..bars {
            for beat in 0..4 {
                let start = bar_index * bar + beat * period;
                for (offset, &value) in low_note.iter().enumerate() {
                    if let Some(slot) = signal.get_mut(start + offset) {
                        *slot += value;
                    }
                }
            }
        }
        signal
    }

    #[test]
    fn a_musical_track_produces_a_complete_profile() {
        let rate = SampleRate::HZ_44100;
        let profile = analyse(&musical_track(rate), rate).expect("a musical track analyses");

        assert!(profile.tempo().is_some(), "no tempo");
        assert!(profile.beats().is_some(), "no beats");
        assert!(profile.key().is_some(), "no key");
        assert!(profile.loudness().is_some(), "no loudness");
        assert!(profile.structure().is_some(), "no structure");
        assert_eq!(profile.completed_stages().len(), 5);
        assert!(profile.stale_stages().is_empty());

        let tempo = profile.tempo().expect("checked").tempo().bpm();
        assert!((tempo - 120.0).abs() < 1.0, "tempo read {tempo}");
        assert!(profile.chroma().is_some());
        assert!(profile.beat_grid(TimeSignature::FOUR_FOUR).is_some());
    }

    #[test]
    fn a_stage_that_finds_nothing_is_absent_rather_than_uncertain() {
        // The distinction the whole type is built around. Ten seconds of silence
        // has no tempo, no key and no structure — and a profile that reported a
        // low-confidence tempo of 120 for it would be lying in a way a user
        // cannot check.
        let rate = SampleRate::HZ_44100;
        let signal = vec![0.0_f32; samples(20.0, rate)];
        let profile = analyse(&signal, rate).expect("silence still measures loudness");

        assert!(profile.tempo().is_none(), "silence was given a tempo");
        assert!(profile.beats().is_none(), "silence was given beats");
        assert!(profile.key().is_none(), "silence was given a key");
        assert!(profile.structure().is_none(), "silence was given sections");
        assert!(
            profile.loudness().is_some(),
            "loudness is measurable for anything, including silence"
        );
        assert_eq!(profile.completed_stages(), vec![Stage::Loudness]);
    }

    #[test]
    fn a_partial_analysis_is_a_success_not_a_failure() {
        // A sustained tone: measurable loudness and key, no beat. A library
        // full of ambient recordings and field samples must not become a
        // library full of failed analyses.
        let rate = SampleRate::HZ_44100;
        let mut signal = tone(220.0, samples(20.0, rate), 0.4, rate);
        let fifth = tone(330.0, samples(20.0, rate), 0.3, rate);
        for (slot, &value) in signal.iter_mut().zip(fifth.iter()) {
            *slot += value;
        }

        let profile = analyse(&signal, rate).expect("a tone still analyses");
        assert!(profile.loudness().is_some());
        assert!(profile.key().is_some());
        assert!(
            profile.tempo().is_none()
                || !profile
                    .tempo()
                    .expect("checked")
                    .confidence()
                    .is_actionable(),
            "a sustained tone was given a confident tempo"
        );
    }

    #[test]
    fn staleness_propagates_to_everything_downstream() {
        // The upgrade path this type exists for. A profile whose tempo was
        // produced by an older algorithm has a stale tempo *and* a stale beat
        // grid, because the grid was computed from it — even though the beat
        // tracker itself has not changed.
        let rate = SampleRate::HZ_44100;
        let mut profile = analyse(&musical_track(rate), rate).expect("analyses");
        assert!(profile.stale_stages().is_empty());

        if let Some(tempo) = profile.tempo.as_mut() {
            tempo.version = 0;
        }
        let stale = profile.stale_stages();
        assert!(stale.contains(&Stage::Tempo), "the tempo is not stale");
        assert!(
            stale.contains(&Stage::Beats),
            "the beat grid derived from a superseded tempo was not marked stale"
        );
        assert!(
            stale.contains(&Stage::Structure),
            "the structure derived from a superseded grid was not marked stale"
        );
        assert!(
            !stale.contains(&Stage::Loudness),
            "loudness does not depend on tempo and should not be invalidated"
        );
        assert!(
            !stale.contains(&Stage::Key),
            "key does not depend on tempo and should not be invalidated"
        );
    }

    #[test]
    fn the_dependency_order_has_no_cycles_and_matches_the_run_order() {
        // A cycle here would make an upgrade loop forever; a dependency that
        // ran before its dependant would make it read a stale value. Both are
        // the kind of defect that only appears once someone adds a seventh
        // stage, which is exactly when a test is the only thing that catches it.
        for (index, stage) in Stage::ALL.iter().enumerate() {
            for dependent in stage.dependents() {
                let position = Stage::ALL
                    .iter()
                    .position(|candidate| candidate == dependent)
                    .expect("every dependent is a stage");
                assert!(
                    position > index,
                    "{stage:?} is run after its dependent {dependent:?}"
                );
            }
        }
    }

    #[test]
    fn stage_keys_are_distinct() {
        for (index, stage) in Stage::ALL.iter().enumerate() {
            for (other_index, other) in Stage::ALL.iter().enumerate() {
                assert!(
                    index == other_index || stage.key() != other.key(),
                    "two stages share the key {}",
                    stage.key()
                );
            }
        }
    }

    #[test]
    fn the_profile_is_no_more_confident_than_its_weakest_stage() {
        let rate = SampleRate::HZ_44100;
        let profile = analyse(&musical_track(rate), rate).expect("analyses");

        let overall = profile.confidence();
        for individual in [
            profile.tempo().map(TempoEstimate::confidence),
            profile.beats().map(BeatEstimate::beat_confidence),
            profile.key().map(KeyEstimate::confidence),
            profile.structure().map(Structure::confidence),
        ]
        .into_iter()
        .flatten()
        {
            assert!(
                overall <= individual,
                "the profile reports {overall} while a stage reports {individual}"
            );
        }
    }

    #[test]
    fn material_too_short_for_any_stage_is_refused() {
        let rate = SampleRate::HZ_44100;
        assert!(matches!(
            analyse(&vec![0.0_f32; 200], rate),
            Err(AnalysisError::NotEnoughAudio { .. })
        ));
    }

    #[test]
    fn analysis_is_reproducible() {
        let rate = SampleRate::HZ_44100;
        let signal = musical_track(rate);
        let first = analyse(&signal, rate).expect("analyses");
        let second = analyse(&signal, rate).expect("analyses");

        assert_eq!(
            first.tempo().map(TempoEstimate::tempo),
            second.tempo().map(TempoEstimate::tempo)
        );
        assert_eq!(
            first.key().map(KeyEstimate::key),
            second.key().map(KeyEstimate::key)
        );
        assert_eq!(
            first.loudness().map(Loudness::integrated),
            second.loudness().map(Loudness::integrated)
        );
        assert_eq!(
            first.structure().map(|found| found.sections().len()),
            second.structure().map(|found| found.sections().len())
        );
    }
}
