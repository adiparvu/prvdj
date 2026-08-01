//! Beat and downbeat tracking.
//!
//! # Why a tempo is not a beat grid
//!
//! Knowing a track is 128 BPM says nothing about where the beats are. Placing
//! them by starting at the first onset and stepping by the period fails on real
//! music for two reasons: the first onset is often not a beat — an intro sweep,
//! a vinyl crackle, a vocal breath — and no recording holds a tempo perfectly,
//! so an error of one part in a thousand puts the grid half a beat out by the
//! end of a six-minute track.
//!
//! Master Prompt #18 requires sample-accurate transport, and Module
//! Specification #003 shows the grid drawn over the waveform. Both mean a user
//! sees the grid against the audio, and a grid that drifts is the most visible
//! defect this crate can produce.
//!
//! # The approach
//!
//! Dynamic programming over the whole track, after Ellis. Every frame gets a
//! score: the onset strength there, plus the best score achievable by any
//! earlier beat, minus a penalty for how far that spacing departs from the
//! estimated period. Then the best path is traced back.
//!
//! What this buys over stepping by the period is that it is *global*. A greedy
//! tracker that picks each next beat from the local neighbourhood commits to
//! early mistakes and cannot recover; the drum fill at 2:30 takes it off the
//! grid for the rest of the track. The dynamic programme considers every
//! placement of every beat at once, so a passage with no clear onsets — a
//! breakdown, an ambient bridge — costs only the penalty for coasting through
//! it at the expected spacing, and the beats after it are placed correctly
//! because the beats before it were.
//!
//! The penalty is squared in the logarithm of the ratio, so it is symmetric:
//! spacing ten per cent long is penalised exactly as much as ten per cent
//! short. A penalty on the raw difference would be lopsided and would bias the
//! whole grid slightly fast.
//!
//! # Downbeats
//!
//! Which beat begins the bar is a different question from where the beats are,
//! and it has a different answer in the audio. Beats are carried by everything;
//! downbeats are carried by the low end, because the kick and the bass note
//! that lands with it are what a listener uses. So the downbeat phase is chosen
//! by low-band energy accumulated over the beats at each phase, not by onset
//! strength — which is roughly equal on all four and would decide the question
//! by noise.

use prv_time::{BeatGrid, Frames, SampleRate, Tempo, TimeSignature};

use crate::confidence::Confidence;
use crate::error::AnalysisError;
use crate::num::{count_to_f64, narrow, round_to_count, signed_to_f64};
use crate::onset::NoveltyCurve;
use crate::tempo::TempoEstimate;

/// How strongly the tracker prefers the estimated spacing.
///
/// Ellis's value, on a novelty curve normalised to unit standard deviation. Low
/// enough that a genuinely syncopated placement can win when the audio insists;
/// high enough that the grid coasts straight through a breakdown rather than
/// latching onto whatever pad swell happens to be there.
const TIGHTNESS: f64 = 100.0;

/// The narrowest spacing the tracker will consider, as a fraction of the period.
const MIN_SPACING: f64 = 0.5;

/// The widest spacing the tracker will consider, as a fraction of the period.
const MAX_SPACING: f64 = 2.0;

/// The default number of beats in a bar when no signature is supplied.
///
/// Four. Not because other signatures do not exist, but because assuming four
/// and being wrong is recoverable — the user changes it and the grid re-lays —
/// whereas inferring a signature from audio is a research problem whose failure
/// mode is a grid that is wrong in a way the user cannot correct by hand.
/// Master Prompt #20 does not require signature detection; pretending to do it
/// badly would be worse than not doing it.
const DEFAULT_BEATS_PER_BAR: usize = 4;

/// The fraction of a beat over which low-band energy is measured for downbeat
/// detection.
///
/// Half a beat. Long enough to contain a kick's whole decay at any tempo a DJ
/// plays, short enough that it does not run into the next beat and average the
/// distinction away.
const DOWNBEAT_ENERGY_SPAN: f64 = 0.5;

/// One tracked beat.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Beat {
    sample: usize,
    strength: f32,
    bar_position: u8,
}

impl Beat {
    /// The sample offset of the beat.
    #[must_use]
    pub const fn sample(&self) -> usize {
        self.sample
    }

    /// The onset strength at the beat.
    ///
    /// Comparable within a track and not across tracks. Useful for showing
    /// which beats the tracker was confident about and which it coasted
    /// through, which is the honest way to draw a grid over a breakdown.
    #[must_use]
    pub const fn strength(&self) -> f32 {
        self.strength
    }

