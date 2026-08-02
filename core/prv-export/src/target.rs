//! Where a mix is going, and what that place requires of it.
//!
//! # Why a target rather than a settings screen
//!
//! Master Prompt #3A requires the master chain to report loudness, and Master
//! Prompt #3C requires an export to come with a report the user can act on.
//! Both need something to report *against*, and the honest form of that is the
//! actual requirement of the actual destination: a streaming platform normalises
//! to a published figure, a club wants headroom for a system that is already
//! loud, an archive wants nothing done to it at all.
//!
//! Expressing that as a target rather than as a pair of numbers in a preferences
//! screen buys two things. The user picks a destination they understand instead
//! of a figure they have to look up; and when a platform changes its figure, one
//! constant changes here rather than every project that was set up under the old
//! one.
//!
//! # The published figures are recorded, not invented
//!
//! Each target's numbers come from the destination's own published
//! specification, and each carries the reason it is what it is. They are
//! reviewed rather than assumed permanent — platforms change them — which is
//! why they sit in one file with a comment each rather than scattered through
//! the code that uses them.

use core::fmt;

/// Where an export is going.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum DeliveryTarget {
    /// A streaming platform that normalises loudness.
    ///
    /// −14 LUFS with a true-peak ceiling of −1 dBTP. The ceiling is the part
    /// that matters and the part most often missed: a platform re-encodes to a
    /// lossy format, and a lossy encoder's output overshoots its input. A file
    /// that peaks at 0 dBTP arrives at the listener clipping, on some players
    /// and not others, which makes it the kind of defect that is reported from
    /// the field and cannot be reproduced.
    Streaming,

    /// A club or festival system.
    ///
    /// −9 LUFS, which is where contemporary dance masters sit, and −1 dBTP for
    /// the same reason. Louder than streaming because nothing downstream will
    /// normalise it and the set has to hold its own against the record before it.
    Club,

    /// A podcast or radio delivery.
    ///
    /// −16 LUFS, the figure spoken-word delivery has settled on. Quieter than
    /// music because a mix that is level-matched to speech is a mix that does
    /// not make a listener reach for the volume between segments.
    Broadcast,

    /// An archive copy.
    ///
    /// No loudness target: an archive is the thing you go back to, and
    /// normalising it destroys the information that would let you normalise it
    /// differently later. The true-peak ceiling is 0 dBTP, which is not a target
    /// but a statement that the file must not already be clipping.
    Archive,
}

impl DeliveryTarget {
    /// Every target, so a caller offering a choice cannot miss one.
    pub const ALL: [Self; 4] = [Self::Streaming, Self::Club, Self::Broadcast, Self::Archive];

    /// The loudness this target expects, in LUFS, if it expects one.
    #[must_use]
    pub const fn loudness_lufs(self) -> Option<f64> {
        match self {
            Self::Streaming => Some(-14.0),
            Self::Club => Some(-9.0),
            Self::Broadcast => Some(-16.0),
            Self::Archive => None,
        }
    }

    /// The highest true peak this target permits, in dBTP.
    #[must_use]
    pub const fn true_peak_ceiling_dbtp(self) -> f64 {
        match self {
            Self::Streaming | Self::Club | Self::Broadcast => -1.0,
            Self::Archive => 0.0,
        }
    }

    /// How far from the loudness target is close enough.
    ///
    /// Half a loudness unit. Below that the difference is inaudible in a
    /// comparison and well inside the spread between one measurement tool and
    /// another; reporting a project as non-compliant over a tenth of a unit
    /// would train the user to ignore the report.
    #[must_use]
    pub const fn loudness_tolerance(self) -> f64 {
        0.5
    }

    /// A stable identifier, for storage and localisation.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Streaming => "target.streaming",
            Self::Club => "target.club",
            Self::Broadcast => "target.broadcast",
            Self::Archive => "target.archive",
        }
    }
}

impl fmt::Display for DeliveryTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.key())
    }
}

/// The container and encoding an export is written as.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Format {
    /// Uncompressed, in a WAVE container.
    Wave,
    /// Uncompressed, in an AIFF container.
    Aiff,
    /// Losslessly compressed.
    Flac,
    /// Lossy, at a stated bit rate in kilobits per second.
    Lossy {
        /// The bit rate.
        kilobits: u32,
    },
}

impl Format {
    /// Whether the format preserves the samples exactly.
    #[must_use]
    pub const fn is_lossless(self) -> bool {
        matches!(self, Self::Wave | Self::Aiff | Self::Flac)
    }

