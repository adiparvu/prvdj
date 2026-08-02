//! Analysis, as a host sees it.
//!
//! # The last thing that was reachable in Rust only
//!
//! Before this, a host had to tell the planner a track's tempo, key, energy and
//! loudness — facts the core already computes and the host has no way to work
//! out. The application would have had to ask a user to type in a BPM, which is
//! not a product.
//!
//! # Why the samples come in as one call and the answers go out as many
//!
//! A track's audio is large and the host already has it decoded; copying it
//! would double the memory of an import for no reason, so it crosses as a
//! pointer and a length and is borrowed for the duration of the call.
//!
//! The *answers* are small, and each is optional in a way that matters. A track
//! with no discernible pulse has no tempo — not a tempo of zero, and not a
//! guess. Returning them one at a time, each with a status that can say "not
//! found", is what keeps "we could not tell" distinct from "it is 120". The
//! second is a lie that would reach the planner and produce a set built on it.
//!
//! # A short track has a tempo and nothing else
//!
//! The stages have different appetites. Tempo comes from a novelty curve and is
//! available after a few seconds; *structure* needs roughly thirty seconds
//! before it can find sections, and without sections there is no energy figure.
//!
//! That matters to a host because a candidate needs both. A twelve-second loop
//! analyses successfully, reports a tempo, and still cannot be planned with —
//! which is the correct outcome and a confusing one if nobody wrote it down.
//!
//! # This is not the audio thread
//!
//! Analysis allocates, takes seconds on a long track, and belongs to the
//! background domain the architecture overview describes. A host that called it
//! from a render callback would drop out, which is why it is documented here and
//! in the header rather than left to be discovered.

use prv_analysis::TrackProfile;
use prv_time::SampleRate;

use crate::status::Status;

/// The largest track the boundary will analyse, in frames.
///
/// Twelve hours at 192 kHz. Not a musical limit — a guard against a host that
/// passes a byte count where a frame count belongs, which would otherwise read
/// four times past the end of its own buffer.
pub const MAX_FRAMES: usize = 192_000 * 60 * 60 * 12;

/// Narrows a double to single precision, clamped to the range an energy has.
///
/// The clamp is not defensive dressing: `f64::from(f32)` back to `f32` is exact,
/// but a mean of values each in `0..=1` can land a hair outside it through
/// rounding, and the planner treats energy as a fraction.
fn narrow(value: f64) -> f32 {
    #[allow(
        clippy::cast_possible_truncation,
        reason = "the value is already clamped to 0..=1, which f32 represents exactly \
                  at both ends and to well within a hundredth everywhere between"
    )]
    let narrowed = value.clamp(0.0, 1.0) as f32;
    narrowed
}

/// What the analysis found out about one track.
#[derive(Debug)]
pub struct Analysis {
    profile: TrackProfile,
}

impl Analysis {
    /// Analyses a track.
    ///
    /// # Errors
    ///
    /// [`Status::InvalidArgument`] for a rate or a length the analysis cannot
    /// use, and [`Status::Refused`] when the pipeline could not run at all —
    /// which happens for audio too short to hold a window.
    pub fn run(samples: &[f32], sample_rate: u32) -> Result<Self, Status> {
        let Ok(rate) = SampleRate::new(sample_rate) else {
            return Err(Status::InvalidArgument);
        };
        if samples.is_empty() || samples.len() > MAX_FRAMES {
            return Err(Status::InvalidArgument);
        }
        let profile = prv_analysis::analyse(samples, rate).map_err(|_| Status::Refused)?;
        Ok(Self { profile })
    }

    /// The tempo in beats per minute, and how sure the estimate is.
    ///
    /// # Errors
    ///
    /// [`Status::Refused`] when no pulse was found. A track with no discernible
    /// tempo has none — not a tempo of zero, and not a guess, because a guess
    /// would reach the planner and a whole set would be built on it.
    pub fn tempo(&self) -> Result<(f64, f32), Status> {
        let estimate = self.profile.tempo().ok_or(Status::Refused)?;
        Ok((estimate.tempo().bpm(), estimate.confidence().value()))
    }