    /// The beat's position within its bar, counting from zero.
    #[must_use]
    pub const fn bar_position(&self) -> u8 {
        self.bar_position
    }

    /// Whether this beat begins a bar.
    #[must_use]
    pub const fn is_downbeat(&self) -> bool {
        self.bar_position == 0
    }
}

/// The result of beat tracking.
#[derive(Debug, Clone)]
pub struct BeatEstimate {
    beats: Vec<Beat>,
    beats_per_bar: usize,
    sample_rate: SampleRate,
    tempo: Tempo,
    origin: Frames,
    /// The fitted position of the first tracked beat, which is not necessarily
    /// a downbeat. Kept because measuring deviation against `origin` would
    /// measure the bar offset rather than the timing.
    first_beat: f64,
    period_frames: f64,
    beat_confidence: Confidence,
    downbeat_confidence: Confidence,
}

impl BeatEstimate {
    /// Every tracked beat, in order.
    #[must_use]
    pub fn beats(&self) -> &[Beat] {
        &self.beats
    }

    /// The beats that begin a bar.
    pub fn downbeats(&self) -> impl Iterator<Item = &Beat> {
        self.beats.iter().filter(|beat| beat.is_downbeat())
    }

    /// The number of beats in a bar the grid was laid out with.
    #[must_use]
    pub const fn beats_per_bar(&self) -> usize {
        self.beats_per_bar
    }

    /// The tempo, refined from the tracked beats.
    ///
    /// Not the tempo the tracker was given. The estimate from [`crate::tempo`]
    /// is resolved to a fraction of an analysis frame; this one is fitted to
    /// every beat in the track at once, so its error falls with the number of
    /// beats rather than staying at the resolution of the curve. Over a
    /// six-minute record that is the difference between a grid that is right at
    /// the start and a grid that is right at the end as well.
    #[must_use]
    pub const fn tempo(&self) -> Tempo {
        self.tempo
    }

    /// The beat period in sample frames, before rounding to a tempo.
    #[must_use]
    pub const fn period_frames(&self) -> f64 {
        self.period_frames
    }

    /// How well determined the beat positions were.
    #[must_use]
    pub const fn beat_confidence(&self) -> Confidence {
        self.beat_confidence
    }

    /// How well determined the downbeat phase was.
    ///
    /// Reported separately from [`BeatEstimate::beat_confidence`] because the
    /// two fail independently and a user acts on them differently. A grid whose
    /// beats are right and whose downbeat is wrong is one keystroke from
    /// correct; the interface can say so rather than casting doubt on the whole
    /// analysis.
    #[must_use]
    pub const fn downbeat_confidence(&self) -> Confidence {
        self.downbeat_confidence
    }

    /// The first downbeat, taken from the fitted grid rather than from a single
    /// tracked beat.
    ///
    /// Using one beat's measured position would inherit that beat's own
    /// quantisation error and hand it to the whole grid. The fitted intercept
    /// averages every beat in the track, so the anchor is as well determined as
    /// the tempo is.
    #[must_use]
    pub const fn first_downbeat(&self) -> Frames {
        self.origin
    }

    /// Builds a [`BeatGrid`] anchored at the first detected downbeat.
    ///
    /// This is where analysis hands over to the exact musical time of
    /// `prv-time`. The grid it returns uses integer arithmetic from that point
    /// on, so nothing downstream inherits the floating-point period this module
    /// worked in. Analysis is allowed to be approximate; the transport is not.
    ///
    /// The grid carries a single tempo rather than a map fitted to the tracked
    /// beats. That is deliberate for a first version: a per-beat tempo map
    /// would track the audio more closely, but it also encodes every tracking
    /// error as a tempo change the user then has to edit. A constant tempo with
    /// an accurate origin is what a DJ can reason about, and the tempo map
    /// exists in `prv-time` for when a later stage can populate it from
    /// something better than beat-to-beat spacing.
    #[must_use]
    pub fn to_beat_grid(&self, signature: TimeSignature) -> BeatGrid {
        BeatGrid::new(self.sample_rate, self.tempo, signature, self.origin)
    }

    /// The mean absolute deviation between the tracked beats and a perfectly
    /// even grid at the estimated tempo, in samples.
    ///
    /// Exposed because it is the number that says whether a constant-tempo grid
    /// is a good description of this track. A live recording or a hand-played
    /// record will show a large deviation, and that is a fact worth surfacing
    /// rather than hiding behind a confidence.
    #[must_use]
    pub fn deviation_from_constant_tempo(&self) -> f64 {
        if self.beats.len() < 2 {
            return 0.0;
        }
        // Frames per beat from the exact integer tempo, so that the reference
        // grid this is measured against is the one `prv-time` would lay out and
        // not a second, slightly different, floating-point one.
        let period = f64::from(self.sample_rate.hz())
            * crate::num::micros_to_seconds(self.tempo.micros_per_beat());
        let mut total = 0.0_f64;
        for (index, beat) in self.beats.iter().enumerate() {
            let expected = self.first_beat + count_to_f64(index) * period;
            total += (count_to_f64(beat.sample()) - expected).abs();
        }
        total / count_to_f64(self.beats.len())
    }
}

