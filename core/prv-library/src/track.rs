use core::fmt;
use std::collections::BTreeSet;

use prv_time::Frames;

/// Identifies one track in the library.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TrackId(u64);

impl TrackId {
    /// Creates an identifier.
    #[must_use]
    pub const fn new(id: u64) -> Self {
        Self(id)
    }

    /// The raw value.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl fmt::Display for TrackId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "track:{}", self.0)
    }
}

/// A handle to the audio itself.
///
/// Opaque here. The core never opens it; the platform layer resolves it to a
/// file, a security-scoped bookmark or a cloud object, whichever applies.
///
/// A handle rather than a path, because a path is not an identity: a user who
/// reorganises their music folder has not replaced their library, and a library
/// keyed on paths would think they had.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MediaRef(String);

impl MediaRef {
    /// Creates a handle.
    #[must_use]
    pub fn new(handle: impl Into<String>) -> Self {
        Self(handle.into())
    }

    /// The opaque handle.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for MediaRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A handle to the artwork for a track.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ArtworkRef(String);

impl ArtworkRef {
    /// Creates a handle.
    #[must_use]
    pub fn new(handle: impl Into<String>) -> Self {
        Self(handle.into())
    }

    /// The opaque handle.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A handle to a track's analysis.
///
/// The analysis itself — tempo, key, structure, energy — belongs to the analysis
/// engine, which versions each stage independently (Master Prompt #20). The
/// library refers to it and never copies it, so improving key detection does not
/// become a migration of the library.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AnalysisRef(u64);

impl AnalysisRef {
    /// Creates a handle.
    #[must_use]
    pub const fn new(id: u64) -> Self {
        Self(id)
    }

    /// The raw value.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// A content fingerprint.
///
/// Derived from the audio rather than from the file, so it survives a re-encode,
/// a tag edit and a rename. Computed by the platform layer; the library only
/// compares.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Fingerprint(u128);

impl Fingerprint {
    /// Creates a fingerprint.
    #[must_use]
    pub const fn new(value: u128) -> Self {
        Self(value)
    }

    /// The raw value.
    #[must_use]
    pub const fn get(self) -> u128 {
        self.0
    }
}

impl fmt::Display for Fingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:032x}", self.0)
    }
}

/// A user rating, from none to five.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Rating(u8);

impl Rating {
    /// Unrated.
    pub const NONE: Self = Self(0);

    /// Creates a rating, clamped to five.
    #[must_use]
    pub const fn new(stars: u8) -> Self {
        Self(if stars > 5 { 5 } else { stars })
    }

    /// The number of stars.
    #[must_use]
    pub const fn stars(self) -> u8 {
        self.0
    }

    /// Whether the track has been rated at all.
    #[must_use]
    pub const fn is_rated(self) -> bool {
        self.0 > 0
    }
}

impl fmt::Display for Rating {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0 == 0 {
            f.write_str("unrated")
        } else {
            write!(f, "{} star{}", self.0, if self.0 == 1 { "" } else { "s" })
        }
    }
}

/// Whether a track is usable.
///
/// Module Specification #001 requires recovery from missing and moved files
/// without losing the user's organisation. These states are how a track survives
/// its audio being unavailable: it stays in the library, keeps its playlists,
/// ratings and tags, and says why it cannot play.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum TrackStatus {
    /// Present and playable.
    Available,
    /// The audio could not be found where it was.
    ///
    /// Not an error to resolve by deleting the track. The user's organisation
    /// around it is intact and the file may come back — an unplugged drive is
    /// the common case, not a lost record.
    Missing,
    /// The audio was found but could not be read.
    Unreadable,
    /// Removed by the user. Hidden, not erased.
    Removed,
}

impl TrackStatus {
    /// Whether the track can be played.
    #[must_use]
    pub const fn is_playable(self) -> bool {
        matches!(self, Self::Available)
    }

    /// Whether the track appears in ordinary browsing.
    #[must_use]
    pub const fn is_visible(self) -> bool {
        !matches!(self, Self::Removed)
    }
}

impl fmt::Display for TrackStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Available => "available",
            Self::Missing => "missing",
            Self::Unreadable => "unreadable",
            Self::Removed => "removed",
        })
    }
}

/// One track in the library.
///
/// The fields Module Specification #001 names, less the ones that belong
/// elsewhere: tempo, key and energy live in the analysis this refers to, not
/// here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Track {
    /// Identity.
    pub id: TrackId,
    /// Title.
    pub title: String,
    /// Artist.
    pub artist: String,
    /// Album.
    pub album: String,
    /// Genre.
    pub genre: String,
    /// Length.
    pub duration: Frames,
    /// Sample rate the file is stored at, in hertz.
    pub sample_rate_hz: u32,
    /// Bits per sample.
    pub bit_depth: u16,
    /// Channel count.
    pub channels: u16,
    /// Codec name, as reported by the decoder.
    pub codec: String,
    /// Where the audio is.
    pub media: MediaRef,
    /// Artwork, if any.
    pub artwork: Option<ArtworkRef>,
    /// Content fingerprint, if it has been computed.
    pub fingerprint: Option<Fingerprint>,
    /// Analysis, if it has been run.
    pub analysis: Option<AnalysisRef>,
    /// When it was imported, in microseconds since the epoch.
    pub imported_at_micros: i64,
    /// When it was last played, if ever.
    pub last_played_micros: Option<i64>,
    /// How many times it has been played.
    pub play_count: u32,
    /// User rating.
    pub rating: Rating,
    /// Whether the user marked it a favourite.
    pub favourite: bool,
    /// User tags, ordered so that iteration is reproducible.
    pub tags: BTreeSet<String>,
    /// The user's own notes.
    pub notes: String,
    /// Whether it is usable.
    pub status: TrackStatus,
    /// Increments on every edit, so a stale writer can be detected.
    pub version: u32,
}

