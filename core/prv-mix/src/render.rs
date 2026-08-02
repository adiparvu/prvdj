//! Turning a plan into edits.
//!
//! # An AI mix is an ordinary edit
//!
//! This module produces [`OperationPayload`]s, not a timeline. That is the
//! decision the whole module is arranged around, and it follows from ADR-0003
//! plus Master Prompt #9: if the system builds a mix by *appending operations to
//! the user's log*, then the mix is undoable, comparable against what was there
//! before, branchable, synchronisable, and editable clip by clip — with none of
//! that written here.
//!
//! The alternative, handing back a finished timeline, would make an AI-generated
//! mix a different kind of object from a hand-made one. Every feature the log
//! provides would then need a second implementation for the generated case, and
//! the first thing a user would discover is that they cannot undo it.
//!
//! Master Prompt #3B requires the user to be able to edit everything the system
//! decides. Emitting operations is what makes that true by construction rather
//! than by a promise to add editing later.
//!
//! # The technique is derived, not chosen
//!
//! How two tracks are joined — a long blend, a bass swap, a filtered fade, a cut
//! — is decided from the evidence the planner already produced. A transition
//! whose harmonic and tempo scores are strong can support a long overlap where
//! both records are audible; one whose harmony is weaker cannot, and the honest
//! response is to spend less time with both playing rather than to blend anyway
//! and hope.
//!
//! This means the technique is *explainable in the same terms as the choice of
//! track*, which is what ADR-0006 requires: the reason for a short cut is the
//! same number that appears in the tracklist.

use prv_project::{
    Interpolation, OperationPayload, ParameterAddress, ParameterKey, ParameterOwner, PlacementId,
    TrackRef,
};
use prv_time::{Frames, SampleRate};

use crate::candidate::{Candidate, TrackId};
use crate::num::{count_to_f64, narrow, round_to_i64, signed_to_f64};
use crate::plan::{MixPlan, PlannedTrack};
use crate::transition::{Component, ScoreComponents};

/// The longest overlap the renderer will produce, in beats.
///
/// Thirty-two, which is eight bars in common time — a full phrase. Longer than
/// that and two arrangements are competing for the same space for so long that
/// the result is a texture rather than a transition, and a DJ who wanted that
/// would build it by hand.
pub const MAX_OVERLAP_BEATS: f64 = 32.0;

/// The shortest overlap that is still a transition rather than a cut, in beats.
pub const MIN_OVERLAP_BEATS: f64 = 4.0;

/// How two tracks are joined.
///
/// Every variant is chosen by a rule stated on it, so a user shown the name can
/// be told exactly what was measured to produce it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Technique {
    /// A long overlap with both records audible, and the bass handed over part
    /// way.
    ///
    /// Chosen when the harmonic and tempo evidence is strong enough that the
    /// two tracks genuinely sit together. This is what a good transition sounds
    /// like and it is the most demanding: everything has to be right.
    Blend,

    /// A medium overlap where the outgoing track's low end is removed early.
    ///
    /// Chosen when the tempo evidence is strong but the harmonic evidence is
    /// not. Two basslines a semitone apart is the most audible clash there is,
    /// and taking one of them out removes it — which is why a DJ reaches for the
    /// low equaliser before they reach for anything else.
    BassSwap,

    /// A medium overlap where the outgoing track is filtered away.
    ///
    /// Chosen when the two records are compatible but the *structure* offers
    /// nowhere quiet to do it. Sweeping the outgoing track upward removes it
    /// from the arrangement gradually without needing a breakdown to hide in.
    FilterFade,

    /// A short overlap, on a phrase boundary.
    ///
    /// Chosen when the evidence does not support keeping both records audible.
    /// A short cut on the beat is an honest answer and is what a working DJ does
    /// with two records that do not blend; a long blend attempted anyway is the
    /// thing an audience notices.
    Cut,
}

impl Technique {
    /// A stable identifier, for storage and localisation.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Blend => "technique.blend",
            Self::BassSwap => "technique.bass_swap",
            Self::FilterFade => "technique.filter_fade",
            Self::Cut => "technique.cut",
        }
    }

    /// How long this technique's overlap is, in beats, given the transition's
    /// overall score.
    ///
    /// The score scales the length within the technique's own range. A blend
    /// scoring 0.95 gets the full phrase; one scoring 0.75 gets less, because a
    /// weaker match spends less time exposed.
    #[must_use]
    pub fn overlap_beats(self, score: f32) -> f64 {
        let quality = f64::from(score.clamp(0.0, 1.0));
        let (shortest, longest) = match self {
            Self::Blend => (16.0, MAX_OVERLAP_BEATS),
            Self::BassSwap | Self::FilterFade => (8.0, 16.0),
            Self::Cut => (MIN_OVERLAP_BEATS, 8.0),
        };
        // Quantised to whole bars in common time, because a transition that
        // begins or ends mid-bar is a transition that sounds late.
        let raw = shortest + (longest - shortest) * quality;
        (raw / 4.0).round() * 4.0
    }
}

/// The evidence that produced a technique choice.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TechniqueChoice {
    technique: Technique,
    reason: Component,
}

impl TechniqueChoice {
    /// The technique.
    #[must_use]
    pub const fn technique(self) -> Technique {
        self.technique
    }