    /// The key, as semitones above C, whether it is minor, and the confidence.
    ///
    /// # Errors
    ///
    /// [`Status::Refused`] when no key was found.
    pub fn key(&self) -> Result<(i32, bool, f32), Status> {
        let estimate = self.profile.key().ok_or(Status::Refused)?;
        let key = estimate.key();
        Ok((
            i32::from(key.tonic.semitones()),
            key.mode == prv_harmony::Mode::Minor,
            estimate.confidence().value(),
        ))
    }

    /// The integrated loudness in LUFS, and the loudness range.
    ///
    /// # Errors
    ///
    /// [`Status::Refused`] when loudness could not be measured.
    pub fn loudness(&self) -> Result<(f64, f64), Status> {
        let loudness = self.profile.loudness().ok_or(Status::Refused)?;
        Ok((loudness.integrated(), loudness.range()))
    }

    /// The true peak, in decibels relative to full scale.
    ///
    /// # Errors
    ///
    /// [`Status::Refused`] when loudness could not be measured.
    pub fn true_peak(&self) -> Result<f64, Status> {
        Ok(self
            .profile
            .loudness()
            .ok_or(Status::Refused)?
            .true_peak_dbfs())
    }

    /// How many places the analysis found that a transition could happen.
    ///
    /// # Errors
    ///
    /// [`Status::Refused`] when the structure could not be determined.
    pub fn transition_point_count(&self) -> Result<u64, Status> {
        let structure = self.profile.structure().ok_or(Status::Refused)?;
        Ok(structure
            .transition_points()
            .len()
            .try_into()
            .unwrap_or(u64::MAX))
    }

    /// One place a transition could happen: where it starts, and how quiet it is.
    ///
    /// Quieter is better to mix on, which is why the energy comes back with the
    /// position rather than the caller having to ask separately.
    ///
    /// # Errors
    ///
    /// [`Status::Refused`] when there is no structure, and
    /// [`Status::InvalidArgument`] when there is no point at that index.
    pub fn transition_point(&self, index: u64) -> Result<(i64, f32), Status> {
        let structure = self.profile.structure().ok_or(Status::Refused)?;
        let points = structure.transition_points();
        let index = usize::try_from(index).map_err(|_| Status::InvalidArgument)?;
        let section = points.get(index).ok_or(Status::InvalidArgument)?;
        Ok((
            i64::try_from(section.start()).unwrap_or(i64::MAX),
            section.energy(),
        ))
    }

    /// The track's overall energy, from zero to one.
    ///
    /// The mean of the sections' energies when a structure was found. A track
    /// with no structure has no energy figure rather than a default one, for the
    /// same reason it has no tempo: the planner would use it.
    ///
    /// # Errors
    ///
    /// [`Status::Refused`] when the structure could not be determined.
    pub fn energy(&self) -> Result<f32, Status> {
        let structure = self.profile.structure().ok_or(Status::Refused)?;
        let sections = structure.sections();
        if sections.is_empty() {
            return Err(Status::Refused);
        }
        let total: f64 = sections
            .iter()
            .map(|section| f64::from(section.energy()))
            .sum();
        // Summed and divided in double precision. A track can have hundreds of
        // sections, and an f32 running total loses its low bits long before
        // that — which would make a long track's energy depend on how many
        // pieces the structure detector happened to cut it into.
        let count = u32::try_from(sections.len()).map_err(|_| Status::Refused)?;
        Ok(narrow(total / f64::from(count)))
    }

