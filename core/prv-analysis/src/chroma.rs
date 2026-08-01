//! Chroma: how much of each of the twelve pitch classes a track contains.
//!
//! # What is being folded away
//!
//! A chroma vector discards octave. C2, C4 and C6 all land in the same bin, and
//! that is the point: the question a key detector asks is which *notes* the
//! music uses, not which registers they were played in. A bassline an octave
//! below the melody is playing the same harmony.
//!
//! # Three decisions that separate a usable chroma from a smear
//!
//! **Only spectral peaks contribute.** A kick drum is broadband: it deposits
//! energy in every bin, and therefore an equal amount in every pitch class,
//! which raises the floor of the chroma vector without adding information.
//! Accumulating only bins that are local maxima keeps sustained pitched content
//! and discards most percussion, because a partial is a peak and noise is not.
//!
//! **Each frame is normalised before it is added.** Without this the loudest
//! forty seconds of a track determine its key. A breakdown in a different key
//! would contribute nothing, and a track that modulates would be analysed as
//! though only its loudest section existed. Normalising per frame makes the
//! result a statement about the whole record.
//!
//! **The range is bounded at both ends.** Below about 65 Hz the analysis window
//! cannot separate adjacent semitones, so anything assigned there is a guess;
//! above about 2 kHz almost all the energy is upper partials rather than
//! fundamentals, and partials of a note are mostly *other* pitch classes — the
//! third harmonic of C is G. Including them adds a smear of fifths to every
//! chroma vector, which is exactly the confusion a key detector must avoid.

use prv_harmony::PitchClass;
use prv_time::SampleRate;

use crate::error::AnalysisError;
use crate::num::{count_to_f64, narrow};
use crate::spectrum::{SpectrumFrame, Stft};

/// The lowest frequency that contributes to chroma, in hertz.
///
/// Roughly C2. Below this the tonal analysis window resolves about 11 Hz while
/// adjacent semitones are less than 4 Hz apart, so a bin cannot be attributed
/// to one pitch class rather than its neighbour. Bass notes below this still
/// reach the chroma through their second harmonic, which is the same pitch
/// class an octave up.
const LOWEST_HZ: f64 = 65.0;

/// The highest frequency that contributes to chroma, in hertz.
///
/// Roughly C7. Above this the spectrum is almost entirely upper partials, and a
/// partial usually belongs to a different pitch class than the note that
/// produced it. Including them would add a systematic bias toward the fifth of
/// whatever is playing.
const HIGHEST_HZ: f64 = 2_100.0;

/// The reference pitch, in hertz.
///
/// Concert A. Recordings that are not at concert pitch — older transfers, tape
/// played at the wrong speed, deliberately detuned productions — will have their
/// energy fall between semitone centres. That is a real limitation, recorded
/// rather than hidden: the chroma degrades gracefully into a smear across two
/// neighbours rather than failing, and detecting the tuning offset is a separate
/// stage that does not exist yet.
const REFERENCE_HZ: f64 = 440.0;

/// The number of pitch classes.
const PITCH_CLASSES: usize = 12;

/// A twelve-element pitch-class profile.
///
/// Normalised so that the elements sum to one, which makes profiles from
/// different tracks — and from passages of different length or loudness —
/// directly comparable. A raw magnitude sum would not be.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Chroma {
    weights: [f32; PITCH_CLASSES],
}

impl Chroma {
    /// A profile with no content.
    pub const SILENT: Self = Self {
        weights: [0.0; PITCH_CLASSES],
    };

    /// Builds a profile from twelve weights, indexed from C, normalising them
    /// to sum to one.
    ///
    /// Exists so that a stored profile can be read back and so that a caller
    /// can compare against a shape it constructed. Negative and non-finite
    /// weights are treated as zero: a chroma is an amount of energy, and a
    /// negative amount is a defect upstream rather than a value to propagate.
    #[must_use]
    pub fn from_weights(weights: [f32; PITCH_CLASSES]) -> Self {
        let mut cleaned = [0.0_f32; PITCH_CLASSES];
        let mut total = 0.0_f64;
        for (slot, &weight) in cleaned.iter_mut().zip(weights.iter()) {
            if weight.is_finite() && weight > 0.0 {
                *slot = weight;
                total += f64::from(weight);
            }
        }
        if total <= 0.0 {
            return Self::SILENT;
        }
        for slot in &mut cleaned {
            *slot = narrow(f64::from(*slot) / total);
        }
        Self { weights: cleaned }
    }

