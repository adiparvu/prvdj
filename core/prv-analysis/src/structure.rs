//! Structure: where the sections of a track begin and end.
//!
//! # What a DJ needs from this
//!
//! Master Prompt #20 requires structure detection, and Master Prompt #21 draws
//! the result on the timeline. The use is concrete: a transition begins at a
//! section boundary and not in the middle of a phrase, an intro is what you mix
//! into, an outro is what you mix out of, and the breakdown before a drop is
//! where the energy of a set is rebuilt. Getting the boundaries wrong by four
//! bars is the difference between a transition that lands and one that does
//! not.
//!
//! # Sections are found on the grid, not in spite of it
//!
//! Almost every produced record changes section on a bar line, and usually on a
//! phrase of four, eight or sixteen bars. So the analysis works at *bar*
//! resolution rather than at frame resolution: features are averaged over each
//! bar of the beat grid, and boundaries fall between bars by construction.
//!
//! This is not a shortcut. A frame-resolution segmenter has to find a boundary
//! that is already known to be at a bar line, and its extra freedom is entirely
//! freedom to be wrong — reporting a change at 1:04.3 when the music changed at
//! 1:04.0. Constraining the search to the grid removes a whole class of error
//! and costs nothing, because the answer was never anywhere else.
//!
//! # How a boundary is recognised
//!
//! Each bar becomes a feature vector: twelve pitch classes and three energy
//! bands. Two bars are compared by cosine similarity, and a boundary is a
//! position where the bars *before* resemble each other, the bars *after*
//! resemble each other, and the two groups do not resemble each other. That is
//! Foote's checkerboard novelty, and it is the right shape because it looks for
//! a change in *what is repeating* rather than a change in level. A filter
//! sweep changes the spectrum without changing the section; a new eight bars
//! with the same instruments at a new energy is a section change. Novelty over
//! a self-similarity matrix separates the two; a level detector does not.
//!
//! # What is deliberately not done
//!
//! Sections are not named "verse", "chorus" or "drop". Those names are genre
//! conventions rather than acoustic facts, and a system that prints "drop" over
//! the wrong eight bars is worse than one that prints nothing: the user stops
//! reading the labels. What *is* reported is what was measured — the energy of
//! each section relative to the track, and which sections resemble each other —
//! plus a small set of positional kinds that follow from the energy curve alone
//! and are defined here rather than assumed.

use prv_time::SampleRate;

use crate::chroma::ChromaBuilder;
use crate::confidence::Confidence;
use crate::error::AnalysisError;
use crate::num::{count_to_f64, narrow, round_to_count};
use crate::spectrum::Stft;

/// The number of pitch classes in a bar's feature vector.
const PITCH_CLASSES: usize = 12;

/// The number of energy bands in a bar's feature vector.
const ENERGY_BANDS: usize = 3;

/// The length of a bar's feature vector.
const FEATURE_LENGTH: usize = PITCH_CLASSES + ENERGY_BANDS;

/// The crossover between the low and mid energy bands, in hertz.
const LOW_MID_HZ: f64 = 250.0;

/// The crossover between the mid and high energy bands, in hertz.
const MID_HIGH_HZ: f64 = 4_000.0;

/// The half-width of the novelty kernel, in bars.
///
/// Four bars either side. A phrase in dance music is four, eight or sixteen
/// bars, so four is the largest half-width that can still resolve the shortest
/// of them. A wider kernel is more robust and would merge a four-bar fill into
/// its neighbours.
const KERNEL_BARS: usize = 4;

/// The shortest section reported, in bars.
///
/// Four. Shorter than this is a fill or a one-bar drum edit, not a section, and
/// reporting them would bury the four boundaries a user actually wants among
/// forty they do not.
const MINIMUM_SECTION_BARS: usize = 4;

