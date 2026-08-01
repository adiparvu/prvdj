//! Tempo estimation.
//!
//! # The two questions
//!
//! Finding the *period* of a piece of dance music is close to solved: the
//! novelty curve of a four-to-the-floor record has an obvious periodicity, and
//! autocorrelation finds it. Finding the *octave* — whether that period is a
//! beat, half a beat or two beats — is not, and it is where every tempo
//! detector that has ever annoyed a DJ goes wrong. A drum and bass track at
//! 174 BPM and the same track called 87 BPM have identical novelty curves at
//! every lag that matters.
//!
//! Master Prompt #20 requires half-time and double-time detection, and
//! `prv-time` already provides exact halving and doubling, so the engine can
//! act on either. What this module owes the rest of the system is therefore not
//! a single number but an honest one: the tempo it believes, the alternatives at
//! other octaves, and a confidence that reflects how separable they actually
//! were.
//!
//! # How the period is found
//!
//! Autocorrelation of the enhanced novelty curve, then a comb score.
//!
//! Raw autocorrelation peaks at the beat period *and* at every multiple of it,
//! and on a track with a strong bar structure the peak at four beats is often
//! the tallest. The comb score fixes that by asking a different question of each
//! candidate lag: not "does the curve repeat at this distance" but "does the
//! curve have energy at this lag and at two, three and four times it". A lag
//! that is really the bar length scores well only at its own multiples; a lag
//! that is really the beat scores well at all of them, because a bar contains
//! beats. The beat therefore wins on evidence rather than on a rule about which
//! peak to prefer.
//!
//! # How the octave is chosen
//!
//! With a prior, stated as a prior.
//!
//! Nothing in the audio distinguishes 87 from 174: the choice is a convention
//! about how people count. So the module applies a preference for tempi near
//! 120 BPM, weighted log-normally, which is the standard model of how listeners
//! choose a tapping rate. That is a *prior*, not a measurement, and it is kept
//! visibly separate: [`TempoEstimate::alternatives`] lists the other octaves
//! with their own comb scores, so the interface can offer them and the user's
//! correction can be learned from (Master Prompt #5) rather than argued with.

use prv_time::{SampleRate, Tempo};

use crate::confidence::Confidence;
use crate::error::AnalysisError;
use crate::fft::{Complex, RealFft};
use crate::num::{count_to_f64, round_to_count};
use crate::onset::NoveltyCurve;

/// The slowest tempo considered, in beats per minute.
///
/// Below this the period exceeds a second and the search starts finding bar
/// lengths and phrase lengths instead. Tracks genuinely slower than this exist,
/// and they are found at their double, which the alternatives expose.
pub const MIN_BPM: f64 = 60.0;

/// The fastest tempo considered, in beats per minute.
///
/// Above this the period approaches the resolution of the novelty curve itself.
/// Genres that count faster — hardcore, some drum and bass conventions — are
/// found at half and offered as an alternative.
pub const MAX_BPM: f64 = 200.0;

/// The centre of the tapping preference, in beats per minute.
///
/// Around two beats a second, which is where listeners asked to tap along with
/// ambiguous music converge, and close to the resonant frequency of human
/// locomotion. It is a fact about people, not about music.
const PREFERRED_BPM: f64 = 120.0;

/// The width of the tapping preference, in octaves.
///
/// Wide enough that a 90 BPM hip-hop record and a 174 BPM drum and bass record
/// are both comfortably inside it, so the prior breaks genuine ties without
/// overruling clear evidence.
const PREFERENCE_WIDTH: f64 = 1.0;

/// How many harmonics the comb score sums.
///
/// Four: the beat, and the second, third and fourth multiples. Four covers a
/// bar in common time, which is where the reinforcing evidence lives. A fifth
/// would start crossing into the next bar and reward the wrong lag.
const COMB_HARMONICS: usize = 4;

