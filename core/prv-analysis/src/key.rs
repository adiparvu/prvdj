//! Key detection.
//!
//! # How it works
//!
//! Compare the track's [`Chroma`] against a reference profile for each of the
//! twenty-four keys and take the best match. The reference profiles are
//! Krumhansl and Kessler's, measured by asking listeners how well each of the
//! twelve pitch classes fits an established key — so the model being fitted is
//! a fact about how people hear tonality, not a heuristic invented here.
//!
//! The comparison is a Pearson correlation rather than a dot product. A dot
//! product rewards a chroma that is simply *large* in the right places, which
//! means a track with one very dominant note matches everything containing that
//! note. Correlation compares shapes, so what is being asked is whether the
//! track's distribution of emphasis resembles the key's, which is the question.
//!
//! # The ambiguity that will not go away
//!
//! C major and A minor use the same seven notes. They differ only in which note
//! feels like home, and that is carried by *when* notes occur — the bass at
//! phrase ends, the final chord — not by how much of each there is. A chroma
//! vector has thrown that information away by construction.
//!
//! The profiles do separate them, because listeners weight the tonic and the
//! fifth more heavily, but the margin is small and a real track can fall either
//! way. This module therefore treats the relative key as a first-class
//! alternative rather than an error: it is always in
//! [`KeyEstimate::alternatives`], and the confidence reflects how close the call
//! was.
//!
//! The practical consequence is smaller than it sounds. On the Camelot wheel a
//! key and its relative sit at the same number, and `prv-harmony` scores that
//! relation as safe — so a track mixed on the strength of the relative key still
//! mixes. Master Prompt #25 requires the product to be honest about uncertainty;
//! here that means saying "C major, possibly A minor" rather than picking one
//! and hoping.

use prv_harmony::{Key, Mode, PitchClass};
use prv_time::SampleRate;

use crate::chroma::Chroma;
use crate::confidence::Confidence;
use crate::error::AnalysisError;
use crate::num::{count_to_f64, narrow};
use crate::spectrum::Stft;

/// The number of pitch classes.
const PITCH_CLASSES: usize = 12;

/// Krumhansl and Kessler's major profile, starting from the tonic.
///
/// Reproduced as published. These are averaged listener ratings of how well
/// each pitch class fits a major context, and they are left exactly as measured
/// rather than rounded or renormalised, so that anyone checking them against the
/// source finds the same numbers.
const MAJOR_PROFILE: [f64; PITCH_CLASSES] = [
    6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88,
];

/// Krumhansl and Kessler's minor profile, starting from the tonic.
const MINOR_PROFILE: [f64; PITCH_CLASSES] = [
    6.33, 2.68, 3.52, 5.38, 2.60, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17,
];

/// The minimum number of frames with tonal content required for an estimate.
///
/// Below this the chroma is built from too little evidence for a correlation to
/// mean anything, and [`AnalysisError::NotEnoughAudio`] is returned rather than
/// a low-confidence key. The distinction matters: a low confidence should mean
/// "this music is tonally ambiguous", not "there was hardly any music".
const MINIMUM_TONAL_FRAMES: usize = 32;

/// One key with the evidence for it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KeyCandidate {
    key: Key,
    correlation: f32,
}

impl KeyCandidate {
    /// The key.
    #[must_use]
    pub const fn key(&self) -> Key {
        self.key
    }

    /// How well the track's chroma correlated with this key's profile, from
    /// minus one to one.
    ///
    /// The raw evidence, not a confidence. It is comparable across keys within
    /// one track and roughly comparable between tracks, which is what makes the
    /// alternatives list readable.
    #[must_use]
    pub const fn correlation(&self) -> f32 {
        self.correlation
    }
}

/// The result of key detection.
#[derive(Debug, Clone)]
pub struct KeyEstimate {
    key: Key,
    confidence: Confidence,
    candidates: Vec<KeyCandidate>,
    chroma: Chroma,
}

impl KeyEstimate {
    /// The key the detector believes.
    #[must_use]
    pub const fn key(&self) -> Key {
        self.key
    }

    /// How well determined the key was.
    #[must_use]
    pub const fn confidence(&self) -> Confidence {
        self.confidence
    }

    /// Every key, ordered by how well it fitted, best first.
    ///
    /// All twenty-four are kept rather than a shortlist. The list is small, and
    /// a user disagreeing with the detector is usually choosing a key that
    /// scored second or third — showing why is more useful than showing that
    /// the detector had an opinion.
    #[must_use]
    pub fn candidates(&self) -> &[KeyCandidate] {
        &self.candidates
    }

    /// The relative major or minor of the detected key, with its own evidence.
    ///
    /// Always present, because the relative key is the one confusion a chroma
    /// cannot rule out.
    #[must_use]
    pub fn relative(&self) -> Option<KeyCandidate> {
        let relative = self.key.relative();
        self.candidates
            .iter()
            .find(|candidate| candidate.key == relative)
            .copied()
    }