    /// The score component that decided it.
    ///
    /// What an explanation leads with: "a short cut, because these two keys are
    /// only loosely compatible" is a sentence a user can act on, and it names
    /// the same number that appears beside the track in the list.
    #[must_use]
    pub const fn reason(self) -> Component {
        self.reason
    }
}

/// Chooses how to join two tracks, from the evidence the planner produced.
///
/// The thresholds are **provisional**, like every weight in this system, and
/// are calibrated in Phase 3 as ADR-0006 schedules. The ordering they induce is
/// not provisional: better evidence never produces a more cautious technique.
#[must_use]
pub fn choose_technique(components: ScoreComponents) -> TechniqueChoice {
    /// Above this a component is strong enough to rely on.
    const STRONG: f32 = 0.7;
    /// Below this a component cannot support keeping both records audible.
    const WEAK: f32 = 0.4;

    let weakest = components.weakest();

    // Tempo first: nothing else matters if the two records will not run
    // together. A tempo the deck cannot reach is not a transition to shorten,
    // it is one to cut.
    if components.tempo < WEAK {
        return TechniqueChoice {
            technique: Technique::Cut,
            reason: Component::Tempo,
        };
    }

    if components.harmonic < WEAK {
        return TechniqueChoice {
            technique: Technique::Cut,
            reason: Component::Harmonic,
        };
    }

    if components.harmonic < STRONG {
        // Compatible enough to overlap, not enough to leave both basslines in.
        return TechniqueChoice {
            technique: Technique::BassSwap,
            reason: Component::Harmonic,
        };
    }

    if components.structure < WEAK {
        // The records suit each other; the arrangement offers nowhere quiet.
        return TechniqueChoice {
            technique: Technique::FilterFade,
            reason: Component::Structure,
        };
    }

    if components.vocal < WEAK {
        // Two vocals over each other is the most recognisable amateur mistake,
        // so the overlap is kept short even when everything else is right.
        return TechniqueChoice {
            technique: Technique::Cut,
            reason: Component::Vocal,
        };
    }

    TechniqueChoice {
        technique: Technique::Blend,
        reason: weakest,
    }
}

/// One transition, as it will be performed.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderedTransition {
    from: TrackId,
    to: TrackId,
    choice: TechniqueChoice,
    at: Frames,
    length: Frames,
}

impl RenderedTransition {
    /// The outgoing track.
    #[must_use]
    pub const fn from(&self) -> TrackId {
        self.from
    }

    /// The incoming track.
    #[must_use]
    pub const fn to(&self) -> TrackId {
        self.to
    }

    /// How the two are joined, and why.
    #[must_use]
    pub const fn choice(&self) -> TechniqueChoice {
        self.choice
    }

    /// Where on the set's timeline the transition begins.
    #[must_use]
    pub const fn at(&self) -> Frames {
        self.at
    }

    /// How long both records are audible for.
    #[must_use]
    pub const fn length(&self) -> Frames {
        self.length
    }

    /// Where the transition ends.
    #[must_use]
    pub fn end(&self) -> Frames {
        Frames::new(self.at.get().saturating_add(self.length.get()))
    }
}

/// A plan, expressed as edits to a project.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderedMix {
    operations: Vec<OperationPayload>,
    transitions: Vec<RenderedTransition>,
    duration: Frames,
}

impl RenderedMix {
    /// The operations to append to the log.
    ///
    /// Appending these *is* creating the mix. Nothing else has to happen, and
    /// nothing here has already happened — the caller decides when, and can
    /// decline.
    #[must_use]
    pub fn operations(&self) -> &[OperationPayload] {
        &self.operations
    }

    /// Consumes the render and returns its operations.
    #[must_use]
    pub fn into_operations(self) -> Vec<OperationPayload> {
        self.operations
    }

    /// Each transition, with the technique chosen and the evidence for it.
    #[must_use]
    pub fn transitions(&self) -> &[RenderedTransition] {
        &self.transitions
    }

    /// How long the rendered set runs for.
    ///
    /// Shorter than the sum of the track lengths, because the tracks overlap.
    #[must_use]
    pub const fn duration(&self) -> Frames {
        self.duration
    }
}

/// How identifiers are allocated for the placements a render creates.
///
/// The renderer must not invent identifiers, because the log's identity
/// guarantees are what everything else rests on: two devices rendering
/// concurrently must not both claim placement 7. The caller owns identity
/// allocation and hands the renderer a source.
#[derive(Debug, Clone, Copy)]
pub struct PlacementIds {
    next: u64,
}

impl PlacementIds {
    /// Starts allocating from a value the caller knows is unused.
    #[must_use]
    pub const fn starting_at(first: u64) -> Self {
        Self { next: first }
    }

    fn take(&mut self) -> PlacementId {
        let id = PlacementId::new(self.next);
        self.next = self.next.saturating_add(1);
        id
    }
}

/// Why a plan could not be rendered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum RenderError {
    /// A track in the plan is not among the candidates supplied.
    ///
    /// The renderer refuses rather than skipping. A set that quietly loses a
    /// track is a set whose energy curve no longer matches the one the user
    /// approved.
    UnknownTrack {
        /// The identifier that was not found.
        track: TrackId,
    },
    /// The plan has no tracks.
    EmptyPlan,
}

impl core::fmt::Display for RenderError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::UnknownTrack { track } => {
                write!(
                    f,
                    "track {} is in the plan but not among the candidates",
                    track.get()
                )
            }
            Self::EmptyPlan => f.write_str("the plan has no tracks"),
        }
    }
}