/// The step of the fractional lag search, in novelty-curve frames.
///
/// A twentieth of a frame. At the rhythm settings one frame is 5.8 milliseconds,
/// so the search resolves the beat period to about a third of a millisecond —
/// well under a hundredth of a beat per minute at any usable tempo, and far
/// finer than the estimate's own uncertainty. The cost is a few thousand comb
/// evaluations, which is nothing beside the transform that produced the curve.
const LAG_STEP: f64 = 0.05;

/// The minimum audio duration for a tempo estimate, in seconds.
///
/// Ten seconds. Shorter than this and there are too few bars for a periodicity
/// to be distinguished from a coincidence. Reporting
/// [`AnalysisError::NotEnoughAudio`] rather than a low confidence keeps the
/// confidence scale meaningful: a low confidence should mean "the music is
/// ambiguous", not "I was not given enough of it".
pub const MINIMUM_SECONDS: f64 = 10.0;

/// A tempo candidate: a period with the evidence supporting it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TempoCandidate {
    tempo: Tempo,
    score: f32,
    relation: OctaveRelation,
}

impl TempoCandidate {
    /// The tempo.
    #[must_use]
    pub const fn tempo(&self) -> Tempo {
        self.tempo
    }

    /// The comb score, relative to the other candidates.
    ///
    /// Comparable within one estimate and meaningless across estimates, because
    /// it is not normalised against anything outside this track.
    #[must_use]
    pub const fn score(&self) -> f32 {
        self.score
    }

    /// How this candidate relates to the chosen tempo.
    #[must_use]
    pub const fn relation(&self) -> OctaveRelation {
        self.relation
    }
}

/// How an alternative relates to the reported tempo.
///
/// Named rather than left as a ratio because the name is what an explanation
/// shows the user: "this could also be read as half time" is a sentence, and
/// "0.5" is not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OctaveRelation {
    /// The reported tempo itself.
    Same,
    /// Half the reported tempo.
    Half,
    /// Twice the reported tempo.
    Double,
    /// Two thirds — the triplet reading, which appears in shuffled material.
    TwoThirds,
    /// Three halves — the other triplet reading.
    ThreeHalves,
}

impl OctaveRelation {
    /// The multiplier this relation applies to a tempo.
    #[must_use]
    pub const fn multiplier(self) -> f64 {
        match self {
            Self::Same => 1.0,
            Self::Half => 0.5,
            Self::Double => 2.0,
            Self::TwoThirds => 2.0 / 3.0,
            Self::ThreeHalves => 1.5,
        }
    }

    /// The relations considered, in the order they are searched.
    const ALL: [Self; 5] = [
        Self::Same,
        Self::Half,
        Self::Double,
        Self::TwoThirds,
        Self::ThreeHalves,
    ];
}

/// The result of tempo estimation.
#[derive(Debug, Clone)]
pub struct TempoEstimate {
    tempo: Tempo,
    confidence: Confidence,
    alternatives: Vec<TempoCandidate>,
    period_frames: f64,
}

impl TempoEstimate {
    /// The tempo the estimator believes.
    #[must_use]
    pub const fn tempo(&self) -> Tempo {
        self.tempo
    }

    /// How well determined the tempo was.
    ///
    /// Two things reduce it, and they are different failures. A weak comb score
    /// means the track has little periodicity at all — ambient, spoken word,
    /// rubato. A strong score with a close rival means the period is clear but
    /// the octave is not, which is the drum and bass case. Both produce a lower
    /// number, and [`TempoEstimate::alternatives`] is what tells them apart.
    #[must_use]
    pub const fn confidence(&self) -> Confidence {
        self.confidence
    }

    /// Other readings of the same audio, strongest first.
    ///
    /// Always includes the chosen tempo as [`OctaveRelation::Same`], so a
    /// caller offering the user a choice does not have to reassemble the list.
    #[must_use]
    pub fn alternatives(&self) -> &[TempoCandidate] {
        &self.alternatives
    }

