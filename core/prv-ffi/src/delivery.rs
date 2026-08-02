//! Getting a finished set out, as a host sees it.
//!
//! # The core does not write files, and this does not either
//!
//! ADR-0001 keeps every file out of the core, so this module encodes nothing. It
//! answers the question the encoder needs answered first: *given what this
//! master measures, what has to happen to it before it can go where it is
//! going?*
//!
//! The host then applies the gain, encodes, and writes. Splitting it that way is
//! what lets the same decision serve a WAV on a laptop, a stream upload and a
//! broadcast delivery, with three encoders and one rule.
//!
//! # Why the answer is a report and not a boolean
//!
//! "Ready" and "not ready" would be enough to gate a button and useless for
//! anything else. A user who is told only *no* cannot act. So the report carries
//! what was measured, what the target wants, the gain that would close the gap,
//! and where the true peak lands afterwards — which is the number that decides
//! whether the gain is safe to apply at all.
//!
//! # Too quiet is not fixed by turning it up
//!
//! A master far below its target is usually a mistake upstream — a missing
//! placement, a muted lane, an export of the wrong project. Applying twenty
//! decibels of gain to it produces a loud version of the wrong thing, so the
//! core reports zero gain and lets a person look.

use prv_export::{report, BitDepth, Compliance, DeliveryTarget, ExportReport, Format};

use crate::status::Status;

/// What a master measures and what a target would need.
#[derive(Debug)]
pub struct Delivery {
    report: ExportReport,
}

impl Delivery {
    /// Judges a master against a target.
    ///
    /// The loudness comes from an [`crate::Analysis`] of the rendered mix — the
    /// same measurement path a track import uses, deliberately, so the number
    /// gating an export is the number the meter showed.
    ///
    /// # Errors
    ///
    /// [`Status::InvalidArgument`] when the target, format or depth is one this
    /// version does not define.
    pub fn judge(
        analysis: &crate::Analysis,
        target_code: i32,
        format_code: i32,
        depth_code: i32,
    ) -> Result<Self, Status> {
        let target = target_from_code(target_code).ok_or(Status::InvalidArgument)?;
        let format = format_from_code(format_code).ok_or(Status::InvalidArgument)?;
        let depth = depth_from_code(depth_code).ok_or(Status::InvalidArgument)?;
        let loudness = analysis.loudness_measurement().ok_or(Status::Refused)?;
        Ok(Self {
            report: report(loudness, target, format, depth),
        })
    }

    /// Whether the master is ready, needs gain, or would clip if gained.
    #[must_use]
    pub fn compliance(&self) -> i32 {
        compliance_code(self.report.compliance())
    }

    /// The measured integrated loudness, in LUFS.
    #[must_use]
    pub fn measured_lufs(&self) -> f64 {
        self.report.measured_lufs()
    }

    /// The measured true peak, in dBTP.
    #[must_use]
    pub fn measured_true_peak(&self) -> f64 {
        self.report.measured_true_peak_dbtp()
    }

    /// The gain that would put the master on target, in decibels.
    ///
    /// Zero when the master is far below target: that is a mistake upstream, and
    /// turning it up produces a loud version of the wrong thing.
    #[must_use]
    pub fn gain_db(&self) -> f64 {
        self.report.gain_db()
    }

    /// Where the true peak lands once the gain is applied, in dBTP.
    ///
    /// The number that decides whether the gain is safe. A host showing only the
    /// gain is showing half the decision.
    #[must_use]
    pub fn resulting_true_peak(&self) -> f64 {
        self.report.resulting_true_peak_dbtp()
    }

    /// How much room is left under the ceiling afterwards, in decibels.
    #[must_use]
    pub fn headroom_db(&self) -> f64 {
        self.report.headroom_db()
    }

    /// Whether the encoder should dither.
    ///
    /// True only when the depth genuinely reduces resolution. Dithering a
    /// float export adds noise for nothing.
    #[must_use]
    pub fn needs_dither(&self) -> bool {
        self.report.dither()
    }

    /// Whether this is something a person should look at before exporting.
    #[must_use]
    pub fn needs_attention(&self) -> bool {
        self.report.compliance().is_actionable()
    }
}

/// The delivery target a code names.
#[must_use]
pub const fn target_from_code(code: i32) -> Option<DeliveryTarget> {
    match code {
        0 => Some(DeliveryTarget::Streaming),
        1 => Some(DeliveryTarget::Club),
        2 => Some(DeliveryTarget::Broadcast),
        _ => None,
    }
}