/// Tracks beats through a novelty curve, given a tempo estimate.
///
/// # Errors
///
/// Returns [`AnalysisError::NoPeriodicity`] when the curve is too short to hold
/// two beats at the estimated period, which means the tempo estimate and the
/// audio disagree about what was analysed.
pub fn track(curve: &NoveltyCurve, tempo: &TempoEstimate) -> Result<BeatEstimate, AnalysisError> {
    track_with_signature(curve, tempo, DEFAULT_BEATS_PER_BAR)
}

/// Tracks beats with an explicit number of beats per bar.
///
/// # Errors
///
/// As [`track`]. A `beats_per_bar` of zero is treated as the default rather
/// than rejected, because it is a caller mistake that has an obvious harmless
/// reading and no musical meaning.
pub fn track_with_signature(
    curve: &NoveltyCurve,
    tempo: &TempoEstimate,
    beats_per_bar: usize,
) -> Result<BeatEstimate, AnalysisError> {
    let beats_per_bar = if beats_per_bar == 0 {
        DEFAULT_BEATS_PER_BAR
    } else {
        beats_per_bar
    };

    let hop = count_to_f64(curve.hop());
    if hop <= 0.0 {
        return Err(AnalysisError::NoPeriodicity);
    }
    let period_in_curve = tempo.period_frames() / hop;
    if period_in_curve < 1.0 || curve.len() < round_to_count(period_in_curve * 2.0) {
        return Err(AnalysisError::NoPeriodicity);
    }

    let enhanced = normalise(&curve.enhanced());
    let path = best_path(&enhanced, period_in_curve);
    if path.is_empty() {
        return Err(AnalysisError::NoPeriodicity);
    }

    let (phase, downbeat_confidence) = downbeat_phase(&path, curve, beats_per_bar, period_in_curve);

    let beats: Vec<Beat> = path
        .iter()
        .enumerate()
        .map(|(index, &frame)| {
            let strength = enhanced.get(frame).copied().unwrap_or(0.0);
            let position = (index + beats_per_bar - phase) % beats_per_bar;
            Beat {
                sample: curve.sample_at(frame),
                strength: narrow(strength),
                bar_position: u8::try_from(position).unwrap_or(0),
            }
        })
        .collect();

    let beat_confidence = beat_confidence(&beats, &enhanced, period_in_curve, curve);

    // The grid is fitted to every tracked beat at once rather than taken from
    // the tempo estimate and the first beat. Both of those carry the novelty
    // curve's own resolution; a least-squares fit over hundreds of beats does
    // not, because independent quantisation errors average away. This is where
    // the analysis becomes accurate enough to hand to `prv-time`.
    let fit = fit_grid(&beats, beats_per_bar).unwrap_or(GridFit {
        origin: count_to_f64(beats.first().map_or(0, Beat::sample)),
        first_beat: count_to_f64(beats.first().map_or(0, Beat::sample)),
        period: tempo.period_frames(),
    });

    let refined = Tempo::from_bpm(bpm_from_period(fit.period, curve.sample_rate()))
        .unwrap_or_else(|_| tempo.tempo());

    Ok(BeatEstimate {
        beats,
        beats_per_bar,
        sample_rate: curve.sample_rate(),
        tempo: refined,
        origin: Frames::new(i64::try_from(round_to_count(fit.origin)).unwrap_or(0)),
        first_beat: fit.first_beat,
        period_frames: fit.period,
        beat_confidence: tempo.confidence().and_then(beat_confidence),
        downbeat_confidence: tempo.confidence().and_then(downbeat_confidence),
    })
}

/// A constant-tempo grid fitted to a set of tracked beats.
#[derive(Debug, Clone, Copy)]
struct GridFit {
    /// The sample position of the first downbeat.
    origin: f64,
    /// The fitted position of the first tracked beat.
    first_beat: f64,
    /// The beat period in sample frames.
    period: f64,
}

/// Converts a beat period in frames to beats per minute.
fn bpm_from_period(period_frames: f64, sample_rate: SampleRate) -> f64 {
    if period_frames <= 0.0 {
        return 0.0;
    }
    f64::from(sample_rate.hz()) * 60.0 / period_frames
}

