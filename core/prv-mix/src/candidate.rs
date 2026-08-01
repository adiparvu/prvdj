//! What the planner knows about a track.
//!
//! # A projection, not the analysis
//!
//! The planner could take a `prv-analysis::TrackProfile` directly. It does not,
//! for the same reason the library holds an `AnalysisFacts` projection rather
//! than a copy of the analysis: the planner needs a handful of values per track
//! and searches over thousands of candidate orderings, so it wants a small,
//! flat, `Copy`-cheap record it can compare millions of times.
//!
//! Assembling a [`Candidate`] from a profile is the caller's job, and that is
//! deliberate. It is the one place where "which key do we use when the detector
//! offered two" and "which sections count as entry points" get decided, and
//! those are product decisions rather than search decisions.
//!
//! # Missing facts are represented, not defaulted
//!
//! A track whose key was not detected has `None`, not C major. The constraint
//! model treats an unknown key as *unconstrained rather than compatible*, which
//! is the only safe reading: defaulting would let the planner build a set on a
//! harmonic relationship it invented.

use prv_analysis::Confidence;
use prv_harmony::Key;
use prv_time::{Frames, Tempo};

/// An opaque identifier for a track.
///
/// The planner never interprets it. It is the caller's key into whatever holds
/// the actual track — the library, a playlist, a test fixture — which keeps the
/// planner independent of how music is stored and lets the same search run over
/// a library, a crate or a folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TrackId(u64);

impl TrackId {
    /// Creates an identifier.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// The underlying value.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// A place in a track where a transition can begin or end.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MixPoint {
    position: Frames,
    energy: f32,
    role: MixPointRole,
}

impl MixPoint {
    /// Creates a mix point.
    #[must_use]
    pub const fn new(position: Frames, energy: f32, role: MixPointRole) -> Self {
        Self {
            position,
            energy,
            role,
        }
    }

    /// Where in the track it is.
    #[must_use]
    pub const fn position(self) -> Frames {
        self.position
    }

    /// The energy of the section it opens, relative to the track's loudest.
    #[must_use]
    pub const fn energy(self) -> f32 {
        self.energy
    }

    /// What kind of point it is.
    #[must_use]
    pub const fn role(self) -> MixPointRole {
        self.role
    }
}

/// What a mix point is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MixPointRole {
    /// A place a new track can be brought in over this one.
    Entry,
    /// A place this track can be taken out from.
    Exit,
}

/// Everything the planner knows about one track.
#[derive(Debug, Clone)]
pub struct Candidate {
    id: TrackId,
    duration: Frames,
    tempo: Tempo,
    key: Option<Key>,
    key_confidence: Confidence,
    energy: f32,
    loudness_lufs: f32,
    has_vocals: Option<bool>,
    points: Vec<MixPoint>,
}

impl Candidate {
    /// Creates a candidate with the minimum a planner needs.
    ///
    /// Energy is clamped to zero to one; a value outside that range is a defect
    /// in whatever built it, and clamping keeps a single bad record from
    /// distorting an entire set's energy curve.
    #[must_use]
    pub fn new(id: TrackId, duration: Frames, tempo: Tempo, energy: f32) -> Self {
        Self {
            id,
            duration,
            tempo,
            key: None,
            key_confidence: Confidence::NONE,
            energy: if energy.is_finite() {
                energy.clamp(0.0, 1.0)
            } else {
                0.0
            },
            loudness_lufs: -14.0,
            has_vocals: None,
            points: Vec::new(),
        }
    }

    /// Attaches a detected key and how well determined it was.
    #[must_use]
    pub fn with_key(mut self, key: Key, confidence: Confidence) -> Self {
        self.key = Some(key);
        self.key_confidence = confidence;
        self
    }

    /// Attaches a measured loudness.
    #[must_use]
    pub const fn with_loudness(mut self, lufs: f32) -> Self {
        self.loudness_lufs = lufs;
        self
    }

    /// Records whether the track has vocals.
    ///
    /// `None` means unknown, which the constraint model treats as a risk rather
    /// than as an absence — two tracks that might both have vocals are a
    /// collision the planner should avoid when it can, and the honest way to
    /// express "we do not know" is to lose a little score rather than to assume
    /// the convenient answer.
    #[must_use]
    pub const fn with_vocals(mut self, has_vocals: bool) -> Self {
        self.has_vocals = Some(has_vocals);
        self
    }

    /// Adds a place a transition can start or end.
    #[must_use]
    pub fn with_point(mut self, point: MixPoint) -> Self {
        self.points.push(point);
        self
    }

    /// The identifier.
    #[must_use]
    pub const fn id(&self) -> TrackId {
        self.id
    }

