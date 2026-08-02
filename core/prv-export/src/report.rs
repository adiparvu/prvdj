//! What an export would be, checked against where it is going.
//!
//! # The report is produced before the render, not after
//!
//! Master Prompt #3C requires an export to come with a report the user can act
//! on. The useful moment for that is *before* the file is written: a
//! forty-minute set takes minutes to render, and telling someone afterwards
//! that it peaks two decibels too high has wasted both the time and the file.
//!
//! Everything here is computed from the mix's measured loudness, which
//! `prv-analysis` already produces. Nothing needs the render.
//!
//! # Nothing is applied silently
//!
//! The report says what gain would bring the mix to its target and whether the
//! peaks would survive it. It does not apply either. Master Prompt #3A puts the
//! master chain under the user's control, and a system that quietly normalised
//! an export would be making a mastering decision on their behalf and hiding it
//! in a file they will hand to someone else.

use prv_analysis::loudness::Loudness;

use crate::target::{BitDepth, DeliveryTarget, Format};

/// How an export stands against its destination's requirements.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Compliance {
    /// Within the destination's requirements as it stands.
    Ready,

    /// Would meet them after the gain the report names.
    ///
    /// The ordinary outcome. Almost no mix is delivered at exactly its target,
    /// and moving it there is one number.
    NeedsGain,

    /// The peaks would exceed the ceiling after that gain.
    ///
    /// A separate outcome from `NeedsGain` because the remedy is different and
    /// the user has to choose it: limit the master, or deliver quieter than the
    /// target. Neither can be picked on their behalf without making a mastering
    /// decision they did not ask for.
    WouldClip,

    /// The mix is too quiet for its measured loudness to mean anything.
    ///
    /// Silence, or a fade that never arrived. Reported rather than normalised,
    /// because the gain that would bring digital silence to −14 LUFS is not a
    /// number anyone wants applied.
    TooQuiet,
}

impl Compliance {
    /// Whether an export can proceed without the user deciding something first.
    #[must_use]
    pub const fn is_actionable(self) -> bool {
        matches!(self, Self::Ready | Self::NeedsGain)
    }

    /// A stable identifier, for storage and localisation.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Ready => "compliance.ready",
            Self::NeedsGain => "compliance.needs_gain",
            Self::WouldClip => "compliance.would_clip",
            Self::TooQuiet => "compliance.too_quiet",
        }
    }
}

/// The loudness below which a measurement is not describing music.
///
/// −40 LUFS. Quieter than the quietest passage of the most dynamic recording
/// anyone delivers, and far above the floor `prv-analysis` reports for silence,
/// so the two cannot be confused.
const TOO_QUIET_LUFS: f64 = -40.0;

/// What an export would be, and whether it is allowed.
#[derive(Debug, Clone, PartialEq)]
pub struct ExportReport {
    target: DeliveryTarget,
    format: Format,
    depth: BitDepth,
    measured_lufs: f64,
    measured_true_peak: f64,
    gain_db: f64,
    resulting_true_peak: f64,
    compliance: Compliance,
    dither: bool,
}

impl ExportReport {
    /// Where the export is going.
    #[must_use]
    pub const fn target(&self) -> DeliveryTarget {
        self.target
    }

    /// The format it would be written in.
    #[must_use]
    pub const fn format(&self) -> Format {
        self.format
    }

    /// The depth it would be written at.
    #[must_use]
    pub const fn depth(&self) -> BitDepth {
        self.depth
    }

    /// The mix's measured loudness, in LUFS.
    #[must_use]
    pub const fn measured_lufs(&self) -> f64 {
        self.measured_lufs
    }

    /// The mix's measured true peak, in dBTP.
    #[must_use]
    pub const fn measured_true_peak_dbtp(&self) -> f64 {
        self.measured_true_peak
    }