/// Fits a straight line through the tracked beat positions.
///
/// Ordinary least squares of position against beat index. The slope is the
/// period and the intercept, taken at the first downbeat, is the grid origin.
///
/// This is the right estimator for the right reason. Each tracked beat carries
/// an independent error of up to half a hop from the resolution of the novelty
/// curve; a fit over `n` beats reduces the error in the slope roughly as `n`
/// to the power of three halves, so a track with three hundred beats gets a
/// period accurate to a fraction of a sample. Taking the period from two beats,
/// or from the tempo estimate alone, keeps the full per-beat error and hands it
/// to a grid that then drifts across the track.
///
/// Returns `None` when there are too few beats for a slope to mean anything.
fn fit_grid(beats: &[Beat], beats_per_bar: usize) -> Option<GridFit> {
    if beats.len() < 2 {
        return None;
    }

    let count = count_to_f64(beats.len());
    let mut index_sum = 0.0_f64;
    let mut position_sum = 0.0_f64;
    for (index, beat) in beats.iter().enumerate() {
        index_sum += count_to_f64(index);
        position_sum += count_to_f64(beat.sample());
    }
    let index_mean = index_sum / count;
    let position_mean = position_sum / count;

    let mut covariance = 0.0_f64;
    let mut variance = 0.0_f64;
    for (index, beat) in beats.iter().enumerate() {
        let centred_index = count_to_f64(index) - index_mean;
        covariance += centred_index * (count_to_f64(beat.sample()) - position_mean);
        variance += centred_index * centred_index;
    }
    if variance <= 0.0 {
        return None;
    }

    let period = covariance / variance;
    if !period.is_finite() || period <= 0.0 {
        return None;
    }
    let intercept = period.mul_add(-index_mean, position_mean);

    // The origin is the fitted position of the first beat that begins a bar,
    // not of the first beat. A grid anchored on beat three of a bar would show
    // every bar line in the wrong place while every beat line was right, which
    // is a more confusing failure than either alone.
    let first_downbeat_index = beats
        .iter()
        .position(Beat::is_downbeat)
        .unwrap_or(0)
        .min(beats_per_bar.saturating_sub(1).max(0));
    let origin = period.mul_add(count_to_f64(first_downbeat_index), intercept);

    Some(GridFit {
        origin: origin.max(0.0),
        first_beat: intercept,
        period,
    })
}

/// Scales a curve to unit standard deviation.
///
/// The tightness constant is only meaningful against a curve of known scale.
/// Without this the penalty would dominate on a quiet track and be ignored on a
/// loud one, so the tracker's behaviour would depend on mastering level — which
/// is exactly the dependency the logarithmic compression in the novelty curve
/// was there to remove.
fn normalise(values: &[f32]) -> Vec<f64> {
    if values.is_empty() {
        return Vec::new();
    }
    let count = count_to_f64(values.len());
    let mean = values.iter().map(|&v| f64::from(v)).sum::<f64>() / count;
    let variance = values
        .iter()
        .map(|&v| {
            let centred = f64::from(v) - mean;
            centred * centred
        })
        .sum::<f64>()
        / count;
    let deviation = variance.sqrt();
    if deviation <= f64::MIN_POSITIVE {
        return vec![0.0; values.len()];
    }
    values.iter().map(|&v| f64::from(v) / deviation).collect()
}