    /// The beat period in sample frames, before rounding to a tempo.
    ///
    /// The beat tracker needs this rather than the rounded tempo: rounding to
    /// the nearest microsecond per beat moves a beat by a fraction of a sample,
    /// but over a six-minute track that fraction accumulates, and the tracker's
    /// job is to place beats where the audio has them.
    #[must_use]
    pub const fn period_frames(&self) -> f64 {
        self.period_frames
    }
}

/// Estimates the tempo of a novelty curve.
///
/// # Errors
///
/// Returns [`AnalysisError::NotEnoughAudio`] for material shorter than
/// [`MINIMUM_SECONDS`], and [`AnalysisError::NoPeriodicity`] when no lag in the
/// searched range has any support — silence, applause, unmeasured speech.
pub fn estimate(curve: &NoveltyCurve) -> Result<TempoEstimate, AnalysisError> {
    let frame_rate = curve.frame_rate();
    let minimum_frames = round_to_count(MINIMUM_SECONDS * frame_rate);
    if curve.len() < minimum_frames {
        return Err(AnalysisError::NotEnoughAudio {
            frames: curve.len() * curve.hop(),
            minimum: minimum_frames * curve.hop(),
        });
    }

    let enhanced = curve.enhanced();
    let correlation = smooth(&autocorrelation(&enhanced));

    // Lags are in novelty-curve frames. The slowest tempo gives the longest
    // lag, so the bounds cross over.
    let longest = round_to_count(frame_rate * 60.0 / MIN_BPM);
    let shortest = round_to_count(frame_rate * 60.0 / MAX_BPM).max(1);
    if longest <= shortest || longest >= correlation.len() {
        return Err(AnalysisError::NotEnoughAudio {
            frames: curve.len() * curve.hop(),
            minimum: minimum_frames * curve.hop(),
        });
    }

    // The search is over fractional lags. A track's period is a whole number
    // of analysis frames only by coincidence, and searching integers alone
    // would quantise the answer to the hop — at the rhythm settings, a fifth of
    // a beat per minute at 120, which is enough to drift a beat within a few
    // minutes. Just as importantly, the fractional search is what lets the comb
    // place its harmonics at the true multiples rather than at multiples of a
    // rounded value.
    let mut best_lag = 0.0_f64;
    let mut best_score = 0.0_f64;
    let steps = round_to_count((count_to_f64(longest - shortest)) / LAG_STEP);
    for step in 0..=steps {
        let lag = count_to_f64(shortest) + count_to_f64(step) * LAG_STEP;
        if lag > count_to_f64(longest) {
            break;
        }
        let score = comb_score(&correlation, lag);
        if score > best_score {
            best_score = score;
            best_lag = lag;
        }
    }

    if best_lag <= 0.0 || best_score <= 0.0 {
        return Err(AnalysisError::NoPeriodicity);
    }

    let period_frames = best_lag * count_to_f64(curve.hop());

    let candidates = octave_candidates(&correlation, best_lag, frame_rate)?;
    let Some((chosen, _)) = candidates.first().copied() else {
        return Err(AnalysisError::NoPeriodicity);
    };

    let confidence = octave_confidence(&candidates, best_score, &correlation);
    let chosen_period = period_frames / chosen.relation.multiplier();

    // The alternatives are relabelled against whichever candidate won, so that
    // "Half" always means half of the reported tempo rather than half of an
    // intermediate the caller never sees.
    let alternatives = candidates
        .iter()
        .map(|(candidate, _)| TempoCandidate {
            tempo: candidate.tempo,
            score: candidate.score,
            relation: relation_between(chosen.tempo, candidate.tempo),
        })
        .collect();

    Ok(TempoEstimate {
        tempo: chosen.tempo,
        confidence,
        alternatives,
        period_frames: chosen_period,
    })
}

/// Estimates the tempo of a mono signal, running the transform for the caller.
///
/// # Errors
///
/// Propagates every error from [`estimate`] and from the transform.
pub fn estimate_signal(
    stft: &mut crate::spectrum::Stft,
    samples: &[f32],
    sample_rate: SampleRate,
) -> Result<TempoEstimate, AnalysisError> {
    let curve = NoveltyCurve::compute(stft, samples, sample_rate)?;
    estimate(&curve)
}