    /// The chroma the estimate was derived from.
    ///
    /// Exposed because ADR-0006 requires an explanation to render the evidence
    /// that produced a decision rather than describe it. An interface showing
    /// "C major, because these are the notes that dominate" needs the notes.
    #[must_use]
    pub const fn chroma(&self) -> Chroma {
        self.chroma
    }
}

/// Detects the key from a chroma profile.
///
/// # Errors
///
/// Returns [`AnalysisError::NotEnoughAudio`] when too few frames carried tonal
/// content for a correlation to be meaningful.
pub fn detect(chroma: Chroma, tonal_frames: usize) -> Result<KeyEstimate, AnalysisError> {
    if tonal_frames < MINIMUM_TONAL_FRAMES {
        return Err(AnalysisError::NotEnoughAudio {
            frames: tonal_frames,
            minimum: MINIMUM_TONAL_FRAMES,
        });
    }
    if chroma.is_silent() {
        return Err(AnalysisError::NotEnoughAudio {
            frames: 0,
            minimum: MINIMUM_TONAL_FRAMES,
        });
    }

    let mut candidates: Vec<KeyCandidate> = Vec::with_capacity(PITCH_CLASSES * 2);
    for &tonic in &PitchClass::ALL {
        let rotated = chroma.rotated(tonic);
        let observed: Vec<f64> = rotated.weights().iter().map(|&w| f64::from(w)).collect();
        for mode in [Mode::Major, Mode::Minor] {
            let profile = match mode {
                Mode::Major => &MAJOR_PROFILE,
                Mode::Minor => &MINOR_PROFILE,
            };
            candidates.push(KeyCandidate {
                key: Key::new(tonic, mode),
                correlation: narrow(correlation(&observed, profile)),
            });
        }
    }

    // Sorted by correlation, with the key itself as a tiebreak so the order is
    // total and reproducible. ADR-0006 requires the same inputs to produce the
    // same output, and a sort whose result depends on the order equal elements
    // happen to arrive in would quietly break that.
    candidates.sort_by(|a, b| {
        b.correlation
            .partial_cmp(&a.correlation)
            .unwrap_or(core::cmp::Ordering::Equal)
            .then_with(|| a.key.tonic.semitones().cmp(&b.key.tonic.semitones()))
            .then_with(|| mode_order(a.key.mode).cmp(&mode_order(b.key.mode)))
    });

    let Some(&best) = candidates.first() else {
        return Err(AnalysisError::NotEnoughAudio {
            frames: tonal_frames,
            minimum: MINIMUM_TONAL_FRAMES,
        });
    };

    Ok(KeyEstimate {
        key: best.key,
        confidence: confidence_for(&candidates),
        candidates,
        chroma,
    })
}

/// Detects the key of a mono signal, running the transform for the caller.
///
/// # Errors
///
/// Propagates every error from [`detect`] and from the transform.
pub fn detect_signal(
    stft: &mut Stft,
    samples: &[f32],
    sample_rate: SampleRate,
) -> Result<KeyEstimate, AnalysisError> {
    let (chroma, frames) = crate::chroma::compute(stft, samples, sample_rate)?;
    detect(chroma, frames)
}

/// Orders the modes so that ties break the same way every run.
const fn mode_order(mode: Mode) -> u8 {
    match mode {
        Mode::Major => 0,
        Mode::Minor => 1,
    }
}

/// Pearson correlation between an observed profile and a reference.
fn correlation(observed: &[f64], reference: &[f64]) -> f64 {
    let count = count_to_f64(PITCH_CLASSES);
    let observed_mean = observed.iter().sum::<f64>() / count;
    let reference_mean = reference.iter().sum::<f64>() / count;

    let mut covariance = 0.0_f64;
    let mut observed_variance = 0.0_f64;
    let mut reference_variance = 0.0_f64;
    for (&a, &b) in observed.iter().zip(reference.iter()) {
        let left = a - observed_mean;
        let right = b - reference_mean;
        covariance += left * right;
        observed_variance += left * left;
        reference_variance += right * right;
    }

    let denominator = (observed_variance * reference_variance).sqrt();
    if denominator <= 0.0 {
        // A perfectly flat chroma correlates with nothing. Returning zero
        // rather than a division by zero keeps the result finite and, more
        // usefully, keeps every key equally unsupported — which is the truth
        // about a track with no tonal centre.
        return 0.0;
    }
    covariance / denominator
}