/// The dynamic programme.
///
/// Returns the frame indices of the beats on the best-scoring path.
fn best_path(onset: &[f64], period: f64) -> Vec<usize> {
    let length = onset.len();
    let mut score = vec![f64::NEG_INFINITY; length];
    let mut previous = vec![usize::MAX; length];

    let earliest = round_to_count(period * MIN_SPACING).max(1);
    let latest = round_to_count(period * MAX_SPACING).max(earliest + 1);

    // The transition penalty depends only on the spacing, so it is tabulated
    // once. Recomputing a logarithm inside the inner loop would make the
    // tracker's cost a logarithm per frame per candidate spacing — for a
    // ten-minute track, tens of millions of them.
    let mut penalty = Vec::with_capacity(latest - earliest + 1);
    for spacing in earliest..=latest {
        let ratio = count_to_f64(spacing) / period;
        let deviation = ratio.ln();
        penalty.push(-TIGHTNESS * deviation * deviation);
    }

    for frame in 0..length {
        let local = onset.get(frame).copied().unwrap_or(0.0);

        // A beat with no predecessor starts a path. Its score is its own onset
        // strength; nothing is charged for the run-in, so the tracker is free
        // to begin wherever the music does rather than at sample zero.
        let mut best = local;
        let mut best_previous = usize::MAX;

        if frame >= earliest {
            let highest = frame - earliest;
            let lowest = frame.saturating_sub(latest);
            for candidate in lowest..=highest {
                let Some(&candidate_score) = score.get(candidate) else {
                    continue;
                };
                if candidate_score == f64::NEG_INFINITY {
                    continue;
                }
                let spacing = frame - candidate;
                let Some(&transition) = penalty.get(spacing.saturating_sub(earliest)) else {
                    continue;
                };
                let total = candidate_score + transition + local;
                if total > best {
                    best = total;
                    best_previous = candidate;
                }
            }
        }

        if let Some(slot) = score.get_mut(frame) {
            *slot = best;
        }
        if let Some(slot) = previous.get_mut(frame) {
            *slot = best_previous;
        }
    }

    // The path ends at the best score in the final stretch. Searching only the
    // last period and a half avoids ending on an accidental late peak — a fade
    // tail, a crowd noise — while still allowing the last real beat to win.
    let tail_start = length.saturating_sub(round_to_count(period * 1.5).max(1));
    let mut end = None;
    let mut best_end = f64::NEG_INFINITY;
    for frame in tail_start..length {
        let Some(&value) = score.get(frame) else {
            continue;
        };
        if value > best_end {
            best_end = value;
            end = Some(frame);
        }
    }

    let Some(mut cursor) = end else {
        return Vec::new();
    };

    let mut path = Vec::new();
    loop {
        path.push(cursor);
        let Some(&step) = previous.get(cursor) else {
            break;
        };
        if step == usize::MAX || step >= cursor {
            break;
        }
        cursor = step;
    }
    path.reverse();
    path
}

/// Chooses which beat of the bar the track starts on.
///
/// Returns the phase and a confidence derived from how far the winning phase
/// stood out. A four-to-the-floor record with a bassline that changes on the
/// downbeat gives a clear answer; a track with an unvarying kick on every beat
/// genuinely does not have one from the low band alone, and the low confidence
/// says so rather than a coin toss being presented as a finding.
fn downbeat_phase(
    path: &[usize],
    curve: &NoveltyCurve,
    beats_per_bar: usize,
    period_in_curve: f64,
) -> (usize, Confidence) {
    if path.is_empty() || beats_per_bar == 0 {
        return (0, Confidence::NONE);
    }

    let low_band = curve.low_band();
    let mut totals = vec![0.0_f64; beats_per_bar];
    let mut counts = vec![0_usize; beats_per_bar];

    // The energy is taken over the first part of each beat rather than at the
    // single frame the beat sits on. A kick is not an instant: it is a hundred
    // milliseconds of low end, and reading one frame would sample it at
    // whatever point the tracker happened to place the beat. Onsets are also
    // detected slightly early, by up to half an analysis window, so a
    // single-frame reading of a downbeat can land just *before* the kick and
    // report the quietest moment of the bar as its loudest.
    let span = round_to_count(period_in_curve * DOWNBEAT_ENERGY_SPAN).max(1);

    for (index, &frame) in path.iter().enumerate() {
        let phase = index % beats_per_bar;
        let mut energy = 0.0_f64;
        for offset in 0..span {
            energy += f64::from(low_band.get(frame + offset).copied().unwrap_or(0.0));
        }
        let energy = energy / count_to_f64(span);
        if let Some(slot) = totals.get_mut(phase) {
            *slot += energy;
        }
        if let Some(slot) = counts.get_mut(phase) {
            *slot += 1;
        }
    }

    let means: Vec<f64> = totals
        .iter()
        .zip(counts.iter())
        .map(|(&total, &count)| {
            if count == 0 {
                0.0
            } else {
                total / count_to_f64(count)
            }
        })
        .collect();

    let mut best_phase = 0_usize;
    let mut best = f64::NEG_INFINITY;
    let mut second = f64::NEG_INFINITY;
    for (phase, &value) in means.iter().enumerate() {
        if value > best {
            second = best;
            best = value;
            best_phase = phase;
        } else if value > second {
            second = value;
        }
    }

    if best <= 0.0 {
        return (0, Confidence::NONE);
    }
    let margin = if second <= f64::NEG_INFINITY {
        1.0
    } else {
        ((best - second) / best).clamp(0.0, 1.0)
    };
    // A tenth of the low-band energy separating first from second is a clear
    // result; the mapping saturates there. Below it the confidence falls away
    // quickly, because a downbeat chosen on a two per cent margin is a guess.
    let confidence = Confidence::from_f64((margin * 10.0).clamp(0.0, 1.0));

    (best_phase, confidence)
}