/// The unbiased autocorrelation of a curve, normalised so that lag zero is one.
///
/// # Computed through the transform, not directly
///
/// The direct double loop costs the curve length times the largest lag. For a
/// ten-minute track that is about five thousand million multiply-and-adds — a
/// wait a user would notice on one track, and one that makes analysing an
/// imported library impossible. The Wiener-Khinchin theorem gives the same
/// answer from two transforms of the padded curve, a few tens of millions of
/// operations: the difference between seconds and minutes per track.
///
/// Padding to at least twice the length is what makes this the *linear*
/// autocorrelation rather than the circular one. Without it the end of the
/// curve would correlate with its beginning, and a track's outro would appear
/// to be evidence about its intro.
///
/// # Unbiased
///
/// Each lag is divided by the number of overlapping terms rather than by the
/// curve length. The biased form tapers toward zero at long lags, and that
/// taper is a property of the estimator rather than of the music: it
/// systematically favours fast tempi, which is exactly the error this module is
/// most at risk of making.
fn autocorrelation(curve: &[f32]) -> Vec<f64> {
    let length = curve.len();
    // Half the curve. Beyond that the overlap is too short for the unbiased
    // normalisation to be stable, and the values become dominated by whichever
    // few frames happen to align.
    let max_lag = length >> 1;
    if length < 2 {
        return vec![0.0; max_lag + 1];
    }

    let padded = length.saturating_mul(2).next_power_of_two();
    let Ok(mut fft) = RealFft::new(padded) else {
        return vec![0.0; max_lag + 1];
    };

    let mut signal = vec![0.0_f64; padded];
    for (slot, &value) in signal.iter_mut().zip(curve.iter()) {
        *slot = f64::from(value);
    }

    let bins = fft.bins();
    let mut spectrum = vec![Complex::ZERO; bins];
    if fft.forward(&signal, &mut spectrum).is_err() {
        return vec![0.0; max_lag + 1];
    }

    // The power spectrum, laid out over the full transform length. It is real
    // and symmetric, which is what lets the inverse be computed by the same
    // forward transform: the inverse transform of a real symmetric sequence is
    // that sequence's own forward transform, divided by the length.
    let mut power = vec![0.0_f64; padded];
    for (bin, value) in spectrum.iter().enumerate() {
        let magnitude = value.magnitude_squared();
        if let Some(slot) = power.get_mut(bin) {
            *slot = magnitude;
        }
        if bin > 0 && bin < bins.saturating_sub(1) {
            if let Some(slot) = power.get_mut(padded - bin) {
                *slot = magnitude;
            }
        }
    }

    let mut correlation = vec![Complex::ZERO; bins];
    if fft.forward(&power, &mut correlation).is_err() {
        return vec![0.0; max_lag + 1];
    }

    let scale = count_to_f64(padded);
    let mut result = Vec::with_capacity(max_lag + 1);
    for lag in 0..=max_lag {
        let overlap = length.saturating_sub(lag);
        let raw = correlation.get(lag).map_or(0.0, |value| value.re) / scale;
        result.push(if overlap == 0 {
            0.0
        } else {
            raw / count_to_f64(overlap)
        });
    }

    let Some(&zero) = result.first() else {
        return result;
    };
    if zero > 0.0 {
        for value in &mut result {
            *value /= zero;
        }
    }
    result
}