    /// The track's length.
    #[must_use]
    pub const fn duration(&self) -> Frames {
        self.duration
    }

    /// The tempo.
    #[must_use]
    pub const fn tempo(&self) -> Tempo {
        self.tempo
    }

    /// The key, if one was detected.
    #[must_use]
    pub const fn key(&self) -> Option<Key> {
        self.key
    }

    /// How well determined the key was.
    #[must_use]
    pub const fn key_confidence(&self) -> Confidence {
        self.key_confidence
    }

    /// The track's overall energy, from zero to one.
    #[must_use]
    pub const fn energy(&self) -> f32 {
        self.energy
    }

    /// The measured loudness, in LUFS.
    #[must_use]
    pub const fn loudness_lufs(&self) -> f32 {
        self.loudness_lufs
    }

    /// Whether the track has vocals, if known.
    #[must_use]
    pub const fn has_vocals(&self) -> Option<bool> {
        self.has_vocals
    }

    /// The mix points.
    #[must_use]
    pub fn points(&self) -> &[MixPoint] {
        &self.points
    }

    /// The best place to bring another track in, if any is known.
    ///
    /// The quietest entry point, because a transition into a quiet passage is
    /// the one least likely to produce two arrangements fighting each other.
    #[must_use]
    pub fn best_entry(&self) -> Option<MixPoint> {
        self.points
            .iter()
            .filter(|point| point.role == MixPointRole::Entry)
            .copied()
            .reduce(|best, point| {
                if point.energy < best.energy {
                    point
                } else {
                    best
                }
            })
    }

    /// The best place to take this track out from, if any is known.
    #[must_use]
    pub fn best_exit(&self) -> Option<MixPoint> {
        self.points
            .iter()
            .filter(|point| point.role == MixPointRole::Exit)
            .copied()
            .reduce(|best, point| {
                if point.energy < best.energy {
                    point
                } else {
                    best
                }
            })
    }
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
    use prv_harmony::PitchClass;

    fn tempo() -> Tempo {
        Tempo::from_bpm(128.0).expect("valid")
    }

    #[test]
    fn an_unknown_key_stays_unknown() {
        // The whole point of representing it as an option. A default would let
        // the planner build a set on a harmonic relationship it invented.
        let candidate = Candidate::new(TrackId::new(1), Frames::new(1000), tempo(), 0.5);
        assert_eq!(candidate.key(), None);
        assert_eq!(candidate.key_confidence(), Confidence::NONE);
        assert_eq!(candidate.has_vocals(), None);
    }

    #[test]
    fn energy_outside_the_range_is_contained_rather_than_propagated() {
        let high = Candidate::new(TrackId::new(1), Frames::new(1000), tempo(), 4.0);
        let low = Candidate::new(TrackId::new(2), Frames::new(1000), tempo(), -1.0);
        let broken = Candidate::new(TrackId::new(3), Frames::new(1000), tempo(), f32::NAN);
        assert_eq!(high.energy(), 1.0);
        assert_eq!(low.energy(), 0.0);
        assert_eq!(broken.energy(), 0.0);
    }

    #[test]
    fn the_best_entry_is_the_quietest_one() {
        let candidate = Candidate::new(TrackId::new(1), Frames::new(100_000), tempo(), 0.5)
            .with_point(MixPoint::new(Frames::new(0), 0.4, MixPointRole::Entry))
            .with_point(MixPoint::new(Frames::new(50_000), 0.1, MixPointRole::Entry))
            .with_point(MixPoint::new(Frames::new(90_000), 0.2, MixPointRole::Exit));

        let entry = candidate.best_entry().expect("there are entries");
        assert_eq!(entry.position(), Frames::new(50_000));
        let exit = candidate.best_exit().expect("there is an exit");
        assert_eq!(exit.position(), Frames::new(90_000));
    }

    #[test]
    fn a_track_with_no_points_has_no_best_point() {
        let candidate = Candidate::new(TrackId::new(1), Frames::new(1000), tempo(), 0.5);
        assert!(candidate.best_entry().is_none());
        assert!(candidate.best_exit().is_none());
    }

    #[test]
    fn attached_facts_are_readable() {
        let key = prv_harmony::Key::minor(PitchClass::A);
        let candidate = Candidate::new(TrackId::new(7), Frames::new(1000), tempo(), 0.5)
            .with_key(key, Confidence::new(0.8))
            .with_loudness(-9.5)
            .with_vocals(true);
        assert_eq!(candidate.key(), Some(key));
        assert!(candidate.key_confidence().is_actionable());
        assert_eq!(candidate.loudness_lufs(), -9.5);
        assert_eq!(candidate.has_vocals(), Some(true));
        assert_eq!(candidate.id().get(), 7);
    }
}