impl core::error::Error for RenderError {}

/// The number of lanes a rendered set uses.
///
/// Two, alternating — which is what a DJ has: two decks. A set laid out on one
/// lane per track would be technically equivalent and would look nothing like
/// what the user is doing, and Master Prompt #21 puts the timeline in front of
/// them.
const LANES: u32 = 2;

/// Renders a plan as operations.
///
/// # Errors
///
/// Returns [`RenderError`] for an empty plan or a plan naming a track that is
/// not among the candidates.
pub fn render(
    plan: &MixPlan,
    candidates: &[Candidate],
    sample_rate: SampleRate,
    ids: &mut PlacementIds,
) -> Result<RenderedMix, RenderError> {
    if plan.tracks().is_empty() {
        return Err(RenderError::EmptyPlan);
    }

    let mut operations = Vec::new();
    let mut transitions = Vec::new();
    let mut previous_start = Frames::ZERO;
    let mut previous_candidate: Option<&Candidate> = None;
    let mut end_of_set = Frames::ZERO;

    for (index, planned) in plan.tracks().iter().enumerate() {
        let Some(candidate) = find(candidates, planned.id()) else {
            return Err(RenderError::UnknownTrack {
                track: planned.id(),
            });
        };

        let lane = u32::try_from(index % (LANES as usize)).unwrap_or(0);
        let placement = ids.take();
        let overlap = overlap_for(planned, candidate, sample_rate);

        // Where this track begins. The first starts at zero; every other one
        // begins where the *outgoing* track invites it to, by the same rule the
        // planner used to decide how long the set would be.
        let start = previous_candidate.map_or(Frames::ZERO, |outgoing| {
            Frames::new(
                previous_start
                    .get()
                    .saturating_add(crate::pacing::advance(outgoing, overlap).get()),
            )
        });

        operations.push(OperationPayload::PlaceTrack {
            placement,
            track: TrackRef::new(candidate.id().get()),
            position: start,
            length: candidate.duration(),
            lane,
        });

        let earlier = index
            .checked_sub(1)
            .and_then(|position| plan.tracks().get(position));
        if let (Some(score), Some(earlier)) = (planned.transition(), earlier) {
            let choice = choose_technique(score.components());
            let transition = RenderedTransition {
                from: earlier.id(),
                to: planned.id(),
                choice,
                at: start,
                length: overlap,
            };
            let earlier_lane =
                u32::try_from(index.saturating_sub(1) % (LANES as usize)).unwrap_or(0);
            operations.extend(automation_for(&transition, lane, earlier_lane));

            // The set takes the incoming track's tempo once the transition is
            // over. During the overlap the two are matched and it is the
            // outgoing record that is still setting the pulse, so the change
            // belongs at the end rather than at the start.
            operations.push(OperationPayload::SetTempo {
                position: transition.end(),
                tempo: candidate.tempo(),
            });
            transitions.push(transition);
        } else {
            // The set opens at its first track's tempo.
            operations.push(OperationPayload::SetTempo {
                position: Frames::ZERO,
                tempo: candidate.tempo(),
            });
        }

        previous_start = start;
        previous_candidate = Some(candidate);
        // A maximum rather than this track's end: a short record placed after a
        // long one can finish before the long one does, and the set lasts until
        // the last sound stops.
        end_of_set = end_of_set.max(Frames::new(
            start.get().saturating_add(candidate.duration().get()),
        ));
    }

    Ok(RenderedMix {
        operations,
        transitions,
        duration: end_of_set,
    })
}

/// How long the overlap into a track should be.
///
/// The opening track is not transitioned into, so it has no overlap. Everything
/// else defers to [`crate::pacing::overlap`], which the planner uses too.
fn overlap_for(planned: &PlannedTrack, candidate: &Candidate, sample_rate: SampleRate) -> Frames {
    planned.transition().map_or(Frames::ZERO, |score| {
        crate::pacing::overlap(score, candidate, sample_rate)
    })
}

/// The automation a technique performs.
///
/// Every technique is expressed as automation on the two lanes rather than as a
/// special case in the renderer. That is what makes a generated transition
/// editable: a user who wants the bass to come in two bars later drags a point,
/// exactly as they would on one they built themselves.
fn automation_for(
    transition: &RenderedTransition,
    incoming_lane: u32,
    outgoing_lane: u32,
) -> Vec<OperationPayload> {
    let points = Points {
        start: transition.at,
        end: transition.end(),
        // The point at which the bass hands over: two thirds through, so the
        // incoming record has established itself before it takes the low end.
        handover: Frames::new(
            transition
                .at
                .get()
                .saturating_add(round_to_i64(signed_to_f64(transition.length.get()) * 0.67)),
        ),
    };

    let mut writer = Automation::new(
        ParameterOwner::Lane(incoming_lane),
        ParameterOwner::Lane(outgoing_lane),
    );

    match transition.choice.technique {
        Technique::Blend => blend(&mut writer, points),
        Technique::BassSwap => bass_swap(&mut writer, points),
        Technique::FilterFade => filter_fade(&mut writer, points),
        Technique::Cut => cut(&mut writer, points),
    }

    writer.finish()
}

/// The three moments a transition is built around.
#[derive(Debug, Clone, Copy)]
struct Points {
    /// Where the transition begins.
    start: Frames,
    /// Where it ends.
    end: Frames,
    /// Where the low end changes hands.
    handover: Frames,
}