    /// A stable identifier.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Wave => "format.wave",
            Self::Aiff => "format.aiff",
            Self::Flac => "format.flac",
            Self::Lossy { .. } => "format.lossy",
        }
    }
}

/// How many bits each sample is written with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum BitDepth {
    /// Sixteen bits. The compact-disc depth, and what a lossy encoder is fed.
    Sixteen,
    /// Twenty-four bits. The delivery depth for anything that will be worked on.
    TwentyFour,
    /// Thirty-two bit floating point.
    ///
    /// Not a *depth* in the same sense: it carries values above full scale
    /// without clipping them, which is what makes it the right choice for a
    /// file that is going back into a session and the wrong one for a file
    /// going to a listener.
    Float32,
}

impl BitDepth {
    /// Whether writing at this depth discards information from the mix.
    ///
    /// This is what decides whether dither is needed, and it is a property of
    /// the depth rather than a setting: the engine works in floating point, so
    /// anything narrower than that is a reduction.
    #[must_use]
    pub const fn reduces_resolution(self) -> bool {
        matches!(self, Self::Sixteen | Self::TwentyFour)
    }

    /// A stable identifier.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Sixteen => "depth.16",
            Self::TwentyFour => "depth.24",
            Self::Float32 => "depth.float32",
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::float_cmp,
        reason = "these are published constants compared exactly"
    )]

    use super::*;

    #[test]
    fn every_target_that_normalises_has_a_ceiling_below_full_scale() {
        // The part most often missed. A platform re-encodes to a lossy format,
        // and a lossy encoder's output overshoots its input — so a file that
        // peaks at full scale arrives at the listener clipping, on some players
        // and not others.
        for target in DeliveryTarget::ALL {
            if target.loudness_lufs().is_some() {
                assert!(
                    target.true_peak_ceiling_dbtp() <= -1.0,
                    "{target} normalises but permits peaks up to {}",
                    target.true_peak_ceiling_dbtp()
                );
            }
        }
    }

    #[test]
    fn an_archive_is_not_normalised() {
        // An archive is the thing you go back to. Normalising it destroys the
        // information that would let you normalise it differently later.
        assert_eq!(DeliveryTarget::Archive.loudness_lufs(), None);
        assert_eq!(DeliveryTarget::Archive.true_peak_ceiling_dbtp(), 0.0);
    }

    #[test]
    fn the_targets_are_ordered_the_way_the_world_is() {
        // A club master is louder than a streaming one, which is louder than a
        // spoken-word one. If this ordering were wrong the reports would be
        // confidently misleading in a way nothing else would catch.
        let club = DeliveryTarget::Club.loudness_lufs().unwrap_or(0.0);
        let streaming = DeliveryTarget::Streaming.loudness_lufs().unwrap_or(0.0);
        let broadcast = DeliveryTarget::Broadcast.loudness_lufs().unwrap_or(0.0);
        assert!(club > streaming);
        assert!(streaming > broadcast);
    }

    #[test]
    fn only_the_depths_narrower_than_the_engine_reduce_resolution() {
        // What decides whether dither is needed. The engine works in floating
        // point, so anything narrower is a reduction — and float is not.
        assert!(BitDepth::Sixteen.reduces_resolution());
        assert!(BitDepth::TwentyFour.reduces_resolution());
        assert!(!BitDepth::Float32.reduces_resolution());
    }

    #[test]
    fn lossless_formats_are_marked_as_such() {
        assert!(Format::Wave.is_lossless());
        assert!(Format::Aiff.is_lossless());
        assert!(Format::Flac.is_lossless());
        assert!(!Format::Lossy { kilobits: 320 }.is_lossless());
    }

    #[test]
    fn keys_are_distinct_within_each_kind() {
        let targets: Vec<&str> = DeliveryTarget::ALL.iter().map(|t| t.key()).collect();
        for (index, key) in targets.iter().enumerate() {
            for (other, value) in targets.iter().enumerate() {
                assert!(index == other || key != value, "two targets share {key}");
            }
        }

        let depths = [
            BitDepth::Sixteen.key(),
            BitDepth::TwentyFour.key(),
            BitDepth::Float32.key(),
        ];
        for (index, key) in depths.iter().enumerate() {
            for (other, value) in depths.iter().enumerate() {
                assert!(index == other || key != value, "two depths share {key}");
            }
        }
    }
}