    /// The gain that would bring the mix to its target, in decibels.
    ///
    /// Zero when the target does not normalise. Reported rather than applied:
    /// Master Prompt #3A puts the master chain under the user's control, and a
    /// system that quietly normalised an export would be making a mastering
    /// decision on their behalf and hiding it in a file they will hand to
    /// someone else.
    #[must_use]
    pub const fn gain_db(&self) -> f64 {
        self.gain_db
    }

    /// The true peak the export would have after that gain, in dBTP.
    #[must_use]
    pub const fn resulting_true_peak_dbtp(&self) -> f64 {
        self.resulting_true_peak
    }

    /// How much room is left under the ceiling, in decibels.
    ///
    /// Negative when the export would exceed it. Exposed as a number rather
    /// than only as a verdict because "0.3 dB over" and "6 dB over" call for
    /// entirely different responses.
    #[must_use]
    pub fn headroom_db(&self) -> f64 {
        self.target.true_peak_ceiling_dbtp() - self.resulting_true_peak
    }

    /// The verdict.
    #[must_use]
    pub const fn compliance(&self) -> Compliance {
        self.compliance
    }

    /// Whether dither should be applied when writing.
    ///
    /// True only when the depth actually discards resolution *and* the format
    /// keeps what is written. Dithering into a lossy encoder adds noise the
    /// encoder then spends bits describing, which is worse than the truncation
    /// it was meant to mask; dithering a floating-point file adds noise to a
    /// file that lost nothing.
    ///
    /// This is the kind of setting that is usually a checkbox and usually wrong.
    /// Deriving it means it is right by default and can still be overridden by
    /// something above this layer that knows better.
    #[must_use]
    pub const fn dither(&self) -> bool {
        self.dither
    }
}

/// Checks what an export would be against where it is going.
#[must_use]
pub fn report(
    loudness: &Loudness,
    target: DeliveryTarget,
    format: Format,
    depth: BitDepth,
) -> ExportReport {
    let measured_lufs = loudness.integrated();
    let measured_true_peak = loudness.true_peak_dbfs();

    let too_quiet = measured_lufs <= TOO_QUIET_LUFS;
    let gain_db = if too_quiet {
        0.0
    } else {
        target
            .loudness_lufs()
            .map_or(0.0, |wanted| wanted - measured_lufs)
    };

    // A gain in decibels moves the peak by exactly the same number of decibels,
    // which is the one part of this that needs no measurement.
    let resulting_true_peak = measured_true_peak + gain_db;
    let ceiling = target.true_peak_ceiling_dbtp();

    let compliance = if too_quiet {
        Compliance::TooQuiet
    } else if resulting_true_peak > ceiling {
        Compliance::WouldClip
    } else if gain_db.abs() <= target.loudness_tolerance() {
        Compliance::Ready
    } else {
        Compliance::NeedsGain
    };

    ExportReport {
        target,
        format,
        depth,
        measured_lufs,
        measured_true_peak,
        gain_db,
        resulting_true_peak,
        compliance,
        dither: depth.reduces_resolution() && format.is_lossless(),
    }
}