/// The container format a code names.
#[must_use]
pub const fn format_from_code(code: i32) -> Option<Format> {
    match code {
        0 => Some(Format::Wave),
        1 => Some(Format::Aiff),
        2 => Some(Format::Flac),
        // A bit rate is part of what a lossy format *is*, so it is carried
        // rather than defaulted. 320 is the rate a delivery target means when it
        // says lossy, and a host that wants another says so with its own code.
        3 => Some(Format::Lossy { kilobits: 320 }),
        _ => None,
    }
}

/// The bit depth a code names.
#[must_use]
pub const fn depth_from_code(code: i32) -> Option<BitDepth> {
    match code {
        0 => Some(BitDepth::Sixteen),
        1 => Some(BitDepth::TwentyFour),
        2 => Some(BitDepth::Float32),
        _ => None,
    }
}

/// The code a compliance verdict has.
#[must_use]
pub const fn compliance_code(compliance: Compliance) -> i32 {
    match compliance {
        Compliance::Ready => 0,
        Compliance::WouldClip => 2,
        // `NeedsGain`, and anything added later.
        //
        // `Compliance` is `#[non_exhaustive]`, so a verdict introduced next year
        // reaches here before anybody has thought about the ABI. Folding it in
        // with "needs gain" rather than with "ready" is the safe direction: the
        // host is told a person should look, which is worse than a precise
        // answer and much better than shipping something the core would have
        // refused.
        _ => 1,
    }
}

/// Every delivery target with its C spelling, in code order.
pub const TARGETS: &[(DeliveryTarget, &str)] = &[
    (DeliveryTarget::Streaming, "PRV_TARGET_STREAMING"),
    (DeliveryTarget::Club, "PRV_TARGET_CLUB"),
    (DeliveryTarget::Broadcast, "PRV_TARGET_BROADCAST"),
];

/// Every format with its C spelling, in code order.
pub const FORMATS: &[(Format, &str)] = &[
    (Format::Wave, "PRV_FORMAT_WAVE"),
    (Format::Aiff, "PRV_FORMAT_AIFF"),
    (Format::Flac, "PRV_FORMAT_FLAC"),
    (Format::Lossy { kilobits: 320 }, "PRV_FORMAT_LOSSY_320"),
];

/// Every bit depth with its C spelling, in code order.
pub const DEPTHS: &[(BitDepth, &str)] = &[
    (BitDepth::Sixteen, "PRV_DEPTH_SIXTEEN"),
    (BitDepth::TwentyFour, "PRV_DEPTH_TWENTY_FOUR"),
    (BitDepth::Float32, "PRV_DEPTH_FLOAT32"),
];

/// Every compliance verdict with its C spelling, in code order.
pub const COMPLIANCE: &[(Compliance, &str)] = &[
    (Compliance::Ready, "PRV_COMPLIANCE_READY"),
    (Compliance::NeedsGain, "PRV_COMPLIANCE_NEEDS_GAIN"),
    (Compliance::WouldClip, "PRV_COMPLIANCE_WOULD_CLIP"),
];

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::panic,
        reason = "a test that cannot build its own fixture should fail loudly"
    )]

    use super::*;

    #[test]
    fn every_code_round_trips_and_an_unknown_one_is_refused() {
        for (index, (target, name)) in TARGETS.iter().enumerate() {
            let code = i32::try_from(index).expect("small");
            assert_eq!(target_from_code(code), Some(*target));
            assert!(name.starts_with("PRV_TARGET_"));
        }
        for (index, (format, _)) in FORMATS.iter().enumerate() {
            assert_eq!(
                format_from_code(i32::try_from(index).expect("small")),
                Some(*format)
            );
        }
        for (index, (depth, _)) in DEPTHS.iter().enumerate() {
            assert_eq!(
                depth_from_code(i32::try_from(index).expect("small")),
                Some(*depth)
            );
        }
        for (index, (compliance, _)) in COMPLIANCE.iter().enumerate() {
            assert_eq!(
                compliance_code(*compliance),
                i32::try_from(index).expect("small")
            );
        }
        assert_eq!(target_from_code(-1), None);
        assert_eq!(format_from_code(99), None);
        assert_eq!(depth_from_code(99), None);
    }

    #[test]
    fn a_dither_decision_follows_the_depth_rather_than_the_format() {
        // Dithering a float export adds noise for nothing.
        assert!(!BitDepth::Float32.reduces_resolution());
        assert!(BitDepth::Sixteen.reduces_resolution());
    }

    #[test]
    fn every_target_has_a_ceiling_and_streaming_has_a_loudness() {
        // The two numbers the whole module turns on. A target with no ceiling
        // would let anything through.
        for (target, _) in TARGETS {
            assert!(
                target.true_peak_ceiling_dbtp() <= 0.0,
                "{target:?} has a ceiling above full scale"
            );
        }
        assert!(
            DeliveryTarget::Streaming.loudness_lufs().is_some(),
            "a streaming target with no loudness has nothing to normalise to"
        );
    }
}