/// Collects automation operations for the two lanes of a transition.
///
/// A tiny writer rather than a free function taking eight arguments, because
/// the four techniques below then read as the description of what a DJ does
/// rather than as a list of parameters.
#[derive(Debug)]
struct Automation {
    incoming: ParameterOwner,
    outgoing: ParameterOwner,
    operations: Vec<OperationPayload>,
}

impl Automation {
    fn new(incoming: ParameterOwner, outgoing: ParameterOwner) -> Self {
        Self {
            incoming,
            outgoing,
            operations: Vec::new(),
        }
    }

    fn write(
        &mut self,
        owner: ParameterOwner,
        key: ParameterKey,
        points: &[(Frames, f32, Interpolation)],
    ) {
        let Ok(address) = ParameterAddress::new(owner, key) else {
            return;
        };
        for &(position, value, interpolation) in points {
            self.operations.push(OperationPayload::SetAutomationPoint {
                address: address.clone(),
                position,
                value,
                interpolation,
            });
        }
    }

    fn incoming(&mut self, key: ParameterKey, points: &[(Frames, f32, Interpolation)]) {
        let owner = self.incoming.clone();
        self.write(owner, key, points);
    }

    fn outgoing(&mut self, key: ParameterKey, points: &[(Frames, f32, Interpolation)]) {
        let owner = self.outgoing.clone();
        self.write(owner, key, points);
    }

    fn finish(self) -> Vec<OperationPayload> {
        self.operations
    }
}

/// Both levels move smoothly; the bass hands over in one step, so there is
/// never a moment with two basslines at full level — the muddiest sound a mix
/// can make.
fn blend(writer: &mut Automation, points: Points) {
    let Points {
        start,
        end,
        handover,
    } = points;
    writer.incoming(
        ParameterKey::Gain,
        &[
            (start, 0.0, Interpolation::Smooth),
            (end, 1.0, Interpolation::Linear),
        ],
    );
    writer.outgoing(
        ParameterKey::Gain,
        &[
            (start, 1.0, Interpolation::Smooth),
            (end, 0.0, Interpolation::Linear),
        ],
    );
    writer.incoming(
        ParameterKey::EqLow,
        &[
            (start, 0.0, Interpolation::Hold),
            (handover, 1.0, Interpolation::Linear),
        ],
    );
    writer.outgoing(
        ParameterKey::EqLow,
        &[
            (start, 1.0, Interpolation::Hold),
            (handover, 0.0, Interpolation::Linear),
        ],
    );
}

/// The low end is handed over at the very start rather than part way, because
/// the reason for this technique is that the two basslines do not agree.
fn bass_swap(writer: &mut Automation, points: Points) {
    let Points { start, end, .. } = points;
    writer.incoming(
        ParameterKey::Gain,
        &[
            (start, 0.0, Interpolation::Linear),
            (end, 1.0, Interpolation::Linear),
        ],
    );
    writer.outgoing(
        ParameterKey::Gain,
        &[
            (start, 1.0, Interpolation::Linear),
            (end, 0.0, Interpolation::Linear),
        ],
    );
    writer.incoming(ParameterKey::EqLow, &[(start, 1.0, Interpolation::Hold)]);
    writer.outgoing(ParameterKey::EqLow, &[(start, 0.0, Interpolation::Hold)]);
}

/// The outgoing record is swept upward out of the arrangement while its level
/// stays put, so the change reads as it leaving rather than as someone turning
/// it down.
fn filter_fade(writer: &mut Automation, points: Points) {
    let Points { start, end, .. } = points;
    writer.incoming(
        ParameterKey::Gain,
        &[
            (start, 0.0, Interpolation::Decelerating),
            (end, 1.0, Interpolation::Linear),
        ],
    );
    writer.outgoing(
        ParameterKey::Filter,
        &[
            (start, 0.5, Interpolation::Accelerating),
            (end, 1.0, Interpolation::Linear),
        ],
    );
    writer.outgoing(
        ParameterKey::Gain,
        &[
            (start, 1.0, Interpolation::Hold),
            (end, 0.0, Interpolation::Linear),
        ],
    );
}

/// Short and square. A cut is not a fade with a small number: the point is that
/// neither record is heard at half level, because that is the part that sounds
/// wrong when two records do not fit.
fn cut(writer: &mut Automation, points: Points) {
    let Points { start, end, .. } = points;
    writer.incoming(ParameterKey::Gain, &[(start, 1.0, Interpolation::Hold)]);
    writer.outgoing(
        ParameterKey::Gain,
        &[
            (start, 1.0, Interpolation::Hold),
            (end, 0.0, Interpolation::Hold),
        ],
    );
}

/// Finds a candidate by identifier.
fn find(candidates: &[Candidate], id: TrackId) -> Option<&Candidate> {
    candidates.iter().find(|candidate| candidate.id() == id)
}

/// The number of transitions of each technique in a render.
///
/// Exposed because a summary of a set — "four blends, two cuts" — is one of the
/// few things a user reads before listening to it.
#[must_use]
pub fn technique_counts(mix: &RenderedMix) -> [(Technique, usize); 4] {
    let mut counts = [
        (Technique::Blend, 0),
        (Technique::BassSwap, 0),
        (Technique::FilterFade, 0),
        (Technique::Cut, 0),
    ];
    for transition in mix.transitions() {
        for entry in &mut counts {
            if entry.0 == transition.choice.technique {
                entry.1 += 1;
            }
        }
    }
    counts
}