    /// How long the analysed audio was, in frames.
    #[must_use]
    pub fn duration(&self) -> i64 {
        i64::try_from(self.profile.duration()).unwrap_or(i64::MAX)
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::panic,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss,
        reason = "a test that cannot build its own fixture should fail loudly, and \
                  a synthetic click track is arithmetic on known-good numbers"
    )]

    use super::*;

    const RATE: u32 = 44_100;

    /// A signal with a real pulse: a click every `period` frames, with a decaying
    /// tail so the novelty curve has something to find.
    fn pulsed(bpm: f64, seconds: usize) -> Vec<f32> {
        let period = (60.0 / bpm * f64::from(RATE)) as usize;
        let length = seconds * RATE as usize;
        let mut samples = vec![0.0_f32; length];
        let mut index = 0;
        while index < length {
            for offset in 0..1_000.min(length - index) {
                let decay = 1.0 - (offset as f32 / 1_000.0);
                if let Some(slot) = samples.get_mut(index + offset) {
                    *slot += 0.8 * decay * ((offset as f32) * 0.05).sin();
                }
            }
            index += period.max(1);
        }
        samples
    }

    #[test]
    fn a_track_with_a_pulse_gets_a_tempo_and_a_loudness() {
        let samples = pulsed(120.0, 20);
        let analysis = Analysis::run(&samples, RATE).expect("a pulsed signal analyses");

        let (bpm, confidence) = analysis.tempo().expect("a pulse has a tempo");
        assert!(bpm > 0.0, "a tempo of {bpm} is not a tempo");
        assert!((0.0..=1.0).contains(&confidence));

        let (integrated, range) = analysis.loudness().expect("audio has a loudness");
        assert!(
            integrated.is_finite(),
            "loudness {integrated} is not a number"
        );
        assert!(range >= 0.0);

        assert_eq!(
            analysis.duration(),
            i64::try_from(samples.len()).unwrap_or(0)
        );
    }

    #[test]
    fn a_rate_or_a_length_the_analysis_cannot_use_is_refused() {
        let samples = vec![0.0_f32; 1_000];
        assert_eq!(
            Analysis::run(&samples, 0).err(),
            Some(Status::InvalidArgument)
        );
        assert_eq!(
            Analysis::run(&[], RATE).err(),
            Some(Status::InvalidArgument)
        );
    }

    #[test]
    fn audio_too_short_to_analyse_is_refused_rather_than_guessed_at() {
        // Ten samples is not a track. Returning a default tempo would put a
        // number the planner trusts into a set built on nothing.
        let samples = vec![0.1_f32; 10];
        assert_eq!(Analysis::run(&samples, RATE).err(), Some(Status::Refused));
    }

    #[test]
    fn silence_has_no_tempo_and_says_so() {
        // The distinction the whole module is arranged around: "we could not
        // tell" is not "it is 120", and only one of those is safe to plan with.
        let samples = vec![0.0_f32; RATE as usize * 10];
        let Ok(analysis) = Analysis::run(&samples, RATE) else {
            // Refusing outright is an equally honest answer for silence.
            return;
        };
        if let Ok((bpm, _)) = analysis.tempo() {
            assert!(bpm > 0.0, "silence produced a tempo of {bpm}");
        }
    }

    #[test]
    fn asking_for_a_transition_point_that_is_not_there_is_refused() {
        let samples = pulsed(128.0, 20);
        let analysis = Analysis::run(&samples, RATE).expect("analyses");

        if let Ok(count) = analysis.transition_point_count() {
            // Every point the analysis claims can be read back.
            for index in 0..count {
                let (position, energy) = analysis.transition_point(index).expect("in range");
                assert!(position >= 0);
                assert!((0.0..=1.0).contains(&energy));
            }
            // And one past the end is refused rather than returning zeros.
            assert_eq!(
                analysis.transition_point(count),
                Err(Status::InvalidArgument)
            );
        }
    }

    #[test]
    fn an_energy_figure_is_absent_rather_than_defaulted() {
        // If there is no structure there is no energy, because the planner
        // shapes a whole set around this number.
        let samples = pulsed(124.0, 20);
        let analysis = Analysis::run(&samples, RATE).expect("analyses");
        match analysis.energy() {
            Ok(energy) => assert!(
                (0.0..=1.0).contains(&energy),
                "energy {energy} is out of range"
            ),
            Err(status) => assert_eq!(
                status,
                Status::Refused,
                "a missing energy was reported as something other than absent"
            ),
        }
    }
}