    /// The weight of one pitch class.
    #[must_use]
    pub fn weight(&self, pitch: PitchClass) -> f32 {
        self.weights
            .get(usize::from(pitch.semitones()))
            .copied()
            .unwrap_or(0.0)
    }

    /// All twelve weights, indexed from C.
    #[must_use]
    pub const fn weights(&self) -> &[f32; PITCH_CLASSES] {
        &self.weights
    }

    /// The pitch class with the most weight, if any has any.
    #[must_use]
    pub fn strongest(&self) -> Option<PitchClass> {
        let mut best: Option<(usize, f32)> = None;
        for (index, &weight) in self.weights.iter().enumerate() {
            if weight <= 0.0 {
                continue;
            }
            match best {
                Some((_, previous)) if previous >= weight => {}
                _ => best = Some((index, weight)),
            }
        }
        best.map(|(index, _)| PitchClass::from_semitones(u8::try_from(index).unwrap_or(0)))
    }

    /// Whether the profile carries any content at all.
    #[must_use]
    pub fn is_silent(&self) -> bool {
        self.weights.iter().all(|&weight| weight <= 0.0)
    }

    /// Returns the profile rotated so that `tonic` sits at index zero.
    ///
    /// Key detection compares one profile shape against twelve rotations rather
    /// than holding twelve copies of it, which is what keeps the published
    /// reference profiles in the source exactly as published.
    #[must_use]
    pub fn rotated(&self, tonic: PitchClass) -> Self {
        let offset = usize::from(tonic.semitones());
        let mut weights = [0.0_f32; PITCH_CLASSES];
        for (index, slot) in weights.iter_mut().enumerate() {
            *slot = self
                .weights
                .get((index + offset) % PITCH_CLASSES)
                .copied()
                .unwrap_or(0.0);
        }
        Self { weights }
    }
}

/// Accumulates a chroma profile over a signal.
///
/// Kept separate from [`Chroma`] because accumulation is stateful and the
/// result is not. A caller analysing a section rather than a whole track builds
/// one of these per section.
#[derive(Debug, Clone)]
pub struct ChromaBuilder {
    totals: [f64; PITCH_CLASSES],
    frames: usize,
}

impl Default for ChromaBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl ChromaBuilder {
    /// Creates an empty builder.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            totals: [0.0; PITCH_CLASSES],
            frames: 0,
        }
    }

    /// Adds one analysed frame.
    pub fn add(&mut self, frame: &SpectrumFrame<'_>) {
        let magnitude = frame.magnitude();
        let lowest = frame.bin_for_frequency(LOWEST_HZ).max(1);
        let highest = frame.bin_for_frequency(HIGHEST_HZ);
        if highest <= lowest {
            return;
        }

        let mut frame_totals = [0.0_f64; PITCH_CLASSES];
        let mut frame_sum = 0.0_f64;

        for bin in lowest..=highest {
            let (Some(&before), Some(&value), Some(&after)) = (
                magnitude.get(bin - 1),
                magnitude.get(bin),
                magnitude.get(bin + 1),
            ) else {
                continue;
            };
            // Only spectral peaks. A partial is a local maximum; broadband
            // percussion is not, and admitting it would raise every pitch class
            // equally.
            if value <= before || value < after || value <= 0.0 {
                continue;
            }

            // The peak's true frequency, refined by fitting a parabola through
            // it and its neighbours. Without this a note is attributed by which
            // bin it happened to land nearest, and at the bottom of the range a
            // bin is a large fraction of a semitone.
            let denominator = before - 2.0 * value + after;
            let offset = if denominator.abs() < f64::EPSILON {
                0.0
            } else {
                (0.5 * (before - after) / denominator).clamp(-0.5, 0.5)
            };
            let frequency = frame.bin_frequency(bin) + offset * frame.bin_frequency(1);
            let Some(pitch) = pitch_class_of(frequency) else {
                continue;
            };

            if let Some(slot) = frame_totals.get_mut(usize::from(pitch.semitones())) {
                *slot += value;
            }
            frame_sum += value;
        }

        if frame_sum <= 0.0 {
            return;
        }
        for (total, frame_total) in self.totals.iter_mut().zip(frame_totals.iter()) {
            *total += frame_total / frame_sum;
        }
        self.frames += 1;
    }

    /// The number of frames that contributed.
    ///
    /// Frames with no spectral peaks in range — silence, and pure percussion —
    /// do not count. A track whose count is very low relative to its length is
    /// one whose chroma should not be trusted, and the key stage uses this.
    #[must_use]
    pub const fn contributing_frames(&self) -> usize {
        self.frames
    }

    /// Finishes the profile.
    #[must_use]
    pub fn finish(&self) -> Chroma {
        let total: f64 = self.totals.iter().sum();
        if total <= 0.0 {
            return Chroma::SILENT;
        }
        let mut weights = [0.0_f32; PITCH_CLASSES];
        for (slot, &value) in weights.iter_mut().zip(self.totals.iter()) {
            *slot = narrow(value / total);
        }
        Chroma { weights }
    }
}