/// The mean overlap of a render, in sample frames.
#[must_use]
pub fn mean_overlap_frames(mix: &RenderedMix) -> f64 {
    if mix.transitions().is_empty() {
        return 0.0;
    }
    let total: f64 = mix
        .transitions()
        .iter()
        .map(|transition| signed_to_f64(transition.length.get()))
        .sum();
    total / count_to_f64(mix.transitions().len())
}

/// Converts a technique's overlap to a normalised display value.
#[must_use]
pub fn overlap_fraction(transition: &RenderedTransition, track_length: Frames) -> f32 {
    if track_length.get() <= 0 {
        return 0.0;
    }
    narrow(
        (signed_to_f64(transition.length.get()) / signed_to_f64(track_length.get()))
            .clamp(0.0, 1.0),
    )
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
    use crate::goal::{EnergyShape, Goal};
    use crate::transition::ScoreComponents;
    use prv_analysis::Confidence;
    use prv_harmony::{Key, PitchClass};
    use prv_project::ProjectState;
    use prv_time::Tempo;

    const TRACK_FRAMES: i64 = 44_100 * 300;
    const RATE: SampleRate = SampleRate::HZ_44100;

    fn track(id: u64, tonic: PitchClass) -> Candidate {
        Candidate::new(
            TrackId::new(id),
            Frames::new(TRACK_FRAMES),
            Tempo::from_bpm(128.0).expect("valid"),
            0.5,
        )
        .with_key(Key::minor(tonic), Confidence::CERTAIN)
        .with_loudness(-8.0)
    }

    fn library() -> Vec<Candidate> {
        vec![
            track(0, PitchClass::A),
            track(1, PitchClass::E),
            track(2, PitchClass::A),
            track(3, PitchClass::E),
        ]
    }

    fn plan_for(candidates: &[Candidate]) -> MixPlan {
        let goal = Goal::new(
            Frames::new(TRACK_FRAMES * 4),
            SampleRate::HZ_44100,
            EnergyShape::Plateau,
        );
        crate::plan::plan(candidates, &goal, 1)
            .expect("a compatible library plans")
            .into_iter()
            .next()
            .expect("one plan")
    }

    fn components(harmonic: f32, tempo: f32, structure: f32, vocal: f32) -> ScoreComponents {
        ScoreComponents {
            harmonic,
            tempo,
            energy: 0.9,
            structure,
            level: 0.9,
            vocal,
        }
    }

    #[test]
    fn a_rendered_mix_is_a_list_of_edits_and_nothing_has_happened_yet() {
        // The decision the module is arranged around. Nothing is mutated;
        // appending the operations is what creates the mix, and the caller can
        // decline.
        let candidates = library();
        let plan = plan_for(&candidates);
        let mut ids = PlacementIds::starting_at(1);
        let mix = render(&plan, &candidates, RATE, &mut ids).expect("renders");

        assert!(!mix.operations().is_empty());
        let placements = mix
            .operations()
            .iter()
            .filter(|operation| matches!(operation, OperationPayload::PlaceTrack { .. }))
            .count();
        assert_eq!(placements, plan.tracks().len());
    }

    #[test]
    fn applying_the_operations_produces_the_set_and_undoing_them_removes_it() {
        // The whole return on emitting operations: an AI mix is undoable
        // because it is an ordinary edit, with no code here to make it so.
        let candidates = library();
        let plan = plan_for(&candidates);
        let mut ids = PlacementIds::starting_at(1);
        let mix = render(&plan, &candidates, RATE, &mut ids).expect("renders");

        let mut state = ProjectState::default();
        let mut inverses = Vec::new();
        for operation in mix.operations() {
            if let Some(inverse) = state.inverses_of(operation).into_iter().next() {
                inverses.push(inverse);
            }
            state.apply(operation);
        }

        assert_eq!(state.placements.len(), plan.tracks().len());
        assert!(
            !state.automation.is_empty(),
            "the transitions produced no automation"
        );

        for inverse in inverses.iter().rev() {
            state.apply(inverse);
        }
        assert!(
            state.placements.is_empty(),
            "undoing the render left placements behind"
        );
        assert!(
            state.automation.is_empty(),
            "undoing the render left automation behind"
        );
    }

    #[test]
    fn tracks_overlap_rather_than_abutting() {
        // A set where every track starts exactly where the last ended is a
        // playlist, not a mix.
        let candidates = library();
        let plan = plan_for(&candidates);
        let mut ids = PlacementIds::starting_at(1);
        let mix = render(&plan, &candidates, RATE, &mut ids).expect("renders");

        let starts: Vec<i64> = mix
            .operations()
            .iter()
            .filter_map(|operation| match operation {
                OperationPayload::PlaceTrack { position, .. } => Some(position.get()),
                _ => None,
            })
            .collect();

        assert!(starts.len() >= 2);
        for pair in starts.windows(2) {
            let (Some(&first), Some(&second)) = (pair.first(), pair.get(1)) else {
                continue;
            };
            assert!(second > first, "tracks are not in order");
            assert!(
                second - first < TRACK_FRAMES,
                "track {second} starts after the previous one ended, so they do not overlap"
            );
        }
        assert!(
            mix.duration().get() < TRACK_FRAMES * i64::try_from(starts.len()).unwrap_or(1),
            "the set is as long as the sum of its tracks, so nothing overlapped"
        );
    }

    #[test]
    fn placements_alternate_between_two_lanes() {
        // A DJ has two decks, and Master Prompt #21 puts this timeline in front
        // of them. One lane per track would be equivalent and would look
        // nothing like what they are doing.
        let candidates = library();
        let plan = plan_for(&candidates);
        let mut ids = PlacementIds::starting_at(1);
        let mix = render(&plan, &candidates, RATE, &mut ids).expect("renders");

        let lanes: Vec<u32> = mix
            .operations()
            .iter()
            .filter_map(|operation| match operation {
                OperationPayload::PlaceTrack { lane, .. } => Some(*lane),
                _ => None,
            })
            .collect();
        for (index, lane) in lanes.iter().enumerate() {
            assert_eq!(*lane, u32::try_from(index % 2).unwrap_or(0));
        }
    }

    #[test]
    fn the_technique_follows_the_evidence() {
        // Every rule stated on a variant, checked. A user shown the technique
        // name can be told exactly what was measured to produce it.
        let unreachable_tempo = choose_technique(components(0.9, 0.2, 0.9, 0.9));
        assert_eq!(unreachable_tempo.technique(), Technique::Cut);
        assert_eq!(unreachable_tempo.reason(), Component::Tempo);

        let clashing = choose_technique(components(0.2, 0.9, 0.9, 0.9));
        assert_eq!(clashing.technique(), Technique::Cut);
        assert_eq!(clashing.reason(), Component::Harmonic);

        let loose = choose_technique(components(0.55, 0.9, 0.9, 0.9));
        assert_eq!(loose.technique(), Technique::BassSwap);
        assert_eq!(loose.reason(), Component::Harmonic);

        let nowhere_to_mix = choose_technique(components(0.9, 0.9, 0.2, 0.9));
        assert_eq!(nowhere_to_mix.technique(), Technique::FilterFade);
        assert_eq!(nowhere_to_mix.reason(), Component::Structure);

        let two_vocals = choose_technique(components(0.9, 0.9, 0.9, 0.1));
        assert_eq!(two_vocals.technique(), Technique::Cut);
        assert_eq!(two_vocals.reason(), Component::Vocal);

        let ideal = choose_technique(components(0.95, 0.95, 0.9, 0.9));
        assert_eq!(ideal.technique(), Technique::Blend);
    }

    #[test]
    fn better_evidence_never_produces_a_more_cautious_technique() {
        // The property that survives recalibration. The thresholds are
        // provisional; the ordering they induce is not.
        // How much of both records is left audible. A bass swap and a filtered
        // fade are equally cautious: both keep the overlap but remove part of
        // one record.
        let caution = |technique: Technique| match technique {
            Technique::Cut => 0_u8,
            Technique::BassSwap | Technique::FilterFade => 1,
            Technique::Blend => 2,
        };

        let mut previous = 0;
        let mut step = 0;
        while step <= 100 {
            let value = narrow(f64::from(step) / 100.0);
            let choice = choose_technique(components(value, value, value, value));
            let level = caution(choice.technique());
            assert!(
                level >= previous,
                "at {value} the technique became more cautious than at the step before"
            );
            previous = level;
            step += 1;
        }
    }

    #[test]
    fn a_blend_overlaps_for_longer_than_a_cut() {
        // The length is the audible consequence of the technique choice.
        let blend = Technique::Blend.overlap_beats(0.9);
        let swap = Technique::BassSwap.overlap_beats(0.9);
        let cut = Technique::Cut.overlap_beats(0.9);
        assert!(blend > swap, "a blend is not longer than a bass swap");
        assert!(swap > cut, "a bass swap is not longer than a cut");
        assert!(blend <= MAX_OVERLAP_BEATS);
        assert!(cut >= MIN_OVERLAP_BEATS);
    }

    #[test]
    fn every_overlap_is_a_whole_number_of_bars() {
        // A transition that begins or ends mid-bar is a transition that sounds
        // late, whatever the arithmetic says.
        for technique in [
            Technique::Blend,
            Technique::BassSwap,
            Technique::FilterFade,
            Technique::Cut,
        ] {
            let mut step = 0;
            while step <= 100 {
                let beats = technique.overlap_beats(narrow(f64::from(step) / 100.0));
                assert!(
                    (beats / 4.0).fract().abs() < 1e-9,
                    "{technique:?} produced {beats} beats, which is not a whole number of bars"
                );
                step += 1;
            }
        }
    }

    #[test]
    fn a_stronger_transition_is_given_more_room() {
        let weak = Technique::Blend.overlap_beats(0.75);
        let strong = Technique::Blend.overlap_beats(1.0);
        assert!(
            strong > weak,
            "a better match was not given a longer overlap: {strong} against {weak}"
        );
    }

    #[test]
    fn every_transition_produces_automation_on_both_lanes() {
        // What makes a generated transition editable: it is expressed as
        // automation, so a user who wants the bass two bars later drags a point.
        let candidates = library();
        let plan = plan_for(&candidates);
        let mut ids = PlacementIds::starting_at(1);
        let mix = render(&plan, &candidates, RATE, &mut ids).expect("renders");

        assert!(!mix.transitions().is_empty());
        let mut state = ProjectState::default();
        for operation in mix.operations() {
            state.apply(operation);
        }

        for lane in 0..LANES {
            let address = ParameterAddress::new(ParameterOwner::Lane(lane), ParameterKey::Gain)
                .expect("valid");
            assert!(
                state.automation.contains_key(&address),
                "lane {lane} has no level automation"
            );
        }
    }

    #[test]
    fn a_plan_naming_an_unknown_track_is_refused_rather_than_shortened() {
        // A set that quietly loses a track is a set whose energy curve no
        // longer matches the one the user approved.
        let candidates = library();
        let plan = plan_for(&candidates);
        let mut ids = PlacementIds::starting_at(1);
        let missing: Vec<Candidate> = candidates.into_iter().take(1).collect();

        assert!(matches!(
            render(&plan, &missing, RATE, &mut ids),
            Err(RenderError::UnknownTrack { .. })
        ));
    }

    #[test]
    fn identifiers_come_from_the_caller_and_never_repeat() {
        // Two devices rendering concurrently must not both claim placement 7,
        // so the renderer does not invent identifiers.
        let candidates = library();
        let plan = plan_for(&candidates);

        let mut ids = PlacementIds::starting_at(500);
        let mix = render(&plan, &candidates, RATE, &mut ids).expect("renders");

        let mut seen = std::collections::BTreeSet::new();
        for operation in mix.operations() {
            if let OperationPayload::PlaceTrack { placement, .. } = operation {
                assert!(
                    placement.get() >= 500,
                    "an identifier below the start was used"
                );
                assert!(seen.insert(placement.get()), "an identifier repeated");
            }
        }
    }

    #[test]
    fn rendering_is_reproducible() {
        let candidates = library();
        let plan = plan_for(&candidates);

        let mut first_ids = PlacementIds::starting_at(1);
        let mut second_ids = PlacementIds::starting_at(1);
        let first = render(&plan, &candidates, RATE, &mut first_ids).expect("renders");
        let second = render(&plan, &candidates, RATE, &mut second_ids).expect("renders");

        assert_eq!(first.operations(), second.operations());
        assert_eq!(first.transitions(), second.transitions());
        assert_eq!(first.duration(), second.duration());
    }

    #[test]
    fn an_empty_plan_is_reported() {
        let candidates = library();
        let plan = plan_for(&candidates);
        let mut ids = PlacementIds::starting_at(1);
        // A plan is never empty by construction, so this exercises the guard
        // rather than a reachable state — which is the point of having it.
        let empty = MixPlan::empty();
        assert_eq!(
            render(&empty, &candidates, RATE, &mut ids).err(),
            Some(RenderError::EmptyPlan)
        );
        assert!(render(&plan, &candidates, RATE, &mut ids).is_ok());
    }

    #[test]
    fn the_summary_counts_what_the_render_produced() {
        let candidates = library();
        let plan = plan_for(&candidates);
        let mut ids = PlacementIds::starting_at(1);
        let mix = render(&plan, &candidates, RATE, &mut ids).expect("renders");

        let counts = technique_counts(&mix);
        let total: usize = counts.iter().map(|(_, count)| count).sum();
        assert_eq!(total, mix.transitions().len());
        assert!(mean_overlap_frames(&mix) > 0.0);

        let Some(transition) = mix.transitions().first() else {
            return;
        };
        let fraction = overlap_fraction(transition, Frames::new(TRACK_FRAMES));
        assert!((0.0..=1.0).contains(&fraction));
        assert_eq!(overlap_fraction(transition, Frames::ZERO), 0.0);
    }

    #[test]
    fn a_transition_lands_on_the_outgoing_tracks_exit_point() {
        // The naive placement mixes into whatever the outgoing record happens
        // to be doing at the end. The analysis has already found where it wants
        // to be left, and this is what uses it.
        use crate::candidate::{MixPoint, MixPointRole};

        let exit_at = Frames::new(TRACK_FRAMES >> 1);
        let with_exit =
            track(0, PitchClass::A).with_point(MixPoint::new(exit_at, 0.1, MixPointRole::Exit));
        let candidates = vec![with_exit, track(1, PitchClass::E)];

        let goal = Goal::new(
            Frames::new(TRACK_FRAMES * 2),
            SampleRate::HZ_44100,
            EnergyShape::Plateau,
        );
        let plan = crate::plan::plan(&candidates, &goal, 1)
            .expect("plans")
            .into_iter()
            .next()
            .expect("one plan");

        // The planner may open with either track; only the case where the one
        // with the exit point goes first tells us anything.
        if plan.tracks().first().map(PlannedTrack::id) != Some(TrackId::new(0)) {
            return;
        }

        let mut ids = PlacementIds::starting_at(1);
        let mix = render(&plan, &candidates, RATE, &mut ids).expect("renders");
        let Some(transition) = mix.transitions().first() else {
            return;
        };
        assert_eq!(
            transition.at(),
            exit_at,
            "the transition did not begin at the outgoing track's exit point"
        );
    }

    #[test]
    fn a_rendered_set_is_as_long_as_the_plan_said_it_would_be() {
        // The property that was violated, and the reason `pacing` exists.
        //
        // The planner laid tracks end to end while the renderer started each one
        // at the outgoing track's exit point. With exit points a quarter of the
        // way in — ordinary for analysed material — a forty-minute plan rendered
        // as a thirteen-minute mix, and `duration_error` reported the set as a
        // perfect match for what the user asked for.
        use crate::candidate::{MixPoint, MixPointRole};

        let candidates: Vec<Candidate> = (0..8)
            .map(|index| {
                let tonic = match index % 3 {
                    0 => PitchClass::A,
                    1 => PitchClass::E,
                    _ => PitchClass::D,
                };
                // A quarter of the way in: early, but ordinary for a record
                // with a long outro, and the case the planner used to lose.
                track(index, tonic).with_point(MixPoint::new(
                    Frames::new(TRACK_FRAMES >> 2),
                    0.05,
                    MixPointRole::Exit,
                ))
            })
            .collect();

        let goal = Goal::new(Frames::new(TRACK_FRAMES * 8), RATE, EnergyShape::Plateau);
        let plan = crate::plan::plan(&candidates, &goal, 1)
            .expect("plans")
            .into_iter()
            .next()
            .expect("one plan");

        let mut ids = PlacementIds::starting_at(1);
        let mix = render(&plan, &candidates, RATE, &mut ids).expect("renders");

        assert_eq!(
            plan.duration(),
            mix.duration(),
            "the plan and the mix disagree about how long the set is"
        );

        // And every track sits where the plan said it would.
        let placed: Vec<Frames> = mix
            .operations()
            .iter()
            .filter_map(|operation| match operation {
                OperationPayload::PlaceTrack { position, .. } => Some(*position),
                _ => None,
            })
            .collect();
        let planned: Vec<Frames> = plan.tracks().iter().map(PlannedTrack::start).collect();
        assert_eq!(placed, planned, "a track was rendered somewhere else");
    }

    #[test]
    fn a_short_set_is_reported_as_short_rather_than_as_a_perfect_match() {
        // The half of the defect that made it dangerous rather than merely
        // wrong. A planner that comes up short can say so and a user can ask for
        // more; one that comes up short and reports success cannot be caught —
        // and `duration_error` is also what the search sorts by, so the wrong
        // number was choosing between plans as well as describing them.
        use crate::candidate::{MixPoint, MixPointRole};

        let candidates: Vec<Candidate> = (0..4)
            .map(|index| {
                let tonic = match index % 3 {
                    0 => PitchClass::A,
                    1 => PitchClass::E,
                    _ => PitchClass::D,
                };
                // A quarter of the way in: early, but ordinary for a record
                // with a long outro, and the case the planner used to lose.
                track(index, tonic).with_point(MixPoint::new(
                    Frames::new(TRACK_FRAMES >> 2),
                    0.05,
                    MixPointRole::Exit,
                ))
            })
            .collect();

        // Four records that each hand over a quarter of the way in cannot fill
        // four records' worth of time.
        let goal = Goal::new(Frames::new(TRACK_FRAMES * 4), RATE, EnergyShape::Plateau);
        let plan = crate::plan::plan(&candidates, &goal, 1)
            .expect("plans")
            .into_iter()
            .next()
            .expect("one plan");

        assert!(
            plan.duration_error(&goal) > 0.25,
            "a set less than half the requested length reported an error of {}",
            plan.duration_error(&goal)
        );
    }

    #[test]
    fn the_set_records_the_tempo_it_runs_at() {
        // Two records playing together run at one tempo, and it belongs to the
        // set rather than to either of them. Without this the timeline has no
        // grid and every downstream feature that snaps has nothing to snap to.
        let candidates = library();
        let plan = plan_for(&candidates);
        let mut ids = PlacementIds::starting_at(1);
        let mix = render(&plan, &candidates, RATE, &mut ids).expect("renders");

        let mut state = ProjectState::default();
        for operation in mix.operations() {
            state.apply(operation);
        }

        assert!(
            state.tempo_changes.contains_key(&0),
            "the set does not record the tempo it opens at"
        );
        assert_eq!(
            state.tempo_changes.len(),
            plan.tracks().len(),
            "there should be one tempo change per track"
        );

        // The change belongs at the *end* of each transition: during the
        // overlap the two records are matched and the outgoing one is still
        // setting the pulse.
        for transition in mix.transitions() {
            assert!(
                state.tempo_changes.contains_key(&transition.end().get()),
                "no tempo change at the end of a transition"
            );
        }
    }

    #[test]
    fn a_tempo_change_undoes_like_any_other_edit() {
        let candidates = library();
        let plan = plan_for(&candidates);
        let mut ids = PlacementIds::starting_at(1);
        let mix = render(&plan, &candidates, RATE, &mut ids).expect("renders");

        let mut state = ProjectState::default();
        let mut inverses = Vec::new();
        for operation in mix.operations() {
            if let Some(inverse) = state.inverses_of(operation).into_iter().next() {
                inverses.push(inverse);
            }
            state.apply(operation);
        }
        assert!(!state.tempo_changes.is_empty());

        for inverse in inverses.iter().rev() {
            state.apply(inverse);
        }
        assert!(
            state.tempo_changes.is_empty(),
            "undoing the render left tempo changes behind"
        );
    }

    #[test]
    fn technique_keys_are_distinct() {
        let keys = [
            Technique::Blend.key(),
            Technique::BassSwap.key(),
            Technique::FilterFade.key(),
            Technique::Cut.key(),
        ];
        for (index, key) in keys.iter().enumerate() {
            for (other, value) in keys.iter().enumerate() {
                assert!(index == other || key != value, "two techniques share {key}");
            }
        }
    }
}