/// The smallest novelty score that can be a boundary.
///
/// The threshold is otherwise purely relative — a standard deviation above the
/// track's own mean — and a purely relative threshold has no floor: on a loop
/// that never changes it faithfully reports the four bars that happened to
/// differ most, and a user shown four boundaries believes there are four
/// sections.
///
/// The novelty is a difference of cosine similarities, so it has an absolute
/// meaning: 0.05 says the bars on either side are five per cent less alike than
/// the bars within each side. An unvarying loop scores around a hundredth of
/// that; an arrangement change scores several times it. The floor sits in the
/// gap rather than on a boundary real music straddles.
const MINIMUM_NOVELTY: f64 = 0.05;

/// How a section sits in the shape of the track.
///
/// Derived from the energy curve and position alone. Every variant is defined
/// by a rule stated here, so a user shown one of these words can be told
/// exactly what was measured to produce it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SectionKind {
    /// The first section, when it is quieter than the track's average.
    ///
    /// What a DJ mixes into.
    Intro,
    /// The last section, when it is quieter than the track's average.
    ///
    /// What a DJ mixes out of.
    Outro,
    /// A local minimum of energy with louder sections on both sides.
    ///
    /// The place a set's energy is rebuilt from, and the safest place to bring
    /// a new track in.
    Breakdown,
    /// A section louder than the one before it and no louder than the one after.
    Build,
    /// A local maximum of energy.
    Peak,
    /// Anything else: the body of the track.
    Body,
}

/// One section of a track.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Section {
    start: usize,
    end: usize,
    start_bar: usize,
    bars: usize,
    energy: f32,
    kind: SectionKind,
    group: usize,
}

impl Section {
    /// The sample offset the section begins at.
    #[must_use]
    pub const fn start(&self) -> usize {
        self.start
    }

    /// The sample offset the section ends at, exclusive.
    #[must_use]
    pub const fn end(&self) -> usize {
        self.end
    }

    /// The bar of the beat grid the section begins at.
    #[must_use]
    pub const fn start_bar(&self) -> usize {
        self.start_bar
    }

    /// The section's length in bars.
    #[must_use]
    pub const fn bars(&self) -> usize {
        self.bars
    }

    /// The section's mean energy, relative to the loudest section of the track.
    ///
    /// One is the loudest section. Relative rather than absolute because the
    /// question a user asks of a section is always comparative — is this
    /// quieter than the drop — and an absolute figure would need the track's
    /// mastering level to answer it.
    #[must_use]
    pub const fn energy(&self) -> f32 {
        self.energy
    }

    /// How the section sits in the shape of the track.
    #[must_use]
    pub const fn kind(&self) -> SectionKind {
        self.kind
    }

    /// Which group of similar sections this belongs to.
    ///
    /// Sections that repeat — the same eight bars returning after a breakdown —
    /// share a group. This is what lets an interface say "this is the same
    /// material as the section at 1:20" without claiming to know that either of
    /// them is a chorus.
    #[must_use]
    pub const fn group(&self) -> usize {
        self.group
    }
}

/// The structure of a track.
#[derive(Debug, Clone)]
pub struct Structure {
    sections: Vec<Section>,
    confidence: Confidence,
}

impl Structure {
    /// The sections, in order.
    #[must_use]
    pub fn sections(&self) -> &[Section] {
        &self.sections
    }

    /// How clearly the boundaries stood out.
    ///
    /// Low on material that genuinely has no sections — a single sustained
    /// texture, a live recording with no arrangement — and that is the honest
    /// answer rather than a set of boundaries placed at regular intervals.
    #[must_use]
    pub const fn confidence(&self) -> Confidence {
        self.confidence
    }

    /// The section containing a sample position, if any.
    #[must_use]
    pub fn section_at(&self, sample: usize) -> Option<&Section> {
        self.sections
            .iter()
            .find(|section| sample >= section.start && sample < section.end)
    }

    /// The sections a transition can safely begin in, quietest first.
    ///
    /// Intros, outros and breakdowns: the places where a second track can enter
    /// without two arrangements colliding. Exposed here rather than left to
    /// each caller so that the planner and the interface agree about what is
    /// safe.
    #[must_use]
    pub fn transition_points(&self) -> Vec<&Section> {
        let mut candidates: Vec<&Section> = self
            .sections
            .iter()
            .filter(|section| {
                matches!(
                    section.kind,
                    SectionKind::Intro | SectionKind::Outro | SectionKind::Breakdown
                )
            })
            .collect();
        candidates.sort_by(|a, b| {
            a.energy
                .partial_cmp(&b.energy)
                .unwrap_or(core::cmp::Ordering::Equal)
                .then_with(|| a.start.cmp(&b.start))
        });
        candidates
    }
}