/// Blurs the correlation so that a fractional period is not lost between two
/// integer lags.
///
/// This is not cosmetic. Without it the estimator has a systematic failure. A
/// novelty curve is close to an impulse train, so its correlation is near zero
/// everywhere except at exact integer lags. A period of 40.6 frames therefore
/// splits its evidence between lag 40 and lag 41 and scores *lower* than twice
/// that period at lag 81, whose own harmonics happen to land closer to whole
/// numbers. The estimator then reports half time — confidently, because the
/// margin is real — and does so specifically for tracks whose tempo is not a
/// whole number of analysis frames, which is most of them.
///
/// A Gaussian of about one frame restores the width that quantisation removed.
/// It is also physically honest: onsets in real music are not aligned to the
/// millisecond, and the transform smears them by a fraction of a window anyway.
fn smooth(correlation: &[f64]) -> Vec<f64> {
    // Three frames either side of a one-frame Gaussian covers everything above
    // a thousandth of the peak.
    const RADIUS: usize = 3;
    const SIGMA: f64 = 1.0;

    let mut kernel = Vec::with_capacity(RADIUS * 2 + 1);
    let mut total = 0.0_f64;
    for offset in 0..=(RADIUS * 2) {
        let distance = count_to_f64(offset) - count_to_f64(RADIUS);
        let weight = (-0.5 * distance * distance / (SIGMA * SIGMA)).exp();
        kernel.push(weight);
        total += weight;
    }
    if total > 0.0 {
        for weight in &mut kernel {
            *weight /= total;
        }
    }

    let mut output = Vec::with_capacity(correlation.len());
    for index in 0..correlation.len() {
        let mut sum = 0.0_f64;
        for (offset, &weight) in kernel.iter().enumerate() {
            let value = (index + offset)
                .checked_sub(RADIUS)
                .and_then(|position| correlation.get(position))
                .copied()
                .unwrap_or(0.0);
            sum += weight * value;
        }
        output.push(sum);
    }
    output
}

/// Reads the correlation at a fractional lag, interpolating linearly.
fn correlation_at(correlation: &[f64], lag: f64) -> f64 {
    if !lag.is_finite() || lag < 0.0 {
        return 0.0;
    }
    let lower = round_to_count(lag.floor());
    let fraction = lag - lag.floor();
    let first = correlation.get(lower).copied().unwrap_or(0.0);
    let second = correlation.get(lower + 1).copied().unwrap_or(0.0);
    first.mul_add(1.0 - fraction, second * fraction)
}

/// Sums the correlation at a lag and its first few multiples.
///
/// Each harmonic is weighted down, because a match at four times the lag is
/// weaker evidence for that lag than a match at the lag itself — a track with a
/// strong bar structure would otherwise let the bar length borrow the beat's
/// score.
///
/// The lag is fractional and the harmonics are computed from it rather than
/// from a rounded value. Rounding first would put the fourth harmonic of a
/// 40.6-frame period at 164 instead of 162.4 — a frame and a half away, far
/// enough that the harmonic contributes nothing, and the score collapses for
/// exactly the periods that are not whole numbers.
fn comb_score(correlation: &[f64], lag: f64) -> f64 {
    if lag <= 0.0 || !lag.is_finite() {
        return 0.0;
    }
    let mut score = 0.0_f64;
    for harmonic in 1..=COMB_HARMONICS {
        let position = lag * count_to_f64(harmonic);
        if position >= count_to_f64(correlation.len()) {
            break;
        }
        score += correlation_at(correlation, position).max(0.0) / count_to_f64(harmonic);
    }
    score
}

/// Builds the octave candidates and orders them by preference-weighted score.
fn octave_candidates(
    correlation: &[f64],
    lag: f64,
    frame_rate: f64,
) -> Result<Vec<(TempoCandidate, f64)>, AnalysisError> {
    let mut candidates: Vec<(TempoCandidate, f64)> = Vec::new();

    for relation in OctaveRelation::ALL {
        // A relation multiplying the tempo divides the lag.
        let candidate_lag = lag / relation.multiplier();
        let bpm = frame_rate * 60.0 / candidate_lag;
        if !(MIN_BPM..=MAX_BPM).contains(&bpm) {
            continue;
        }
        let Ok(tempo) = Tempo::from_bpm(bpm) else {
            continue;
        };

        let score = comb_score(correlation, candidate_lag);
        if score <= 0.0 {
            continue;
        }

        let weighted = score * tapping_preference(bpm);
        candidates.push((
            TempoCandidate {
                tempo,
                score: crate::num::narrow(score),
                relation,
            },
            weighted,
        ));
    }

    if candidates.is_empty() {
        return Err(AnalysisError::NoPeriodicity);
    }

    // Sorted by weighted score, with the tempo as a tiebreak so the order is
    // total and therefore reproducible. ADR-0006 requires the same inputs to
    // produce the same output; a sort whose result depends on the comparison
    // order of equal elements would quietly break that.
    candidates.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(core::cmp::Ordering::Equal)
            .then_with(|| {
                a.0.tempo
                    .micros_per_beat()
                    .cmp(&b.0.tempo.micros_per_beat())
            })
    });

    Ok(candidates)
}