/// Computes the chroma profile of a mono signal.
///
/// # Errors
///
/// Propagates transform errors.
pub fn compute(
    stft: &mut Stft,
    samples: &[f32],
    sample_rate: SampleRate,
) -> Result<(Chroma, usize), AnalysisError> {
    let mut builder = ChromaBuilder::new();
    stft.analyse(samples, sample_rate, |frame| builder.add(frame))?;
    Ok((builder.finish(), builder.contributing_frames()))
}

/// The pitch class a frequency belongs to, or `None` if it is out of range.
fn pitch_class_of(frequency: f64) -> Option<PitchClass> {
    if !frequency.is_finite() || !(LOWEST_HZ..=HIGHEST_HZ).contains(&frequency) {
        return None;
    }
    // Semitones above concert A, which is pitch class 9.
    let semitones_from_a = 12.0 * (frequency / REFERENCE_HZ).log2();
    let rounded = semitones_from_a.round();
    if !rounded.is_finite() {
        return None;
    }
    // Wrapped into 0..12 with A at 9. `rem_euclid` rather than `%` because the
    // value is negative for everything below concert A, which is most of the
    // range, and a negative remainder would index the wrong pitch class.
    let index = (rounded + 9.0).rem_euclid(count_to_f64(PITCH_CLASSES));
    let index = crate::num::round_to_count(index) % PITCH_CLASSES;
    Some(PitchClass::from_semitones(u8::try_from(index).unwrap_or(0)))
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

    /// The frequency of a note, given semitones above concert A.
    fn note(semitones_from_a: i32) -> f64 {
        REFERENCE_HZ * 2.0_f64.powf(f64::from(semitones_from_a) / 12.0)
    }

    #[test]
    fn a_pure_tone_lands_in_its_own_pitch_class() {
        let rate = SampleRate::HZ_44100;
        let length = samples(2.0, rate);
        // A4, C5, E5 — a spread across the range rather than one convenient
        // frequency.
        for (semitones, expected) in [
            (0, PitchClass::A),
            (3, PitchClass::C),
            (7, PitchClass::E),
            (-9, PitchClass::C),
            (-21, PitchClass::C),
        ] {
            let signal = tone(note(semitones), length, 0.5, rate);
            let mut stft = Stft::for_tone().expect("valid");
            let (chroma, frames) = compute(&mut stft, &signal, rate).expect("valid");
            assert!(frames > 10, "only {frames} frames contributed");
            assert_eq!(
                chroma.strongest(),
                Some(expected),
                "a tone {semitones} semitones from A gave {:?}",
                chroma.weights()
            );
            assert!(
                chroma.weight(expected) > 0.8,
                "the tone's own pitch class holds only {}",
                chroma.weight(expected)
            );
        }
    }

    #[test]
    fn a_chord_shows_all_of_its_notes() {
        // A minor: A, C, E. The property that matters is that all three are
        // present and everything else is not, which is what a key detector
        // correlates against.
        let rate = SampleRate::HZ_44100;
        let length = samples(3.0, rate);
        let mut signal = vec![0.0_f32; length];
        for semitones in [0, 3, 7] {
            let partial = tone(note(semitones), length, 0.3, rate);
            for (slot, &value) in signal.iter_mut().zip(partial.iter()) {
                *slot += value;
            }
        }

        let mut stft = Stft::for_tone().expect("valid");
        let (chroma, _) = compute(&mut stft, &signal, rate).expect("valid");

        for pitch in [PitchClass::A, PitchClass::C, PitchClass::E] {
            assert!(
                chroma.weight(pitch) > 0.2,
                "{pitch:?} holds only {} of the chord",
                chroma.weight(pitch)
            );
        }
        let stray: f32 = PitchClass::ALL
            .iter()
            .filter(|pitch| !matches!(pitch, PitchClass::A | PitchClass::C | PitchClass::E))
            .map(|&pitch| chroma.weight(pitch))
            .sum();
        assert!(
            stray < 0.1,
            "notes outside the chord hold {stray} of the profile"
        );
    }

    #[test]
    fn percussion_does_not_colour_the_profile() {
        // The reason only spectral peaks contribute. A kick and a snare deposit
        // energy in every bin; if they reached the chroma they would raise all
        // twelve pitch classes and flatten the profile a key detector needs.
        let rate = SampleRate::HZ_44100;
        let length = samples(4.0, rate);

        let tonal = tone(note(3), length, 0.4, rate);
        let mut with_drums = tonal.clone();
        let period = samples(0.5, rate);
        let mut position = 0;
        let mut seed = 0x3141_u64;
        while position + samples(0.02, rate) < length {
            write_click(&mut with_drums, position, seed, rate);
            position += period;
            seed += 7;
        }

        let mut stft = Stft::for_tone().expect("valid");
        let (clean, _) = compute(&mut stft, &tonal, rate).expect("valid");
        let (noisy, _) = compute(&mut stft, &with_drums, rate).expect("valid");

        assert_eq!(clean.strongest(), Some(PitchClass::C));
        assert_eq!(
            noisy.strongest(),
            Some(PitchClass::C),
            "drums moved the strongest pitch class"
        );
        assert!(
            noisy.weight(PitchClass::C) > 0.6,
            "drums diluted the tonal content to {}",
            noisy.weight(PitchClass::C)
        );
    }

    #[test]
    fn a_quiet_passage_counts_as_much_as_a_loud_one() {
        // The reason frames are normalised before accumulation. Two seconds of
        // C at full level followed by two seconds of F sharp twenty decibels
        // down: without per-frame normalisation the profile would be C and
        // almost nothing else, and a track that modulates would be analysed as
        // though only its loudest section existed.
        let rate = SampleRate::HZ_44100;
        let length = samples(2.0, rate);
        let mut signal = tone(note(3), length, 0.5, rate);
        signal.extend(tone(note(9), length, 0.05, rate));

        let mut stft = Stft::for_tone().expect("valid");
        let (chroma, _) = compute(&mut stft, &signal, rate).expect("valid");

        let loud = chroma.weight(PitchClass::C);
        let quiet = chroma.weight(PitchClass::FSharp);
        assert!(
            quiet > loud * 0.5,
            "the quiet half contributed {quiet} against the loud half's {loud}"
        );
    }

    #[test]
    fn silence_produces_a_silent_profile() {
        let rate = SampleRate::HZ_44100;
        let mut stft = Stft::for_tone().expect("valid");
        let (chroma, frames) =
            compute(&mut stft, &vec![0.0_f32; samples(2.0, rate)], rate).expect("valid");
        assert!(chroma.is_silent());
        assert_eq!(chroma.strongest(), None);
        assert_eq!(frames, 0);
    }

    #[test]
    fn rotation_moves_the_tonic_to_the_front_and_is_reversible() {
        let mut weights = [0.0_f32; PITCH_CLASSES];
        weights[0] = 0.5;
        weights[4] = 0.3;
        weights[7] = 0.2;
        let chroma = Chroma { weights };

        let rotated = chroma.rotated(PitchClass::E);
        assert_eq!(rotated.weights()[0], 0.3);
        assert_eq!(rotated.weights()[8], 0.5);

        // Rotating by the complement restores the original, which is what makes
        // comparing a profile against twelve rotations equivalent to comparing
        // twelve profiles against it.
        let restored = rotated.rotated(PitchClass::GSharp);
        assert_eq!(restored.weights(), chroma.weights());
    }

    #[test]
    fn pitch_class_mapping_is_correct_below_concert_pitch() {
        // `rem_euclid` rather than `%`: everything below A4 gives a negative
        // semitone count, and a negative remainder would index a pitch class
        // several semitones away. Most of the useful range is below A4, so
        // getting this wrong would be wrong nearly everywhere.
        assert_eq!(pitch_class_of(note(0)), Some(PitchClass::A));
        assert_eq!(pitch_class_of(note(-1)), Some(PitchClass::GSharp));
        assert_eq!(pitch_class_of(note(-9)), Some(PitchClass::C));
        assert_eq!(pitch_class_of(note(-12)), Some(PitchClass::A));
        assert_eq!(pitch_class_of(note(-24)), Some(PitchClass::A));
        assert_eq!(pitch_class_of(note(-13)), Some(PitchClass::GSharp));
    }

    #[test]
    fn frequencies_outside_the_useful_range_are_refused() {
        assert_eq!(pitch_class_of(30.0), None);
        assert_eq!(pitch_class_of(8_000.0), None);
        assert_eq!(pitch_class_of(f64::NAN), None);
        assert_eq!(pitch_class_of(0.0), None);
    }
}