/// Finds the structure of a signal, given its beat grid.
///
/// # Errors
///
/// Returns [`AnalysisError::NotEnoughAudio`] for material with fewer bars than
/// one novelty kernel spans, and propagates transform errors.
pub fn detect(
    stft: &mut Stft,
    samples: &[f32],
    sample_rate: SampleRate,
    beats: &crate::beats::BeatEstimate,
) -> Result<Structure, AnalysisError> {
    let bar_starts = bar_boundaries(samples.len(), beats);
    let minimum_bars = KERNEL_BARS * 2 + MINIMUM_SECTION_BARS;
    if bar_starts.len() < minimum_bars {
        return Err(AnalysisError::NotEnoughAudio {
            frames: samples.len(),
            minimum: minimum_bars,
        });
    }

    let features = bar_features(stft, samples, sample_rate, &bar_starts)?;
    let novelty = checkerboard_novelty(&features);
    let boundaries = pick_boundaries(&novelty);

    let sections = build_sections(&bar_starts, &features, &boundaries, samples.len());
    let confidence = boundary_confidence(&novelty, &boundaries);

    Ok(Structure {
        sections,
        confidence,
    })
}

/// The sample offset at which each bar begins.
fn bar_boundaries(length: usize, beats: &crate::beats::BeatEstimate) -> Vec<usize> {
    let mut starts: Vec<usize> = beats
        .beats()
        .iter()
        .filter(|beat| beat.is_downbeat())
        .map(crate::beats::Beat::sample)
        .filter(|&sample| sample < length)
        .collect();
    starts.sort_unstable();
    starts.dedup();
    starts
}

/// One feature vector per bar.
fn bar_features(
    stft: &mut Stft,
    samples: &[f32],
    sample_rate: SampleRate,
    bar_starts: &[usize],
) -> Result<Vec<[f32; FEATURE_LENGTH]>, AnalysisError> {
    let mut builders: Vec<ChromaBuilder> = vec![ChromaBuilder::new(); bar_starts.len()];
    let mut bands = vec![[0.0_f64; ENERGY_BANDS]; bar_starts.len()];
    let mut counts = vec![0_usize; bar_starts.len()];

    stft.analyse(samples, sample_rate, |frame| {
        // Which bar this frame falls in. `partition_point` rather than a linear
        // scan: a ten-minute track has tens of thousands of frames, and a scan
        // per frame would make this quadratic in the track length.
        let bar = bar_starts.partition_point(|&start| start <= frame.centre_sample());
        let Some(bar) = bar.checked_sub(1) else {
            return;
        };

        if let Some(builder) = builders.get_mut(bar) {
            builder.add(frame);
        }

        let magnitude = frame.magnitude();
        let low_edge = frame.bin_for_frequency(LOW_MID_HZ);
        let high_edge = frame.bin_for_frequency(MID_HIGH_HZ);
        let mut totals = [0.0_f64; ENERGY_BANDS];
        for (bin, &value) in magnitude.iter().enumerate() {
            let band = if bin <= low_edge {
                0
            } else if bin <= high_edge {
                1
            } else {
                2
            };
            if let Some(slot) = totals.get_mut(band) {
                *slot += value * value;
            }
        }
        if let Some(slot) = bands.get_mut(bar) {
            for (accumulated, total) in slot.iter_mut().zip(totals.iter()) {
                *accumulated += total.sqrt();
            }
        }
        if let Some(slot) = counts.get_mut(bar) {
            *slot += 1;
        }
    })?;

    let mut features = Vec::with_capacity(bar_starts.len());
    for (index, builder) in builders.iter().enumerate() {
        let mut vector = [0.0_f32; FEATURE_LENGTH];
        let chroma = builder.finish();
        for (slot, &weight) in vector.iter_mut().zip(chroma.weights().iter()) {
            *slot = weight;
        }
        let count = counts.get(index).copied().unwrap_or(0).max(1);
        if let Some(band) = bands.get(index) {
            for (offset, &energy) in band.iter().enumerate() {
                if let Some(slot) = vector.get_mut(PITCH_CLASSES + offset) {
                    *slot = narrow(energy / count_to_f64(count));
                }
            }
        }
        features.push(vector);
    }

    Ok(features)
}