/// The log-normal tapping preference, peaking at [`PREFERRED_BPM`].
fn tapping_preference(bpm: f64) -> f64 {
    if bpm <= 0.0 {
        return 0.0;
    }
    let octaves = (bpm / PREFERRED_BPM).log2() / PREFERENCE_WIDTH;
    (-0.5 * octaves * octaves).exp()
}

/// Derives a confidence from how clearly the winner beat its rivals.
///
/// Two independent doubts, combined by [`Confidence::and_then`] so that either
/// one alone is enough to lower the label.
///
/// # The octave margin is a posterior, and says so
///
/// The separation is measured on the preference-weighted scores rather than on
/// the raw comb scores, and that choice needs stating plainly because it looks
/// like the opposite of what ADR-0006 asks for.
///
/// A signal that repeats exactly every beat also repeats exactly every two
/// beats. Its correlation at the beat and at twice the beat are therefore
/// equal, and no amount of better signal processing changes that — the octave
/// is not underdetermined by *this* method, it is underdetermined by the
/// audio. Measuring the margin on the raw scores would report near-zero
/// confidence for every cleanly produced dance record, which is not honesty; it
/// is a number that carries no information because it is always the same.
///
/// What *is* determined is the answer the system gives, and that answer comes
/// from the evidence together with a stated prior about how listeners count. So
/// the confidence describes how firmly that combination settled the question,
/// and [`TempoEstimate::alternatives`] carries the ambiguity itself — with the
/// raw, unweighted evidence attached to each reading, so nothing is hidden.
fn octave_confidence(
    candidates: &[(TempoCandidate, f64)],
    best_score: f64,
    correlation: &[f64],
) -> Confidence {
    // Doubt one: is there periodicity at all? The comb score of the winner is
    // compared against the mean of the whole correlation, which for a track
    // with no beat is roughly the same number.
    let mean: f64 = if correlation.is_empty() {
        0.0
    } else {
        correlation.iter().map(|value| value.max(0.0)).sum::<f64>()
            / count_to_f64(correlation.len())
    };
    let strength = if best_score <= 0.0 {
        0.0
    } else {
        // A comb score of four times the background is treated as conclusive
        // evidence that something is periodic. The scale is provisional; the
        // ordering it induces is not.
        ((best_score / (mean * count_to_f64(COMB_HARMONICS)).max(f64::MIN_POSITIVE)) / 4.0)
            .clamp(0.0, 1.0)
    };

    // Doubt two: is the octave settled? The winner's margin over the runner-up.
    let separation = match (candidates.first(), candidates.get(1)) {
        (Some(&(_, top)), Some(&(_, next))) => {
            if top <= 0.0 {
                0.0
            } else {
                ((top - next) / top).clamp(0.0, 1.0)
            }
        }
        // No rival at all: the octave question did not arise.
        (Some(_), None) => 1.0,
        _ => 0.0,
    };

    // A margin of a quarter is already decisive; the mapping saturates there so
    // that ordinary music is not permanently labelled uncertain because a
    // half-time reading also has some support. It always does.
    let octave = (separation * 4.0).clamp(0.0, 1.0);

    Confidence::from_f64(strength).and_then(Confidence::from_f64(octave))
}