/// Derives a confidence from how clearly the best key beat the alternatives.
///
/// Two doubts, and they are different failures.
///
/// The first is whether the chroma resembles *any* key: a track built from
/// noise, or from a single sustained note, correlates weakly with all
/// twenty-four, and its top score being highest means little.
///
/// The second is the margin, measured against the best key that is **not** the
/// relative. The relative major or minor is deliberately excluded from that
/// comparison, because it will always score close — it uses the same notes —
/// and including it would cap the confidence of every correctly detected key at
/// the level of an ambiguity that barely matters in practice. A key and its
/// relative sit at the same Camelot number, so a DJ acting on either mixes the
/// same records.
fn confidence_for(candidates: &[KeyCandidate]) -> Confidence {
    let Some(&best) = candidates.first() else {
        return Confidence::NONE;
    };

    let top = f64::from(best.correlation);
    if top <= 0.0 {
        return Confidence::NONE;
    }

    // A correlation of 0.8 with a listener-derived profile is about as good as
    // real music gets; the scale saturates there. Provisional, like every
    // number that becomes a confidence, and calibrated in Phase 3.
    let fit = (top / 0.8).clamp(0.0, 1.0);

    let relative = best.key.relative();
    let rival = candidates
        .iter()
        .skip(1)
        .find(|candidate| candidate.key != relative)
        .map_or(0.0, |candidate| f64::from(candidate.correlation));

    let margin = if top <= 0.0 {
        0.0
    } else {
        ((top - rival) / top).clamp(0.0, 1.0)
    };
    // A fifth of the top score separating the winner from its nearest
    // unrelated rival is decisive.
    let separation = (margin * 5.0).clamp(0.0, 1.0);

    Confidence::from_f64(fit).and_then(Confidence::from_f64(separation))
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
    use crate::testing::{samples, tone};

    /// Builds a chroma from note weights, bypassing the transform.
    ///
    /// Key detection and chroma extraction are separate stages with separate
    /// failure modes, and testing them together would mean every key test could
    /// also fail for a spectral reason. These tests exercise the correlation
    /// alone; `chroma` tests the extraction.
    fn build_chroma(weights: [f32; PITCH_CLASSES]) -> Chroma {
        Chroma::from_weights(weights)
    }

    /// The frequency of a note, given semitones above concert A.
    fn note(semitones_from_a: i32) -> f64 {
        440.0 * 2.0_f64.powf(f64::from(semitones_from_a) / 12.0)
    }

    #[test]
    fn a_c_major_scale_is_detected_as_c_major_or_its_relative() {
        // The honest assertion. A scale contains no cadence and no bass motion,
        // so nothing in it distinguishes C major from A minor — the two use the
        // same seven notes. What must be true is that the detector lands on one
        // of them and says so, not that it guesses the one the test author had
        // in mind.
        let rate = SampleRate::HZ_44100;
        let each = samples(0.5, rate);
        let mut signal = Vec::new();
        // C D E F G A B C, from C4 up.
        for semitones in [-9, -7, -5, -4, -2, 0, 2, 3] {
            signal.extend(tone(note(semitones), each, 0.5, rate));
        }

        let mut stft = Stft::for_tone().expect("valid");
        let estimate = detect_signal(&mut stft, &signal, rate).expect("a scale has a key");

        let c_major = Key::major(PitchClass::C);
        let a_minor = Key::minor(PitchClass::A);
        assert!(
            estimate.key() == c_major || estimate.key() == a_minor,
            "a C major scale was detected as {:?}",
            estimate.key()
        );
        assert_eq!(
            estimate.key().relative(),
            if estimate.key() == c_major {
                a_minor
            } else {
                c_major
            }
        );
    }

    #[test]
    fn the_relative_is_always_offered() {
        let rate = SampleRate::HZ_44100;
        let each = samples(0.4, rate);
        let mut signal = Vec::new();
        for semitones in [-9, -5, -2, -9, -4, 0] {
            signal.extend(tone(note(semitones), each, 0.5, rate));
        }

        let mut stft = Stft::for_tone().expect("valid");
        let estimate = detect_signal(&mut stft, &signal, rate).expect("has a key");

        let relative = estimate.relative().expect("the relative must be offered");
        assert_eq!(relative.key(), estimate.key().relative());
        assert_eq!(estimate.candidates().len(), 24, "all keys must be reported");
    }

    #[test]
    fn a_tonic_heavy_profile_beats_its_relative() {
        // The one thing a chroma *can* say about the major-minor question:
        // which note is emphasised. A profile weighted toward C should read as C
        // major rather than A minor even though both fit the note set.
        let mut weights = [0.0_f32; PITCH_CLASSES];
        // C major scale, with the tonic and dominant emphasised as a real
        // recording would emphasise them.
        weights[0] = 0.30; // C
        weights[2] = 0.08; // D
        weights[4] = 0.14; // E
        weights[5] = 0.09; // F
        weights[7] = 0.22; // G
        weights[9] = 0.10; // A
        weights[11] = 0.07; // B
        let chroma = build_chroma(weights);

        let estimate = detect(chroma, 500).expect("has a key");
        assert_eq!(estimate.key(), Key::major(PitchClass::C));
        assert!(
            estimate.confidence().is_actionable(),
            "a clear major profile gave confidence {}",
            estimate.confidence()
        );
    }

    #[test]
    fn a_minor_heavy_profile_beats_its_relative() {
        let mut weights = [0.0_f32; PITCH_CLASSES];
        // The same note set, weighted toward A and E.
        weights[9] = 0.30; // A
        weights[11] = 0.08; // B
        weights[0] = 0.13; // C
        weights[2] = 0.09; // D
        weights[4] = 0.22; // E
        weights[5] = 0.10; // F
        weights[7] = 0.08; // G
        let chroma = build_chroma(weights);

        let estimate = detect(chroma, 500).expect("has a key");
        assert_eq!(estimate.key(), Key::minor(PitchClass::A));
    }

    #[test]
    fn a_flat_profile_is_not_given_a_confident_key() {
        // Twelve equal pitch classes is what noise looks like. It correlates
        // with nothing, and the detector must say so rather than reporting
        // whichever key won by a rounding error.
        let chroma = build_chroma([1.0 / 12.0; PITCH_CLASSES]);
        let estimate = detect(chroma, 500).expect("still produces a result");
        assert!(
            !estimate.confidence().is_actionable(),
            "a flat chroma was given {:?} with confidence {}",
            estimate.key(),
            estimate.confidence()
        );
    }

    #[test]
    fn too_little_tonal_content_is_refused_rather_than_guessed() {
        let mut weights = [0.0_f32; PITCH_CLASSES];
        weights[0] = 1.0;
        assert!(matches!(
            detect(build_chroma(weights), 4),
            Err(AnalysisError::NotEnoughAudio { .. })
        ));
        assert!(matches!(
            detect(Chroma::SILENT, 500),
            Err(AnalysisError::NotEnoughAudio { .. })
        ));
    }

    #[test]
    fn transposing_a_profile_transposes_the_key() {
        // The property that shows the twenty-four comparisons are one shape
        // rotated rather than twenty-four independent tables that could each be
        // wrong differently.
        let mut weights = [0.0_f32; PITCH_CLASSES];
        weights[0] = 0.30;
        weights[2] = 0.08;
        weights[4] = 0.14;
        weights[5] = 0.09;
        weights[7] = 0.22;
        weights[9] = 0.10;
        weights[11] = 0.07;

        for shift in 0..PITCH_CLASSES {
            let mut shifted = [0.0_f32; PITCH_CLASSES];
            for (index, &value) in weights.iter().enumerate() {
                shifted[(index + shift) % PITCH_CLASSES] = value;
            }
            let estimate = detect(build_chroma(shifted), 500).expect("has a key");
            let expected = Key::major(PitchClass::from_semitones(u8::try_from(shift).unwrap_or(0)));
            assert_eq!(
                estimate.key(),
                expected,
                "shifting by {shift} semitones gave {:?}",
                estimate.key()
            );
        }
    }

    #[test]
    fn detection_is_reproducible() {
        let mut weights = [0.0_f32; PITCH_CLASSES];
        weights[3] = 0.4;
        weights[7] = 0.3;
        weights[10] = 0.3;
        let first = detect(build_chroma(weights), 500).expect("has a key");
        let second = detect(build_chroma(weights), 500).expect("has a key");
        assert_eq!(first.key(), second.key());
        assert_eq!(first.confidence(), second.confidence());
        let left: Vec<Key> = first.candidates().iter().map(KeyCandidate::key).collect();
        let right: Vec<Key> = second.candidates().iter().map(KeyCandidate::key).collect();
        assert_eq!(left, right);
    }

    #[test]
    fn the_published_profiles_are_intact() {
        // A typo in a reference profile is a defect that produces plausible
        // wrong keys forever, so the numbers are pinned by the two properties
        // that identify them rather than by copying them out again — a second
        // copy of a table is not a check on the first.
        //
        // In both profiles the tonic is the largest. The *second* largest
        // differs, and that difference is the whole of the major-minor
        // distinction: in major it is the dominant, and in minor it is the
        // minor third — the note that makes the mode minor. Getting this
        // backwards would produce a detector that confuses every key with its
        // parallel.
        for (profile, defining) in [(&MAJOR_PROFILE, 7_usize), (&MINOR_PROFILE, 3_usize)] {
            let tonic = profile[0];
            assert!(
                profile.iter().all(|&value| value <= tonic),
                "the tonic is not the largest entry"
            );
            let second = profile[defining];
            let larger = profile
                .iter()
                .enumerate()
                .filter(|&(index, &value)| index != 0 && value > second)
                .count();
            assert_eq!(
                larger, 0,
                "the entry at {defining} is not the second largest"
            );
        }
    }
}