/// Cosine similarity between two feature vectors.
///
/// Cosine rather than Euclidean distance because it compares *shape* rather
/// than magnitude. Two eight-bar passages with the same instruments at
/// different levels are the same section played louder, and a distance measure
/// would call them different.
fn similarity(a: &[f32; FEATURE_LENGTH], b: &[f32; FEATURE_LENGTH]) -> f64 {
    let mut dot = 0.0_f64;
    let mut left = 0.0_f64;
    let mut right = 0.0_f64;
    for (&x, &y) in a.iter().zip(b.iter()) {
        let x = f64::from(x);
        let y = f64::from(y);
        dot += x * y;
        left += x * x;
        right += y * y;
    }
    let denominator = (left * right).sqrt();
    if denominator <= 0.0 {
        return 0.0;
    }
    (dot / denominator).clamp(-1.0, 1.0)
}

/// Foote's checkerboard novelty over the bar self-similarity.
///
/// At each bar, the score is how much the preceding bars resemble each other,
/// plus how much the following bars resemble each other, minus how much the two
/// groups resemble one another. A section boundary scores high; a gradual filter
/// sweep does not, because it changes both groups equally.
fn checkerboard_novelty(features: &[[f32; FEATURE_LENGTH]]) -> Vec<f64> {
    let mut novelty = vec![0.0_f64; features.len()];
    if features.len() < KERNEL_BARS * 2 {
        return novelty;
    }

    for centre in KERNEL_BARS..(features.len() - KERNEL_BARS) {
        let mut before = 0.0_f64;
        let mut after = 0.0_f64;
        let mut across = 0.0_f64;

        for i in 0..KERNEL_BARS {
            for j in 0..KERNEL_BARS {
                let (Some(left_a), Some(left_b)) = (
                    features.get(centre - KERNEL_BARS + i),
                    features.get(centre - KERNEL_BARS + j),
                ) else {
                    continue;
                };
                let (Some(right_a), Some(right_b)) =
                    (features.get(centre + i), features.get(centre + j))
                else {
                    continue;
                };
                before += similarity(left_a, left_b);
                after += similarity(right_a, right_b);
                across += similarity(left_a, right_b);
            }
        }

        let cells = count_to_f64(KERNEL_BARS * KERNEL_BARS);
        if let Some(slot) = novelty.get_mut(centre) {
            *slot = ((before + after - 2.0 * across) / (2.0 * cells)).max(0.0);
        }
    }

    novelty
}

/// Picks boundaries from the novelty curve.
fn pick_boundaries(novelty: &[f64]) -> Vec<usize> {
    let positive: Vec<f64> = novelty
        .iter()
        .copied()
        .filter(|&value| value > 0.0)
        .collect();
    if positive.is_empty() {
        return Vec::new();
    }
    let mean = positive.iter().sum::<f64>() / count_to_f64(positive.len());
    let variance = positive
        .iter()
        .map(|&value| {
            let centred = value - mean;
            centred * centred
        })
        .sum::<f64>()
        / count_to_f64(positive.len());
    // A boundary must stand a standard deviation above the ordinary bar-to-bar
    // change. A track with no arrangement has a flat novelty curve, so nothing
    // clears this and no boundaries are reported — which is the correct answer
    // for such a track rather than a failure to find any.
    let threshold = (mean + variance.sqrt()).max(MINIMUM_NOVELTY);

    let mut boundaries = Vec::new();
    for (bar, &value) in novelty.iter().enumerate() {
        if value < threshold {
            continue;
        }
        let higher_neighbour = novelty
            .get(bar.wrapping_sub(1))
            .is_some_and(|&previous| previous > value)
            || novelty.get(bar + 1).is_some_and(|&next| next > value);
        if higher_neighbour {
            continue;
        }
        if let Some(&last) = boundaries.last() {
            if bar.saturating_sub(last) < MINIMUM_SECTION_BARS {
                continue;
            }
        }
        boundaries.push(bar);
    }
    boundaries
}