/// Classifies how one tempo relates to another.
fn relation_between(reference: Tempo, other: Tempo) -> OctaveRelation {
    let reference_bpm = reference.bpm();
    let other_bpm = other.bpm();
    if reference_bpm <= 0.0 {
        return OctaveRelation::Same;
    }
    let observed = other_bpm / reference_bpm;

    let mut best = OctaveRelation::Same;
    let mut best_error = f64::INFINITY;
    for relation in OctaveRelation::ALL {
        let error = (observed - relation.multiplier()).abs();
        if error < best_error {
            best_error = error;
            best = relation;
        }
    }
    best
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
    use crate::spectrum::Stft;
    use crate::testing::{click_track, samples, tone};

    fn estimate_click_track(bpm: f64, beats: usize) -> TempoEstimate {
        let rate = SampleRate::HZ_44100;
        let period = round_to_count(f64::from(rate.hz()) * 60.0 / bpm);
        let signal = click_track(period, beats, rate);
        let mut stft = Stft::for_rhythm().expect("valid");
        estimate_signal(&mut stft, &signal, rate).expect("a click track has a tempo")
    }

    #[test]
    fn a_click_track_is_measured_to_within_half_a_beat_per_minute() {
        // This module produces the *coarse* tempo: good enough to seed the beat
        // tracker's search and to choose the octave, and not claimed to be more
        // than that. The precise figure comes from fitting a line through every
        // tracked beat, which `beats` does and tests to a tenth of this
        // tolerance. Asserting that accuracy here would be asserting it of the
        // wrong stage.
        for bpm in [90.0, 120.0, 128.0, 140.0] {
            let estimate = estimate_click_track(bpm, 64);
            let measured = estimate.tempo().bpm();
            assert!(
                (measured - bpm).abs() < 0.5,
                "expected {bpm} BPM, measured {measured}"
            );
            assert!(
                estimate.confidence().is_actionable(),
                "a click track should be measured with actionable confidence, got {}",
                estimate.confidence()
            );
        }
    }

    #[test]
    fn a_tempo_that_is_not_a_whole_number_of_frames_is_not_read_as_half_time() {
        // The failure the correlation smoothing exists to prevent, and the one
        // that would have shipped: 127.3 BPM is 114.8 analysis frames, so its
        // evidence splits between lags 114 and 115 while the *doubled* period
        // lands closer to whole numbers and scores higher. The estimator would
        // then report 63.6 BPM — confidently, because the margin is real — for
        // every track whose tempo is not a whole number of frames, which is
        // almost all of them.
        let rate = SampleRate::HZ_44100;
        for bpm in [127.3, 121.7, 133.9, 96.4] {
            let period = round_to_count(f64::from(rate.hz()) * 60.0 / bpm);
            let signal = click_track(period, 96, rate);
            let mut stft = Stft::for_rhythm().expect("valid");
            let estimate = estimate_signal(&mut stft, &signal, rate).expect("has a tempo");
            let measured = estimate.tempo().bpm();
            assert!(
                (measured - bpm).abs() < 0.5,
                "expected {bpm} BPM, measured {measured} — an octave error, not a rounding one"
            );
        }
    }

    #[test]
    fn the_half_time_reading_is_offered_rather_than_hidden() {
        // Master Prompt #20 requires half and double time to be available. This
        // is the behaviour a DJ notices: the estimator commits to one reading
        // but does not pretend the other does not exist.
        let estimate = estimate_click_track(128.0, 64);
        let relations: Vec<OctaveRelation> = estimate
            .alternatives()
            .iter()
            .map(TempoCandidate::relation)
            .collect();

        assert!(
            relations.contains(&OctaveRelation::Same),
            "the chosen tempo must appear in its own alternatives"
        );
        assert!(
            relations.contains(&OctaveRelation::Half)
                || relations.contains(&OctaveRelation::Double),
            "no octave alternative was offered: {relations:?}"
        );
    }

    #[test]
    fn a_fast_track_is_reported_in_the_range_a_dj_expects() {
        // A 174 BPM pattern. The tapping preference sits at 120, so this is the
        // case where the prior and the evidence pull in different directions.
        // What must not happen is silently reporting 87 with high confidence.
        let estimate = estimate_click_track(174.0, 128);
        let measured = estimate.tempo().bpm();
        let plausible = (measured - 174.0).abs() < 1.0 || (measured - 87.0).abs() < 1.0;
        assert!(plausible, "measured {measured}, neither 174 nor 87");

        if (measured - 87.0).abs() < 1.0 {
            // If the prior won, the true reading must be on offer.
            let doubled: Vec<f64> = estimate
                .alternatives()
                .iter()
                .map(|candidate| candidate.tempo().bpm())
                .collect();
            assert!(
                doubled.iter().any(|bpm| (bpm - 174.0).abs() < 1.0),
                "reported half time without offering the double: {doubled:?}"
            );
        }
    }

    #[test]
    fn silence_reports_no_periodicity_rather_than_a_number() {
        let rate = SampleRate::HZ_44100;
        let signal = vec![0.0_f32; samples(20.0, rate)];
        let mut stft = Stft::for_rhythm().expect("valid");
        assert_eq!(
            estimate_signal(&mut stft, &signal, rate).err(),
            Some(AnalysisError::NoPeriodicity)
        );
    }

    #[test]
    fn a_steady_tone_is_not_given_an_actionable_tempo() {
        // A sustained tone has no beat. The estimator may still find its
        // strongest lag, but it must not claim to be sure about it, because a
        // confident wrong tempo is what makes a user stop trusting the label.
        let rate = SampleRate::HZ_44100;
        let signal = tone(220.0, samples(20.0, rate), 0.5, rate);
        let mut stft = Stft::for_rhythm().expect("valid");

        match estimate_signal(&mut stft, &signal, rate) {
            Err(AnalysisError::NoPeriodicity) => {}
            Ok(estimate) => assert!(
                !estimate.confidence().is_actionable(),
                "a steady tone was given tempo {} with confidence {}",
                estimate.tempo().bpm(),
                estimate.confidence()
            ),
            Err(other) => panic!("unexpected error {other:?}"),
        }
    }

    #[test]
    fn material_shorter_than_the_minimum_is_refused_rather_than_guessed() {
        let rate = SampleRate::HZ_44100;
        let period = round_to_count(f64::from(rate.hz()) * 0.5);
        let signal = click_track(period, 8, rate);
        let mut stft = Stft::for_rhythm().expect("valid");
        assert!(matches!(
            estimate_signal(&mut stft, &signal, rate),
            Err(AnalysisError::NotEnoughAudio { .. })
        ));
    }

    #[test]
    fn estimation_is_reproducible() {
        // ADR-0006 requires it, and a sort with a non-total comparator is the
        // usual way this quietly stops being true.
        let first = estimate_click_track(124.0, 64);
        let second = estimate_click_track(124.0, 64);
        assert_eq!(first.tempo(), second.tempo());
        assert_eq!(first.confidence(), second.confidence());
        assert_eq!(
            first.alternatives().len(),
            second.alternatives().len(),
            "the alternative list differed between identical runs"
        );
    }

    #[test]
    fn the_tapping_preference_peaks_where_it_claims_to() {
        assert!((tapping_preference(PREFERRED_BPM) - 1.0).abs() < 1e-12);
        assert!(tapping_preference(60.0) < 1.0);
        assert!(tapping_preference(240.0) < 1.0);
        // Symmetric in octaves, which is the point of using a log scale: half
        // time and double time are penalised equally, so the prior does not
        // secretly favour one direction.
        assert!((tapping_preference(60.0) - tapping_preference(240.0)).abs() < 1e-12);
        assert!((tapping_preference(0.0) - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn relation_between_names_the_ratios_correctly() {
        let base = Tempo::from_bpm(128.0).expect("valid");
        let half = Tempo::from_bpm(64.0).expect("valid");
        let double = Tempo::from_bpm(180.0).expect("valid");
        assert_eq!(relation_between(base, base), OctaveRelation::Same);
        assert_eq!(relation_between(base, half), OctaveRelation::Half);
        assert_eq!(relation_between(base, double), OctaveRelation::ThreeHalves);
    }
}