impl Track {
    /// Creates a track with the minimum a decoder can always supply.
    ///
    /// Everything else has a defined default, so an import that reads a file
    /// with no tags at all still produces a usable entry rather than failing.
    #[must_use]
    pub fn new(id: TrackId, title: impl Into<String>, media: MediaRef) -> Self {
        Self {
            id,
            title: title.into(),
            artist: String::new(),
            album: String::new(),
            genre: String::new(),
            duration: Frames::ZERO,
            sample_rate_hz: 44_100,
            bit_depth: 16,
            channels: 2,
            codec: String::new(),
            media,
            artwork: None,
            fingerprint: None,
            analysis: None,
            imported_at_micros: 0,
            last_played_micros: None,
            play_count: 0,
            rating: Rating::NONE,
            favourite: false,
            tags: BTreeSet::new(),
            notes: String::new(),
            status: TrackStatus::Available,
            version: 1,
        }
    }

    /// Every piece of text this track can be searched by.
    ///
    /// Ordered and explicit rather than derived by reflection, so that adding a
    /// field is a deliberate decision about whether it should be searchable.
    #[must_use]
    pub fn searchable_text(&self) -> Vec<&str> {
        let mut text = vec![
            self.title.as_str(),
            self.artist.as_str(),
            self.album.as_str(),
            self.genre.as_str(),
            self.notes.as_str(),
        ];
        text.extend(self.tags.iter().map(String::as_str));
        text
    }

    /// Whether the track has never been analysed.
    #[must_use]
    pub const fn needs_analysis(&self) -> bool {
        self.analysis.is_none()
    }

    /// Whether the track has no artwork.
    ///
    /// Module Specification #001 requires missing artwork to be detectable so
    /// that a library view can offer to find it.
    #[must_use]
    pub const fn needs_artwork(&self) -> bool {
        self.artwork.is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(id: u64, title: &str) -> Track {
        Track::new(
            TrackId::new(id),
            title,
            MediaRef::new(format!("media:{id}")),
        )
    }

    #[test]
    fn a_new_track_is_usable_with_only_a_title() {
        // An import of a file with no tags at all must still produce something
        // the user can see and play.
        let track = track(1, "Untitled");
        assert_eq!(track.status, TrackStatus::Available);
        assert!(track.status.is_playable());
        assert!(track.needs_analysis());
        assert!(track.needs_artwork());
        assert_eq!(track.version, 1);
    }

    #[test]
    fn ratings_are_clamped_rather_than_rejected() {
        assert_eq!(Rating::new(3).stars(), 3);
        assert_eq!(Rating::new(9).stars(), 5);
        assert_eq!(Rating::new(0), Rating::NONE);
        assert!(!Rating::NONE.is_rated());
        assert!(Rating::new(1).is_rated());
    }

    #[test]
    fn ratings_read_naturally() {
        assert_eq!(Rating::NONE.to_string(), "unrated");
        assert_eq!(Rating::new(1).to_string(), "1 star");
        assert_eq!(Rating::new(4).to_string(), "4 stars");
    }

    #[test]
    fn a_missing_track_stays_in_the_library() {
        // The point of the state: an unplugged drive is not a lost record.
        let mut track = track(1, "Somewhere");
        track.status = TrackStatus::Missing;
        assert!(!track.status.is_playable());
        assert!(
            track.status.is_visible(),
            "a missing track must still be visible, with its ratings and playlists intact"
        );
    }

    #[test]
    fn a_removed_track_is_hidden_rather_than_erased() {
        let mut track = track(1, "Gone");
        track.status = TrackStatus::Removed;
        assert!(!track.status.is_visible());
    }

    #[test]
    fn searchable_text_covers_every_field_a_user_would_type() {
        let mut track = track(1, "Strobe");
        track.artist = String::from("deadmau5");
        track.album = String::from("For Lack of a Better Name");
        track.genre = String::from("Progressive House");
        track.notes = String::from("closing track");
        track.tags.insert(String::from("peak-time"));

        let text = track.searchable_text();
        assert!(text.contains(&"Strobe"));
        assert!(text.contains(&"deadmau5"));
        assert!(text.contains(&"For Lack of a Better Name"));
        assert!(text.contains(&"Progressive House"));
        assert!(text.contains(&"closing track"));
        assert!(text.contains(&"peak-time"));
    }

    #[test]
    fn tags_iterate_in_a_stable_order() {
        let mut track = track(1, "Test");
        for tag in ["zebra", "alpha", "mango"] {
            track.tags.insert(String::from(tag));
        }
        let order: Vec<&String> = track.tags.iter().collect();
        assert_eq!(order, vec!["alpha", "mango", "zebra"]);
    }

    #[test]
    fn a_fingerprint_displays_as_a_fixed_width_value() {
        assert_eq!(
            Fingerprint::new(255).to_string(),
            "000000000000000000000000000000ff"
        );
    }

    #[test]
    fn identifiers_display_readably() {
        assert_eq!(TrackId::new(42).to_string(), "track:42");
        assert_eq!(MediaRef::new("file:abc").to_string(), "file:abc");
        assert_eq!(TrackStatus::Missing.to_string(), "missing");
    }
}