/// Turns boundaries into sections and classifies them.
fn build_sections(
    bar_starts: &[usize],
    features: &[[f32; FEATURE_LENGTH]],
    boundaries: &[usize],
    length: usize,
) -> Vec<Section> {
    let mut edges = vec![0_usize];
    edges.extend(boundaries.iter().copied());
    edges.push(bar_starts.len());
    edges.dedup();

    let mut sections: Vec<Section> = Vec::new();
    for pair in edges.windows(2) {
        let (Some(&from), Some(&to)) = (pair.first(), pair.get(1)) else {
            continue;
        };
        if to <= from {
            continue;
        }
        let start = bar_starts.get(from).copied().unwrap_or(0);
        let end = bar_starts.get(to).copied().unwrap_or(length).min(length);
        if end <= start {
            continue;
        }

        let mut energy = 0.0_f64;
        let mut counted = 0_usize;
        for bar in from..to {
            let Some(feature) = features.get(bar) else {
                continue;
            };
            for offset in 0..ENERGY_BANDS {
                energy += f64::from(feature.get(PITCH_CLASSES + offset).copied().unwrap_or(0.0));
            }
            counted += 1;
        }
        let mean_energy = if counted == 0 {
            0.0
        } else {
            energy / count_to_f64(counted)
        };

        sections.push(Section {
            start,
            end,
            start_bar: from,
            bars: to - from,
            energy: narrow(mean_energy),
            kind: SectionKind::Body,
            group: 0,
        });
    }

    normalise_energy(&mut sections);
    classify(&mut sections);
    group_similar(&mut sections, features);
    sections
}

/// Scales section energies so that the loudest is one.
fn normalise_energy(sections: &mut [Section]) {
    let loudest = sections
        .iter()
        .map(|section| f64::from(section.energy))
        .fold(0.0_f64, f64::max);
    if loudest <= 0.0 {
        return;
    }
    for section in sections.iter_mut() {
        section.energy = narrow(f64::from(section.energy) / loudest);
    }
}

/// Assigns a kind to each section from the energy curve and position.
fn classify(sections: &mut [Section]) {
    let count = sections.len();
    if count == 0 {
        return;
    }
    let mean = sections
        .iter()
        .map(|section| f64::from(section.energy))
        .sum::<f64>()
        / count_to_f64(count);

    let energies: Vec<f64> = sections
        .iter()
        .map(|section| f64::from(section.energy))
        .collect();

    for (index, section) in sections.iter_mut().enumerate() {
        let energy = energies.get(index).copied().unwrap_or(0.0);
        let before = index.checked_sub(1).and_then(|i| energies.get(i)).copied();
        let after = energies.get(index + 1).copied();

        section.kind = if index == 0 && energy < mean {
            SectionKind::Intro
        } else if index + 1 == count && energy < mean {
            SectionKind::Outro
        } else {
            match (before, after) {
                (Some(before), Some(after)) if energy < before && energy < after => {
                    SectionKind::Breakdown
                }
                (Some(before), Some(after)) if energy > before && energy <= after => {
                    SectionKind::Build
                }
                (Some(before), Some(after)) if energy > before && energy > after => {
                    SectionKind::Peak
                }
                (None, Some(after)) if energy > after => SectionKind::Peak,
                (Some(before), None) if energy > before => SectionKind::Peak,
                _ => SectionKind::Body,
            }
        };
    }
}