/// The gain that would bring an export within its ceiling, in decibels.
///
/// The answer to "deliver quieter rather than limit", which is one of the two
/// choices a [`Compliance::WouldClip`] report puts in front of the user. It is
/// computed rather than left to them because the arithmetic is easy to get
/// wrong by a sign and the consequence is a file that clips anyway.
#[must_use]
pub fn gain_to_fit_ceiling(report: &ExportReport) -> f64 {
    let ceiling = report.target.true_peak_ceiling_dbtp();
    if report.resulting_true_peak <= ceiling {
        return report.gain_db;
    }
    report.gain_db - (report.resulting_true_peak - ceiling)
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
    use prv_analysis::loudness::measure;
    use prv_time::SampleRate;

    const RATE: SampleRate = SampleRate::HZ_48000;

    /// A tone at a chosen amplitude, long enough to measure.
    fn mix(amplitude: f64) -> Loudness {
        let step = core::f64::consts::TAU * 1_000.0 / f64::from(RATE.hz());
        let samples: Vec<f32> = (0..(48_000 * 5))
            .map(|n| {
                #[allow(
                    clippy::cast_possible_truncation,
                    reason = "a test signal, generated in exact bounded quantities"
                )]
                {
                    (amplitude * (step * f64::from(n)).sin()) as f32
                }
            })
            .collect();
        measure(&samples, RATE).expect("five seconds is enough")
    }

    /// A quiet mix with brief full-scale transients: the shape that clips when
    /// it is normalised up.
    fn dynamic_mix() -> Loudness {
        let step = core::f64::consts::TAU * 1_000.0 / f64::from(RATE.hz());
        let mut samples: Vec<f32> = (0..(48_000 * 5))
            .map(|n| {
                #[allow(
                    clippy::cast_possible_truncation,
                    reason = "a test signal, generated in exact bounded quantities"
                )]
                {
                    (0.05 * (step * f64::from(n)).sin()) as f32
                }
            })
            .collect();

        // A two-millisecond full-scale burst each second. Short enough that the
        // gated loudness barely moves, tall enough that normalising the mix up
        // by fifteen decibels takes it far over the ceiling.
        for second in 0..5 {
            let start = second * 48_000;
            for offset in 0..96 {
                if let Some(slot) = samples.get_mut(start + offset) {
                    *slot = if offset % 2 == 0 { 0.95 } else { -0.95 };
                }
            }
        }
        measure(&samples, RATE).expect("five seconds is enough")
    }

    #[test]
    fn a_mix_already_at_its_target_is_ready() {
        // A full-scale sine reads −3.01 LUFS, so −14 LUFS is 10.99 dB below
        // full scale — an amplitude of 0.282, not the 0.2 a first guess
        // suggests.
        let loudness = mix(0.282);
        let checked = report(
            &loudness,
            DeliveryTarget::Streaming,
            Format::Flac,
            BitDepth::TwentyFour,
        );
        assert_eq!(checked.compliance(), Compliance::Ready);
        assert!(checked.gain_db().abs() <= DeliveryTarget::Streaming.loudness_tolerance());
        assert!(checked.compliance().is_actionable());
    }

    #[test]
    fn a_quiet_mix_needs_gain_and_the_gain_is_the_difference() {
        let loudness = mix(0.05);
        let checked = report(
            &loudness,
            DeliveryTarget::Streaming,
            Format::Flac,
            BitDepth::TwentyFour,
        );
        assert_eq!(checked.compliance(), Compliance::NeedsGain);
        assert!(checked.gain_db() > 0.0);
        assert!(
            (checked.measured_lufs() + checked.gain_db() - (-14.0)).abs() < 1e-9,
            "applying the reported gain does not reach the target"
        );
    }

    #[test]
    fn a_dynamic_mix_that_would_clip_is_a_different_verdict_from_one_that_needs_gain() {
        // The signal this actually happens to is *quiet with tall peaks*, not
        // simply loud: a loud mix going to a quieter target gets turned down
        // and cannot clip. A dynamic one measures quiet, is normalised
        // upward, and its peaks go over.
        //
        // The remedy is different from a plain gain and the user has to choose
        // it — limit the master, or deliver quieter — and neither can be picked
        // on their behalf without making a mastering decision they did not ask
        // for.
        let loudness = dynamic_mix();
        let checked = report(
            &loudness,
            DeliveryTarget::Streaming,
            Format::Wave,
            BitDepth::TwentyFour,
        );

        assert_eq!(checked.compliance(), Compliance::WouldClip);
        assert!(
            !checked.compliance().is_actionable(),
            "an export that would clip should not proceed without a decision"
        );
        assert!(
            checked.headroom_db() < 0.0,
            "headroom should be negative when the ceiling is exceeded"
        );

        // And the alternative to limiting is computed rather than left to the
        // user, because the arithmetic is easy to get wrong by a sign.
        let quieter = gain_to_fit_ceiling(&checked);
        assert!(quieter < checked.gain_db());
        assert!(
            (checked.measured_true_peak_dbtp() + quieter
                - DeliveryTarget::Streaming.true_peak_ceiling_dbtp())
            .abs()
                < 1e-9,
            "the fitted gain does not land on the ceiling"
        );
    }

    #[test]
    fn silence_is_reported_rather_than_normalised() {
        // The gain that would bring digital silence to −14 LUFS is not a number
        // anyone wants applied.
        let loudness = measure(&vec![0.0_f32; 48_000 * 5], RATE).expect("valid");
        let checked = report(
            &loudness,
            DeliveryTarget::Streaming,
            Format::Flac,
            BitDepth::TwentyFour,
        );
        assert_eq!(checked.compliance(), Compliance::TooQuiet);
        assert_eq!(checked.gain_db(), 0.0);
        assert!(!checked.compliance().is_actionable());
    }

    #[test]
    fn an_archive_is_never_gained_and_only_has_to_not_be_clipping() {
        let loudness = mix(0.05);
        let checked = report(
            &loudness,
            DeliveryTarget::Archive,
            Format::Wave,
            BitDepth::Float32,
        );
        assert_eq!(checked.gain_db(), 0.0, "an archive was normalised");
        assert_eq!(checked.compliance(), Compliance::Ready);
    }

    #[test]
    fn dither_is_derived_and_is_right_in_all_four_cases() {
        // The kind of setting that is usually a checkbox and usually wrong.
        let loudness = mix(0.282);
        let dither_for = |format: Format, depth: BitDepth| {
            report(&loudness, DeliveryTarget::Streaming, format, depth).dither()
        };

        // Reducing resolution into a format that keeps what is written.
        assert!(dither_for(Format::Wave, BitDepth::Sixteen));
        assert!(dither_for(Format::Flac, BitDepth::TwentyFour));

        // Floating point loses nothing, so there is nothing to mask.
        assert!(!dither_for(Format::Wave, BitDepth::Float32));

        // Dithering into a lossy encoder adds noise the encoder then spends
        // bits describing — worse than the truncation it was meant to mask.
        assert!(!dither_for(
            Format::Lossy { kilobits: 320 },
            BitDepth::Sixteen
        ));
    }

    #[test]
    fn the_report_says_what_it_measured_as_well_as_what_it_concluded() {
        // A verdict with no numbers behind it is a verdict a user cannot check,
        // and "0.3 dB over" and "6 dB over" call for entirely different
        // responses.
        let loudness = mix(0.5);
        let checked = report(
            &loudness,
            DeliveryTarget::Streaming,
            Format::Flac,
            BitDepth::TwentyFour,
        );
        assert_eq!(checked.measured_lufs(), loudness.integrated());
        assert_eq!(checked.measured_true_peak_dbtp(), loudness.true_peak_dbfs());
        assert_eq!(checked.target(), DeliveryTarget::Streaming);
        assert_eq!(checked.format(), Format::Flac);
        assert_eq!(checked.depth(), BitDepth::TwentyFour);
        assert!(
            (checked.resulting_true_peak_dbtp()
                - (checked.measured_true_peak_dbtp() + checked.gain_db()))
            .abs()
                < 1e-9,
            "the resulting peak does not follow from the measured one and the gain"
        );
    }

    #[test]
    fn compliance_keys_are_distinct() {
        let keys = [
            Compliance::Ready.key(),
            Compliance::NeedsGain.key(),
            Compliance::WouldClip.key(),
            Compliance::TooQuiet.key(),
        ];
        for (index, key) in keys.iter().enumerate() {
            for (other, value) in keys.iter().enumerate() {
                assert!(index == other || key != value, "two verdicts share {key}");
            }
        }
    }
}