/// Derives a confidence for the beat positions.
///
/// Two things are asked. Did the beats land on actual onsets, or did the
/// tracker coast? And was the spacing consistent, or did the path stretch and
/// compress to reach them? A grid can fail either way, and the weaker answer
/// governs.
fn beat_confidence(beats: &[Beat], onset: &[f64], period: f64, curve: &NoveltyCurve) -> Confidence {
    if beats.len() < 2 || period <= 0.0 {
        return Confidence::NONE;
    }

    let mut supported = 0_usize;
    for beat in beats {
        let frame = curve.index_at(beat.sample());
        let value = onset.get(frame).copied().unwrap_or(0.0);
        // The curve is normalised to unit standard deviation, so a value of one
        // is a full deviation above the local background — a real event rather
        // than a ripple.
        if value >= 1.0 {
            supported += 1;
        }
    }
    let support = count_to_f64(supported) / count_to_f64(beats.len());

    let mut deviation_total = 0.0_f64;
    let mut intervals = 0_usize;
    let expected = period * count_to_f64(curve.hop());
    for pair in beats.windows(2) {
        let (Some(first), Some(second)) = (pair.first(), pair.get(1)) else {
            continue;
        };
        let spacing = signed_to_f64(
            i64::try_from(second.sample()).unwrap_or(0)
                - i64::try_from(first.sample()).unwrap_or(0),
        );
        if spacing <= 0.0 || expected <= 0.0 {
            continue;
        }
        deviation_total += (spacing / expected).ln().abs();
        intervals += 1;
    }
    let steadiness = if intervals == 0 {
        0.0
    } else {
        let mean = deviation_total / count_to_f64(intervals);
        // A mean absolute log-ratio of 0.05 is five per cent spacing error,
        // which is where a grid visibly stops matching the transients.
        (1.0 - mean / 0.05).clamp(0.0, 1.0)
    };

    Confidence::from_f64(support).and_then(Confidence::from_f64(steadiness))
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
    use crate::testing::{click_track, samples, write_click};
    use crate::{tempo, NoveltyCurve};

    struct Tracked {
        estimate: BeatEstimate,
        period: usize,
    }

    fn track_clicks(bpm: f64, beats: usize) -> Tracked {
        let rate = SampleRate::HZ_44100;
        let period = round_to_count(f64::from(rate.hz()) * 60.0 / bpm);
        let signal = click_track(period, beats, rate);
        let mut stft = Stft::for_rhythm().expect("valid");
        let curve = NoveltyCurve::compute(&mut stft, &signal, rate).expect("valid");
        let estimate = tempo::estimate(&curve).expect("a click track has a tempo");
        let tracked = track(&curve, &estimate).expect("a click track has beats");
        Tracked {
            estimate: tracked,
            period,
        }
    }

    #[test]
    fn beats_land_on_the_clicks() {
        let tracked = track_clicks(120.0, 64);
        let beats = tracked.estimate.beats();
        assert!(
            beats.len() >= 60,
            "only {} beats found in a 64-beat track",
            beats.len()
        );

        for beat in beats {
            let offset = beat.sample() % tracked.period;
            let error = offset.min(tracked.period - offset);
            assert!(
                error <= 1024,
                "beat at {} is {error} samples from a click",
                beat.sample()
            );
        }
    }

    #[test]
    fn the_grid_does_not_drift_over_a_long_track() {
        // The failure this module exists to prevent. Stepping blindly by a
        // period would accumulate; the dynamic programme re-anchors on every
        // beat, so the last beat must be as accurate as the first.
        let tracked = track_clicks(128.0, 256);
        let beats = tracked.estimate.beats();
        let Some(last) = beats.last() else {
            panic!("no beats were tracked");
        };
        let offset = last.sample() % tracked.period;
        let error = offset.min(tracked.period - offset);
        assert!(
            error <= 1024,
            "the last beat of a 256-beat track is {error} samples off"
        );
    }

    #[test]
    fn the_tracker_coasts_through_a_gap_and_recovers() {
        // A breakdown: eight beats of silence in the middle. A greedy tracker
        // loses the grid here and never returns to it. This is the test that
        // justifies the dynamic programme over the simpler alternative.
        let rate = SampleRate::HZ_44100;
        let period = round_to_count(f64::from(rate.hz()) * 0.5);
        let total = 64_usize;
        let mut signal = vec![0.0_f32; period * total + period];
        for index in 0..total {
            if (16..24).contains(&index) {
                continue;
            }
            write_click(&mut signal, index * period, 0x9E_37 + index as u64, rate);
        }

        let mut stft = Stft::for_rhythm().expect("valid");
        let curve = NoveltyCurve::compute(&mut stft, &signal, rate).expect("valid");
        let estimate = tempo::estimate(&curve).expect("has a tempo");
        let tracked = track(&curve, &estimate).expect("has beats");

        let after_gap: Vec<&Beat> = tracked
            .beats()
            .iter()
            .filter(|beat| beat.sample() > 30 * period)
            .collect();
        assert!(
            after_gap.len() > 20,
            "only {} beats after the gap",
            after_gap.len()
        );
        for beat in after_gap {
            let offset = beat.sample() % period;
            let error = offset.min(period - offset);
            assert!(
                error <= 1500,
                "after the breakdown, a beat at {} is {error} samples off",
                beat.sample()
            );
        }
    }

    #[test]
    fn the_downbeat_follows_the_low_end_rather_than_the_onsets() {
        // Every beat has a click; only every fourth has bass under it. The
        // onset strengths are therefore nearly equal on all four positions, so
        // a tracker that chose the phase by onset strength would decide this by
        // noise. Choosing by low-band energy gets it right.
        let rate = SampleRate::HZ_44100;
        let period = round_to_count(f64::from(rate.hz()) * 0.5);
        let total = 64_usize;
        let mut signal = vec![0.0_f32; period * total + period];
        for index in 0..total {
            write_click(&mut signal, index * period, 0xB5_29 + index as u64, rate);
        }

        // A 55 Hz kick on every fourth beat, offset by one so that the
        // downbeat is beat 1 rather than beat 0 — a phase the tracker cannot
        // get right by accident. The kick decays over about 150 milliseconds,
        // which is what a kick does; a tone sustained for the whole beat would
        // be a bass *note*, and a note that lasts a beat says nothing about
        // where within the beat its energy is.
        let kick_length = samples(0.15, rate);
        let kick = crate::testing::tone(55.0, kick_length, 0.8, rate);
        for index in (1..total).step_by(4) {
            let start = index * period;
            for (offset, &value) in kick.iter().enumerate() {
                let decay = (-4.0 * count_to_f64(offset) / count_to_f64(kick_length)).exp();
                if let Some(slot) = signal.get_mut(start + offset) {
                    *slot += narrow(f64::from(value) * decay);
                }
            }
        }

        let mut stft = Stft::for_rhythm().expect("valid");
        let curve = NoveltyCurve::compute(&mut stft, &signal, rate).expect("valid");
        let estimate = tempo::estimate(&curve).expect("has a tempo");
        let tracked = track(&curve, &estimate).expect("has beats");

        let downbeats: Vec<usize> = tracked.downbeats().map(Beat::sample).collect();
        assert!(
            downbeats.len() >= 12,
            "only {} downbeats in a 64-beat track",
            downbeats.len()
        );

        let mut on_bass = 0_usize;
        for sample in &downbeats {
            let beat_index = round_to_count(count_to_f64(*sample) / count_to_f64(period));
            if beat_index % 4 == 1 {
                on_bass += 1;
            }
        }
        assert!(
            on_bass * 4 >= downbeats.len() * 3,
            "only {on_bass} of {} downbeats landed on the bass",
            downbeats.len()
        );
        assert!(
            tracked.downbeat_confidence().value() > 0.0,
            "a clear downbeat pattern produced no confidence"
        );
    }

    #[test]
    fn bar_positions_cycle_and_start_at_a_downbeat() {
        let tracked = track_clicks(120.0, 64);
        let beats = tracked.estimate.beats();
        let mut expected: Option<u8> = None;
        for beat in beats {
            if let Some(want) = expected {
                assert_eq!(beat.bar_position(), want, "bar positions must cycle");
            }
            expected = Some((beat.bar_position() + 1) % 4);
            assert!(beat.bar_position() < 4);
        }
        assert!(
            beats.iter().any(Beat::is_downbeat),
            "no beat was marked as a downbeat"
        );
    }

    #[test]
    fn a_beat_grid_is_anchored_on_the_first_downbeat() {
        // The handover to exact integer time. Everything after this point is
        // integer arithmetic in prv-time, so the grid must start where the
        // analysis says the music does.
        let tracked = track_clicks(120.0, 64);
        let grid = tracked.estimate.to_beat_grid(TimeSignature::FOUR_FOUR);
        assert_eq!(grid.origin(), tracked.estimate.first_downbeat());
        assert!(
            (grid.tempo_map().tempo_at_frames(Frames::ZERO).bpm() - 120.0).abs() < 0.1,
            "the grid carries the wrong tempo"
        );
    }

    #[test]
    fn the_fitted_tempo_is_accurate_to_a_hundredth_of_a_beat_per_minute() {
        // The claim the coarse estimator deliberately does not make. Fitting a
        // line through every tracked beat reduces the per-beat quantisation
        // error by a factor that grows with the number of beats, which is what
        // makes a grid that still lines up at the end of a six-minute track.
        //
        // The tempi here are chosen not to be whole numbers of analysis frames,
        // because a period that happens to land on a frame boundary would let a
        // much worse estimator pass.
        let rate = SampleRate::HZ_44100;
        // Tempi the tapping prior does not re-octave. A track at 174 BPM is
        // reported at 87 with the double on offer, which `tempo` tests
        // deliberately; mixing that case in here would be testing two things at
        // once and would fail for the right reason at the wrong assertion.
        for bpm in [120.0, 127.3, 133.9, 96.4] {
            let period = round_to_count(f64::from(rate.hz()) * 60.0 / bpm);
            // The exact tempo of the generated signal, which is not quite the
            // requested one because a click track has whole-sample spacing.
            let exact = f64::from(rate.hz()) * 60.0 / count_to_f64(period);

            let signal = click_track(period, 192, rate);
            let mut stft = Stft::for_rhythm().expect("valid");
            let curve = NoveltyCurve::compute(&mut stft, &signal, rate).expect("valid");
            let estimate = tempo::estimate(&curve).expect("has a tempo");
            let tracked = track(&curve, &estimate).expect("has beats");

            let error = (tracked.period_frames() - count_to_f64(period)).abs();
            assert!(
                error < 2.0,
                "at {exact} BPM the fitted period is {} frames rather than {period}",
                tracked.period_frames()
            );

            let measured = tracked.tempo().bpm();
            assert!(
                (measured - exact).abs() < 0.01,
                "expected {exact} BPM, the fitted grid gives {measured}"
            );
        }
    }

    #[test]
    fn the_grid_origin_is_within_a_few_milliseconds_of_the_first_beat() {
        // A constant offset on the whole grid is the timing error a listener
        // notices first, and it is the one the early-firing flux detector
        // introduces. This bounds it.
        let rate = SampleRate::HZ_44100;
        let tracked = track_clicks(120.0, 96);
        // The origin is the first *downbeat*, so on a bare click track — where
        // nothing distinguishes one beat of the bar from another — it may be
        // any of the first four beats. What is being tested is that it lands on
        // a beat, not which one.
        let origin = tracked.estimate.first_downbeat().get();
        let within = samples(0.010, rate);
        assert!(origin >= 0, "the grid origin is negative: {origin}");
        let offset = round_to_count(signed_to_f64(origin)) % tracked.period;
        let error = offset.min(tracked.period - offset);
        assert!(
            error <= within,
            "the grid origin is {error} samples off a beat, more than 10 ms"
        );
    }

    #[test]
    fn a_steady_click_track_deviates_almost_nothing_from_constant_tempo() {
        let tracked = track_clicks(124.0, 96);
        let deviation = tracked.estimate.deviation_from_constant_tempo();
        assert!(
            deviation < 300.0,
            "a machine-steady track deviated {deviation} samples from a constant grid"
        );
    }

    #[test]
    fn silence_after_a_valid_tempo_is_refused_rather_than_gridded() {
        let rate = SampleRate::HZ_44100;
        let period = round_to_count(f64::from(rate.hz()) * 0.5);
        let signal = click_track(period, 64, rate);
        let mut stft = Stft::for_rhythm().expect("valid");
        let curve = NoveltyCurve::compute(&mut stft, &signal, rate).expect("valid");
        let estimate = tempo::estimate(&curve).expect("has a tempo");

        let quiet = vec![0.0_f32; samples(20.0, rate)];
        let silent_curve = NoveltyCurve::compute(&mut stft, &quiet, rate).expect("valid");
        let tracked = track(&silent_curve, &estimate).expect("silence still produces a path");
        assert!(
            !tracked.beat_confidence().is_actionable(),
            "beats were tracked through silence with confidence {}",
            tracked.beat_confidence()
        );
    }

    #[test]
    fn tracking_is_reproducible() {
        let first = track_clicks(126.0, 64);
        let second = track_clicks(126.0, 64);
        let left: Vec<usize> = first.estimate.beats().iter().map(Beat::sample).collect();
        let right: Vec<usize> = second.estimate.beats().iter().map(Beat::sample).collect();
        assert_eq!(left, right);
    }
}