/// Groups sections that resemble one another.
fn group_similar(sections: &mut [Section], features: &[[f32; FEATURE_LENGTH]]) {
    /// How alike two sections must be to be called the same material.
    ///
    /// Cosine similarity of 0.9 over a fifteen-dimensional feature is a strong
    /// match; anything looser starts grouping a verse with its own chorus
    /// because both use the track's key.
    const SAME_MATERIAL: f64 = 0.9;

    let averages: Vec<[f32; FEATURE_LENGTH]> = sections
        .iter()
        .map(|section| average_feature(features, section.start_bar, section.bars))
        .collect();

    let mut next_group = 0_usize;
    let mut assigned: Vec<Option<usize>> = vec![None; sections.len()];

    for index in 0..sections.len() {
        if assigned.get(index).copied().flatten().is_some() {
            continue;
        }
        let group = next_group;
        next_group += 1;
        if let Some(slot) = assigned.get_mut(index) {
            *slot = Some(group);
        }
        let Some(reference) = averages.get(index) else {
            continue;
        };
        for other in (index + 1)..sections.len() {
            if assigned.get(other).copied().flatten().is_some() {
                continue;
            }
            let Some(candidate) = averages.get(other) else {
                continue;
            };
            if similarity(reference, candidate) >= SAME_MATERIAL {
                if let Some(slot) = assigned.get_mut(other) {
                    *slot = Some(group);
                }
            }
        }
    }

    for (section, group) in sections.iter_mut().zip(assigned.iter()) {
        section.group = group.unwrap_or(0);
    }
}

/// The mean feature vector over a span of bars.
fn average_feature(
    features: &[[f32; FEATURE_LENGTH]],
    start: usize,
    bars: usize,
) -> [f32; FEATURE_LENGTH] {
    let mut total = [0.0_f64; FEATURE_LENGTH];
    let mut counted = 0_usize;
    for bar in start..(start + bars) {
        let Some(feature) = features.get(bar) else {
            continue;
        };
        for (slot, &value) in total.iter_mut().zip(feature.iter()) {
            *slot += f64::from(value);
        }
        counted += 1;
    }
    let mut average = [0.0_f32; FEATURE_LENGTH];
    if counted == 0 {
        return average;
    }
    for (slot, &value) in average.iter_mut().zip(total.iter()) {
        *slot = narrow(value / count_to_f64(counted));
    }
    average
}

/// How clearly the boundaries stood out from ordinary bar-to-bar variation.
fn boundary_confidence(novelty: &[f64], boundaries: &[usize]) -> Confidence {
    if boundaries.is_empty() || novelty.is_empty() {
        return Confidence::NONE;
    }
    let background = novelty.iter().copied().filter(|&value| value > 0.0);
    let collected: Vec<f64> = background.collect();
    if collected.is_empty() {
        return Confidence::NONE;
    }
    let mean = collected.iter().sum::<f64>() / count_to_f64(collected.len());
    if mean <= 0.0 {
        return Confidence::NONE;
    }

    let mut total = 0.0_f64;
    for &bar in boundaries {
        total += novelty.get(bar).copied().unwrap_or(0.0);
    }
    let boundary_mean = total / count_to_f64(boundaries.len());

    // A boundary three times the ordinary bar-to-bar change is conclusive. The
    // scale is provisional; the ordering is not.
    Confidence::from_f64(((boundary_mean / mean) / 3.0).clamp(0.0, 1.0))
}

/// The number of bars a section of a given sample length spans at a period.
///
/// Exposed for callers that need to reason about section length in musical
/// rather than sample terms.
#[must_use]
pub fn bars_in(samples: usize, beat_period: f64, beats_per_bar: usize) -> usize {
    if beat_period <= 0.0 || beats_per_bar == 0 {
        return 0;
    }
    round_to_count(count_to_f64(samples) / (beat_period * count_to_f64(beats_per_bar)))
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
    use crate::{beats, tempo, NoveltyCurve};

    /// Builds a track with a deliberate arrangement: a quiet intro, a loud
    /// body, a breakdown and a loud finish, all on bar lines.
    fn arranged_track(rate: SampleRate) -> (Vec<f32>, usize) {
        let period = samples(0.5, rate); // 120 BPM
        let bar = period * 4;
        let bars = 32_usize;
        let mut signal = vec![0.0_f32; bar * bars];

        for index in 0..(bars * 4) {
            write_click(&mut signal, index * period, 0xC0DE + index as u64, rate);
        }

        // A bass note under every beat of the loud sections, and a sparse pad
        // in the quiet ones. The sections differ in both spectrum and energy,
        // which is what a real arrangement change does.
        let low_note = tone(55.0, period, 0.9, rate);
        let pad = tone(440.0, bar, 0.06, rate);

        let loud_bars: Vec<usize> = (8..16).chain(24..32).collect();
        for &bar_index in &loud_bars {
            for beat in 0..4 {
                let start = bar_index * bar + beat * period;
                for (offset, &value) in low_note.iter().enumerate() {
                    if let Some(slot) = signal.get_mut(start + offset) {
                        *slot += value;
                    }
                }
            }
        }
        for bar_index in (0..8).chain(16..24) {
            let start = bar_index * bar;
            for (offset, &value) in pad.iter().enumerate() {
                if let Some(slot) = signal.get_mut(start + offset) {
                    *slot += value;
                }
            }
        }

        (signal, bar)
    }

    fn analyse(rate: SampleRate) -> (Structure, usize) {
        let (signal, bar) = arranged_track(rate);
        let mut rhythm = Stft::for_rhythm().expect("valid");
        let curve = NoveltyCurve::compute(&mut rhythm, &signal, rate).expect("valid");
        let estimate = tempo::estimate(&curve).expect("has a tempo");
        let tracked = beats::track(&curve, &estimate).expect("has beats");

        let mut tonal = Stft::for_tone().expect("valid");
        let structure =
            detect(&mut tonal, &signal, rate, &tracked).expect("an arranged track has structure");
        (structure, bar)
    }

    #[test]
    fn boundaries_land_on_the_arrangement_changes() {
        let rate = SampleRate::HZ_44100;
        let (structure, bar) = analyse(rate);
        let sections = structure.sections();
        assert!(
            sections.len() >= 3,
            "only {} sections found in a track with four",
            sections.len()
        );

        // The arrangement changes at bars 8, 16 and 24. Each must be within two
        // bars of a reported boundary — two bars because the novelty kernel
        // averages over four and cannot resolve more finely than that.
        for change in [8_usize, 16, 24] {
            let expected = change * bar;
            let nearest = sections
                .iter()
                .map(|section| section.start().abs_diff(expected))
                .min()
                .unwrap_or(usize::MAX);
            assert!(
                nearest <= bar * 2,
                "the change at bar {change} is {nearest} samples from the nearest boundary"
            );
        }
    }

    #[test]
    fn sections_tile_the_track_without_gaps_or_overlaps() {
        // The property every consumer relies on: a timeline drawn from these
        // must not have holes, and `section_at` must never be ambiguous.
        let rate = SampleRate::HZ_44100;
        let (structure, _) = analyse(rate);
        let sections = structure.sections();

        for pair in sections.windows(2) {
            let (Some(first), Some(second)) = (pair.first(), pair.get(1)) else {
                continue;
            };
            assert_eq!(
                first.end(),
                second.start(),
                "a gap or overlap between sections"
            );
            assert!(first.end() > first.start(), "an empty section");
        }
    }

    #[test]
    fn the_quiet_sections_are_recognised_as_places_to_mix() {
        let rate = SampleRate::HZ_44100;
        let (structure, _) = analyse(rate);

        let points = structure.transition_points();
        assert!(
            !points.is_empty(),
            "no transition points found in a track with a quiet intro and a breakdown"
        );
        // They are ordered quietest first, which is what makes the list usable
        // without re-sorting it at every call site.
        for pair in points.windows(2) {
            let (Some(first), Some(second)) = (pair.first(), pair.get(1)) else {
                continue;
            };
            assert!(first.energy() <= second.energy());
        }
        // The first section is quiet, so it should be an intro.
        let first = structure.sections().first().expect("at least one section");
        assert_eq!(first.kind(), SectionKind::Intro);
    }

    #[test]
    fn repeated_material_shares_a_group() {
        // Bars 8 to 16 and 24 to 32 are the same arrangement. Whatever the
        // boundaries turn out to be, two sections covering that material must
        // be grouped together — that is what lets an interface say "this is the
        // same as the section at 1:20" without naming either of them.
        let rate = SampleRate::HZ_44100;
        let (structure, _) = analyse(rate);
        let sections = structure.sections();

        let groups: Vec<usize> = sections.iter().map(Section::group).collect();
        let distinct = {
            let mut sorted = groups.clone();
            sorted.sort_unstable();
            sorted.dedup();
            sorted.len()
        };
        assert!(
            distinct < sections.len(),
            "every one of the {} sections was given its own group, so nothing was recognised as \
             repeating",
            sections.len()
        );
    }

    #[test]
    fn a_track_with_no_arrangement_reports_no_boundaries() {
        // The honest answer for material that genuinely has no sections. A
        // segmenter that always produces boundaries would place them at
        // arbitrary points here and the user would trust them.
        let rate = SampleRate::HZ_44100;
        let period = samples(0.5, rate);
        let bars = 32_usize;
        let mut signal = vec![0.0_f32; period * 4 * bars];
        for index in 0..(bars * 4) {
            write_click(&mut signal, index * period, 0xFEED + index as u64, rate);
        }

        let mut rhythm = Stft::for_rhythm().expect("valid");
        let curve = NoveltyCurve::compute(&mut rhythm, &signal, rate).expect("valid");
        let estimate = tempo::estimate(&curve).expect("has a tempo");
        let tracked = beats::track(&curve, &estimate).expect("has beats");

        let mut tonal = Stft::for_tone().expect("valid");
        let structure = detect(&mut tonal, &signal, rate, &tracked).expect("valid");

        assert!(
            structure.sections().len() <= 2,
            "an unvarying loop was split into {} sections",
            structure.sections().len()
        );
    }

    #[test]
    fn section_at_finds_the_containing_section() {
        let rate = SampleRate::HZ_44100;
        let (structure, _) = analyse(rate);
        let Some(section) = structure.sections().get(1) else {
            return;
        };
        let middle = section.start() + ((section.end() - section.start()) >> 1);
        let found = structure.section_at(middle).expect("a section contains it");
        assert_eq!(found.start(), section.start());
        assert!(structure.section_at(usize::MAX).is_none());
    }

    #[test]
    fn material_too_short_to_segment_is_refused() {
        let rate = SampleRate::HZ_44100;
        let period = samples(0.5, rate);
        let mut signal = vec![0.0_f32; period * 40];
        for index in 0..40 {
            write_click(&mut signal, index * period, 0xAB + index as u64, rate);
        }

        let mut rhythm = Stft::for_rhythm().expect("valid");
        let curve = NoveltyCurve::compute(&mut rhythm, &signal, rate).expect("valid");
        let estimate = tempo::estimate(&curve).expect("has a tempo");
        let tracked = beats::track(&curve, &estimate).expect("has beats");

        let mut tonal = Stft::for_tone().expect("valid");
        assert!(matches!(
            detect(&mut tonal, &signal, rate, &tracked),
            Err(AnalysisError::NotEnoughAudio { .. })
        ));
    }

    #[test]
    fn similarity_compares_shape_rather_than_level() {
        // Why cosine and not distance. The same passage played 6 dB louder is
        // the same passage.
        let mut quiet = [0.0_f32; FEATURE_LENGTH];
        quiet[0] = 0.4;
        quiet[4] = 0.3;
        quiet[12] = 0.2;
        let mut loud = quiet;
        for value in &mut loud {
            *value *= 2.0;
        }
        assert!((similarity(&quiet, &loud) - 1.0).abs() < 1e-12);

        let mut different = [0.0_f32; FEATURE_LENGTH];
        different[7] = 0.5;
        different[14] = 0.5;
        assert!(similarity(&quiet, &different) < 0.3);
    }

    #[test]
    fn bars_in_handles_degenerate_input() {
        assert_eq!(bars_in(44_100, 0.0, 4), 0);
        assert_eq!(bars_in(44_100, 22_050.0, 0), 0);
        assert_eq!(bars_in(88_200, 22_050.0, 4), 1);
    }
}
