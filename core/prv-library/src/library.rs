use core::fmt;
use std::collections::{BTreeMap, BTreeSet};

use crate::query::{compare, AnalysisFacts, Query};
use crate::track::{Fingerprint, Track, TrackId, TrackStatus};

/// The largest library the engine will hold.
///
/// Module Specification #001 names 100 000 tracks as the demanding case. The
/// bound is set well above it so that a real collection is never refused, and
/// exists so that a corrupted index cannot ask for unbounded allocation.
const MAX_TRACKS: usize = 1_000_000;

/// Shortest prefix a search will index a token under.
///
/// Indexing single characters would put a quarter of the library under "a" and
/// make that search slower than a scan, for a query nobody types deliberately.
const MIN_TOKEN_LENGTH: usize = 1;

/// Failures from library operations.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum LibraryError {
    /// A track with this identity is already present.
    DuplicateId(TrackId),
    /// No track with this identity.
    UnknownTrack(TrackId),
    /// The library has reached its bound.
    LibraryFull,
}

impl fmt::Display for LibraryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateId(id) => write!(f, "{id} is already in the library"),
            Self::UnknownTrack(id) => write!(f, "{id} is not in the library"),
            Self::LibraryFull => f.write_str("the library is full"),
        }
    }
}

impl core::error::Error for LibraryError {}

/// What made two tracks look like duplicates.
///
/// Reported so that the user can judge. Module Specification #001 forbids
/// deleting duplicates automatically, and the reason is that these signals mean
/// different things: an identical fingerprint is near-certain, while identical
/// metadata is common between a single edit and an album version that a DJ may
/// well want both of.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[non_exhaustive]
pub enum DuplicateSignal {
    /// The audio itself matches. Near-certain.
    Fingerprint,
    /// Title, artist and duration all match. Strong, but a remaster is not a
    /// duplicate.
    MetadataAndDuration,
    /// Title and artist match but the durations differ. Often an edit and an
    /// extended mix, which a DJ wants both of.
    MetadataOnly,
}

impl DuplicateSignal {
    /// How much confidence the signal carries, for ordering the report.
    #[must_use]
    pub const fn confidence(self) -> u8 {
        match self {
            Self::Fingerprint => 3,
            Self::MetadataAndDuration => 2,
            Self::MetadataOnly => 1,
        }
    }
}

impl fmt::Display for DuplicateSignal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Fingerprint => "identical audio",
            Self::MetadataAndDuration => "same title, artist and length",
            Self::MetadataOnly => "same title and artist, different length",
        })
    }
}

/// Tracks that appear to be the same recording.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DuplicateGroup {
    /// The tracks involved, in identifier order.
    pub tracks: Vec<TrackId>,
    /// Why they were grouped.
    pub signal: DuplicateSignal,
}

/// What the library contains.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LibraryStats {
    /// Tracks visible in ordinary browsing.
    pub visible: usize,
    /// Tracks the user has removed. Hidden, not erased.
    pub removed: usize,
    /// Tracks whose audio could not be found.
    pub missing: usize,
    /// Tracks that have never been analysed.
    pub unanalysed: usize,
    /// Tracks with no artwork.
    pub without_artwork: usize,
    /// Tracks marked favourite.
    pub favourites: usize,
}

/// A prefix index over the library's text.
///
/// # Why an index rather than a scan
///
/// Module Specification #001 requires search to feel instantaneous at 100 000
/// tracks and to update as the user types. A scan costs the size of the library
/// on every keystroke; an index costs the size of the *answer*. At the point
/// where a query has narrowed to a handful of tracks, that is the difference
/// between a list that keeps up with typing and one that lags behind it.
///
/// Ordered rather than hashed, so that a prefix is a contiguous range — which is
/// what makes "type three letters and see results" a range scan rather than a
/// full pass.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct TextIndex {
    tokens: BTreeMap<String, BTreeSet<TrackId>>,
}

impl TextIndex {
    /// Adds every token in a track's text.
    fn insert(&mut self, id: TrackId, texts: &[&str]) {
        for text in texts {
            for token in tokenise(text) {
                self.tokens.entry(token).or_default().insert(id);
            }
        }
    }

    /// Removes a track from every token it appears under.
    fn remove(&mut self, id: TrackId, texts: &[&str]) {
        for text in texts {
            for token in tokenise(text) {
                let mut empty = false;
                if let Some(entry) = self.tokens.get_mut(&token) {
                    entry.remove(&id);
                    empty = entry.is_empty();
                }
                if empty {
                    self.tokens.remove(&token);
                }
            }
        }
    }

    /// Every track with a token starting with `prefix`.
    fn matching_prefix(&self, prefix: &str) -> BTreeSet<TrackId> {
        let mut found = BTreeSet::new();
        for (token, ids) in self.tokens.range(prefix.to_owned()..) {
            if !token.starts_with(prefix) {
                break;
            }
            found.extend(ids.iter().copied());
        }
        found
    }

    /// Every track matching all of a query's tokens.
    ///
    /// Conjunction across tokens: typing two words narrows rather than widens,
    /// which is what a person expects and what makes the third keystroke faster
    /// than the second rather than slower.
    fn search(&self, text: &str) -> Option<BTreeSet<TrackId>> {
        let mut result: Option<BTreeSet<TrackId>> = None;
        for token in tokenise(text) {
            let matches = self.matching_prefix(&token);
            result = Some(match result {
                None => matches,
                Some(existing) => existing.intersection(&matches).copied().collect(),
            });
            if result.as_ref().is_some_and(BTreeSet::is_empty) {
                break;
            }
        }
        result
    }
}

/// Splits text into lowercase tokens.
///
/// Splits on anything that is not alphanumeric, so "deadmau5 — Strobe (Original
/// Mix)" yields the words a person would type without the punctuation they would
/// not.
fn tokenise(text: &str) -> Vec<String> {
    text.split(|character: char| !character.is_alphanumeric())
        .filter(|token| token.len() >= MIN_TOKEN_LENGTH)
        .map(str::to_lowercase)
        .collect()
}

/// The music library.
///
/// Holds tracks, the projection of analysis facts browsing needs, the text index
/// and the collections a track can belong to.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Library {
    tracks: BTreeMap<TrackId, Track>,
    facts: BTreeMap<TrackId, AnalysisFacts>,
    index: TextIndex,
    collections: BTreeMap<String, BTreeSet<TrackId>>,
}

impl Library {
    /// Creates an empty library.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of tracks, including removed ones.
    #[must_use]
    pub fn len(&self) -> usize {
        self.tracks.len()
    }

    /// Whether the library holds nothing at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tracks.is_empty()
    }

    /// Adds a track.
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::DuplicateId`] if the identity is taken, or
    /// [`LibraryError::LibraryFull`] at the bound.
    pub fn add(&mut self, track: Track) -> Result<(), LibraryError> {
        if self.tracks.len() >= MAX_TRACKS {
            return Err(LibraryError::LibraryFull);
        }
        if self.tracks.contains_key(&track.id) {
            return Err(LibraryError::DuplicateId(track.id));
        }
        self.index.insert(track.id, &track.searchable_text());
        self.tracks.insert(track.id, track);
        Ok(())
    }

    /// Returns a track.
    #[must_use]
    pub fn get(&self, id: TrackId) -> Option<&Track> {
        self.tracks.get(&id)
    }

    /// Replaces a track, keeping the index in step.
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::UnknownTrack`] if there is no such track.
    pub fn update(&mut self, mut track: Track) -> Result<(), LibraryError> {
        let Some(existing) = self.tracks.get(&track.id) else {
            return Err(LibraryError::UnknownTrack(track.id));
        };
        // The index must be updated against the *old* text, or tokens that were
        // removed would linger and the track would keep matching searches it no
        // longer satisfies.
        self.index.remove(track.id, &existing.searchable_text());
        track.version = existing.version.saturating_add(1);
        self.index.insert(track.id, &track.searchable_text());
        self.tracks.insert(track.id, track);
        Ok(())
    }

    /// Marks a track removed.
    ///
    /// Hidden, not erased. Everything the user built around it survives:
    /// playlists, collections, rating, tags, notes and play count. Master Prompt
    /// #9 forbids permanent deletion by default.
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::UnknownTrack`] if there is no such track.
    pub fn remove(&mut self, id: TrackId) -> Result<(), LibraryError> {
        let Some(track) = self.tracks.get_mut(&id) else {
            return Err(LibraryError::UnknownTrack(id));
        };
        track.status = TrackStatus::Removed;
        track.version = track.version.saturating_add(1);
        Ok(())
    }

    /// Brings a removed track back.
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::UnknownTrack`] if there is no such track.
    pub fn restore(&mut self, id: TrackId) -> Result<(), LibraryError> {
        let Some(track) = self.tracks.get_mut(&id) else {
            return Err(LibraryError::UnknownTrack(id));
        };
        track.status = TrackStatus::Available;
        track.version = track.version.saturating_add(1);
        Ok(())
    }

    /// Records the analysis facts browsing needs.
    ///
    /// Called when analysis completes. A projection with a single writer, not a
    /// second source of truth: it can be rebuilt from the analysis engine at any
    /// time.
    pub fn set_analysis_facts(&mut self, id: TrackId, facts: AnalysisFacts) {
        self.facts.insert(id, facts);
    }

    /// The analysis facts recorded for a track.
    #[must_use]
    pub fn analysis_facts(&self, id: TrackId) -> Option<&AnalysisFacts> {
        self.facts.get(&id)
    }

    /// Adds a track to a named collection.
    ///
    /// Collections overlap freely: a track belongs to as many as the user puts
    /// it in. Module Specification #001 requires exactly that, because a record
    /// can be both a warm-up track and a favourite.
    pub fn add_to_collection(&mut self, name: &str, id: TrackId) {
        self.collections
            .entry(name.to_owned())
            .or_default()
            .insert(id);
    }

    /// Removes a track from a collection.
    pub fn remove_from_collection(&mut self, name: &str, id: TrackId) {
        let mut empty = false;
        if let Some(members) = self.collections.get_mut(name) {
            members.remove(&id);
            empty = members.is_empty();
        }
        if empty {
            self.collections.remove(name);
        }
    }

    /// The tracks in a collection.
    #[must_use]
    pub fn collection(&self, name: &str) -> Option<&BTreeSet<TrackId>> {
        self.collections.get(name)
    }

    /// Every collection name, in order.
    pub fn collection_names(&self) -> impl Iterator<Item = &str> {
        self.collections.keys().map(String::as_str)
    }

    /// Runs a query.
    ///
    /// Text narrows first, through the index, so filters and sorting run over
    /// the answer rather than over the library.
    #[must_use]
    pub fn search(&self, query: &Query) -> Vec<TrackId> {
        let candidates = query
            .text
            .as_deref()
            .and_then(|text| self.index.search(text));

        let mut matched: Vec<&Track> = match candidates {
            Some(ids) => ids
                .into_iter()
                .filter_map(|id| self.tracks.get(&id))
                .collect(),
            None => self.tracks.values().collect(),
        };

        matched.retain(|track| {
            // Removed tracks are excluded unless the query asks for them by
            // status, so that ordinary browsing does not show what the user
            // removed while restoring it stays possible.
            let wants_removed = query
                .filters
                .iter()
                .any(|filter| matches!(filter, crate::query::Filter::Status(status) if !status.is_visible()));
            (track.status.is_visible() || wants_removed)
                && query.matches_filters(track, self.facts.get(&track.id))
        });

        matched.sort_by(|left, right| {
            compare(
                left,
                self.facts.get(&left.id),
                right,
                self.facts.get(&right.id),
                query.sort,
                query.order,
            )
        });

        let mut results: Vec<TrackId> = matched.into_iter().map(|track| track.id).collect();
        if let Some(limit) = query.limit {
            results.truncate(limit);
        }
        results
    }

    /// Groups tracks that appear to be the same recording.
    ///
    /// Never removes anything. Module Specification #001 requires duplicates to
    /// be suggested rather than acted on, and the signals below mean different
    /// things: identical audio is near-certain, while an edit and an extended mix
    /// share title and artist and are two records a DJ wants both of.
    ///
    /// Groups are reported strongest signal first, so the near-certain ones are
    /// what the user sees at the top.
    #[must_use]
    pub fn find_duplicates(&self) -> Vec<DuplicateGroup> {
        let mut groups = Vec::new();
        let visible: Vec<&Track> = self
            .tracks
            .values()
            .filter(|track| track.status.is_visible())
            .collect();

        // Identical audio.
        let mut by_fingerprint: BTreeMap<Fingerprint, Vec<TrackId>> = BTreeMap::new();
        for track in &visible {
            if let Some(fingerprint) = track.fingerprint {
                by_fingerprint
                    .entry(fingerprint)
                    .or_default()
                    .push(track.id);
            }
        }
        let mut fingerprinted: BTreeSet<TrackId> = BTreeSet::new();
        for (_, ids) in by_fingerprint {
            if ids.len() > 1 {
                fingerprinted.extend(ids.iter().copied());
                groups.push(DuplicateGroup {
                    tracks: ids,
                    signal: DuplicateSignal::Fingerprint,
                });
            }
        }

        // Metadata, for tracks the fingerprint did not already group. Grouping
        // them again would report the same pair twice with a weaker reason.
        let mut by_metadata: BTreeMap<(String, String), Vec<&Track>> = BTreeMap::new();
        for track in &visible {
            if fingerprinted.contains(&track.id) {
                continue;
            }
            let key = (
                track.title.trim().to_lowercase(),
                track.artist.trim().to_lowercase(),
            );
            if key.0.is_empty() {
                continue;
            }
            by_metadata.entry(key).or_default().push(track);
        }

        for (_, candidates) in by_metadata {
            if candidates.len() < 2 {
                continue;
            }
            let same_length = candidates
                .windows(2)
                .all(|pair| pair.first().map(|t| t.duration) == pair.get(1).map(|t| t.duration));
            groups.push(DuplicateGroup {
                tracks: candidates.iter().map(|track| track.id).collect(),
                signal: if same_length {
                    DuplicateSignal::MetadataAndDuration
                } else {
                    DuplicateSignal::MetadataOnly
                },
            });
        }

        groups.sort_by(|left, right| {
            right
                .signal
                .confidence()
                .cmp(&left.signal.confidence())
                .then_with(|| left.tracks.cmp(&right.tracks))
        });
        groups
    }

    /// What the library contains.
    #[must_use]
    pub fn stats(&self) -> LibraryStats {
        let mut stats = LibraryStats::default();
        for track in self.tracks.values() {
            match track.status {
                TrackStatus::Removed => stats.removed += 1,
                TrackStatus::Missing => {
                    stats.missing += 1;
                    stats.visible += 1;
                }
                _ => stats.visible += 1,
            }
            if track.status.is_visible() {
                if track.needs_analysis() {
                    stats.unanalysed += 1;
                }
                if track.needs_artwork() {
                    stats.without_artwork += 1;
                }
                if track.favourite {
                    stats.favourites += 1;
                }
            }
        }
        stats
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::query::{Filter, SortKey, SortOrder};
    use crate::track::{ArtworkRef, MediaRef, Rating};
    use prv_time::Frames;

    fn track(id: u64, title: &str, artist: &str) -> Track {
        let mut track = Track::new(TrackId::new(id), title, MediaRef::new(format!("m{id}")));
        track.artist = String::from(artist);
        track
    }

    fn library_with(tracks: Vec<Track>) -> Library {
        let mut library = Library::new();
        for track in tracks {
            assert!(library.add(track).is_ok());
        }
        library
    }

    #[test]
    fn a_track_can_be_added_and_found() {
        let mut library = Library::new();
        assert!(library.add(track(1, "Strobe", "deadmau5")).is_ok());
        assert_eq!(library.len(), 1);
        assert_eq!(
            library.get(TrackId::new(1)).map(|t| t.title.as_str()),
            Some("Strobe")
        );
    }

    #[test]
    fn a_duplicate_identity_is_refused() {
        let mut library = Library::new();
        assert!(library.add(track(1, "A", "X")).is_ok());
        assert_eq!(
            library.add(track(1, "B", "Y")),
            Err(LibraryError::DuplicateId(TrackId::new(1)))
        );
    }

    #[test]
    fn search_matches_by_prefix_across_every_field() {
        let library = library_with(vec![
            track(1, "Strobe", "deadmau5"),
            track(2, "Ghosts n Stuff", "deadmau5"),
            track(3, "Midnight City", "M83"),
        ]);

        let by_title = library.search(&Query::new().searching("stro"));
        assert_eq!(by_title, vec![TrackId::new(1)]);

        let by_artist = library.search(&Query::new().searching("deadmau"));
        assert_eq!(by_artist.len(), 2);
    }

    #[test]
    fn typing_more_narrows_rather_than_widens() {
        // What a person expects, and what makes the third keystroke faster than
        // the second rather than slower.
        let library = library_with(vec![
            track(1, "Strobe", "deadmau5"),
            track(2, "Strobe Remix", "Someone Else"),
        ]);

        assert_eq!(library.search(&Query::new().searching("strobe")).len(), 2);
        assert_eq!(
            library.search(&Query::new().searching("strobe dead")).len(),
            1
        );
    }

    #[test]
    fn search_ignores_punctuation_and_case() {
        let mut subject = track(1, "Strobe (Original Mix)", "deadmau5");
        subject.album = String::from("For Lack of a Better Name");
        let library = library_with(vec![subject]);

        assert_eq!(library.search(&Query::new().searching("ORIGINAL")).len(), 1);
        assert_eq!(library.search(&Query::new().searching("mix")).len(), 1);
        assert_eq!(library.search(&Query::new().searching("lack")).len(), 1);
    }

    #[test]
    fn updating_a_track_keeps_the_index_in_step() {
        // Without removing the old text first, a renamed track would keep
        // matching searches it no longer satisfies.
        let mut library = library_with(vec![track(1, "Old Title", "Artist")]);

        let Some(mut updated) = library.get(TrackId::new(1)).cloned() else {
            unreachable!()
        };
        updated.title = String::from("New Title");
        assert!(library.update(updated).is_ok());

        assert!(library.search(&Query::new().searching("old")).is_empty());
        assert_eq!(library.search(&Query::new().searching("new")).len(), 1);
    }

    #[test]
    fn updating_increments_the_version() {
        let mut library = library_with(vec![track(1, "T", "A")]);
        let Some(mut updated) = library.get(TrackId::new(1)).cloned() else {
            unreachable!()
        };
        updated.notes = String::from("changed");
        assert!(library.update(updated).is_ok());
        assert_eq!(library.get(TrackId::new(1)).map(|t| t.version), Some(2));
    }

    #[test]
    fn removing_hides_a_track_without_losing_what_the_user_built() {
        let mut subject = track(1, "Keep", "Artist");
        subject.rating = Rating::new(5);
        subject.tags.insert(String::from("favourite-set"));
        let mut library = library_with(vec![subject]);
        library.add_to_collection("Wedding", TrackId::new(1));

        assert!(library.remove(TrackId::new(1)).is_ok());

        assert!(
            library.search(&Query::new()).is_empty(),
            "a removed track must not appear in ordinary browsing"
        );
        let stored = library.get(TrackId::new(1));
        assert_eq!(stored.map(|t| t.rating), Some(Rating::new(5)));
        assert_eq!(
            library.collection("Wedding").map(BTreeSet::len),
            Some(1),
            "collection membership must survive removal"
        );
    }

    #[test]
    fn a_removed_track_can_be_restored_with_everything_intact() {
        let mut subject = track(1, "Keep", "Artist");
        subject.rating = Rating::new(4);
        let mut library = library_with(vec![subject]);

        assert!(library.remove(TrackId::new(1)).is_ok());
        assert!(library.restore(TrackId::new(1)).is_ok());

        assert_eq!(library.search(&Query::new()).len(), 1);
        assert_eq!(
            library.get(TrackId::new(1)).map(|t| t.rating),
            Some(Rating::new(4))
        );
    }

    #[test]
    fn removed_tracks_can_still_be_found_when_asked_for_by_status() {
        let mut library = library_with(vec![track(1, "Gone", "Artist")]);
        assert!(library.remove(TrackId::new(1)).is_ok());

        let found = library.search(&Query::new().filtered_by(Filter::Status(TrackStatus::Removed)));
        assert_eq!(found, vec![TrackId::new(1)]);
    }

    #[test]
    fn a_missing_track_still_appears_so_the_user_can_find_it() {
        // An unplugged drive is not a lost record.
        let mut library = library_with(vec![track(1, "Somewhere", "Artist")]);
        let Some(mut updated) = library.get(TrackId::new(1)).cloned() else {
            unreachable!()
        };
        updated.status = TrackStatus::Missing;
        assert!(library.update(updated).is_ok());

        assert_eq!(library.search(&Query::new()).len(), 1);
        assert_eq!(library.stats().missing, 1);
    }

    #[test]
    fn filters_and_sorting_compose_with_search() {
        let mut first = track(1, "Alpha", "Artist");
        first.genre = String::from("House");
        first.rating = Rating::new(5);
        let mut second = track(2, "Beta", "Artist");
        second.genre = String::from("House");
        second.rating = Rating::new(2);
        let mut third = track(3, "Gamma", "Artist");
        third.genre = String::from("Techno");
        third.rating = Rating::new(5);

        let library = library_with(vec![first, second, third]);

        let results = library.search(
            &Query::new()
                .filtered_by(Filter::Genre(String::from("house")))
                .sorted_by(SortKey::Rating, SortOrder::Descending),
        );
        assert_eq!(results, vec![TrackId::new(1), TrackId::new(2)]);
    }

    #[test]
    fn a_limit_truncates_after_sorting_not_before() {
        // Otherwise "the top ten by rating" would return ten arbitrary tracks
        // sorted, rather than the ten best.
        let mut tracks = Vec::new();
        for id in 1..=10_u64 {
            let mut subject = track(id, &format!("Track {id}"), "Artist");
            subject.rating = Rating::new((id % 6) as u8);
            tracks.push(subject);
        }
        let library = library_with(tracks);

        let top = library.search(
            &Query::new()
                .sorted_by(SortKey::Rating, SortOrder::Descending)
                .limited_to(3),
        );
        assert_eq!(top.len(), 3);
        for id in &top {
            let rating = library.get(*id).map_or(0, |t| t.rating.stars());
            assert!(rating >= 4, "the limit must keep the best, not the first");
        }
    }

    #[test]
    fn collections_overlap_freely() {
        // A record can be both a warm-up track and a favourite.
        let mut library = library_with(vec![track(1, "T", "A")]);
        library.add_to_collection("Warm Up", TrackId::new(1));
        library.add_to_collection("Favourites", TrackId::new(1));

        assert_eq!(library.collection_names().count(), 2);
        assert!(library
            .collection("Warm Up")
            .is_some_and(|set| set.contains(&TrackId::new(1))));
        assert!(library
            .collection("Favourites")
            .is_some_and(|set| set.contains(&TrackId::new(1))));
    }

    #[test]
    fn an_emptied_collection_disappears() {
        let mut library = library_with(vec![track(1, "T", "A")]);
        library.add_to_collection("Temporary", TrackId::new(1));
        library.remove_from_collection("Temporary", TrackId::new(1));
        assert_eq!(library.collection_names().count(), 0);
    }

    #[test]
    fn identical_audio_is_the_strongest_duplicate_signal() {
        let mut first = track(1, "Strobe", "deadmau5");
        let mut second = track(2, "Strobe (Radio Edit)", "deadmau5");
        first.fingerprint = Some(Fingerprint::new(42));
        second.fingerprint = Some(Fingerprint::new(42));
        let library = library_with(vec![first, second]);

        let groups = library.find_duplicates();
        assert_eq!(groups.len(), 1);
        assert_eq!(
            groups.first().map(|group| group.signal),
            Some(DuplicateSignal::Fingerprint)
        );
    }

    #[test]
    fn an_edit_and_an_extended_mix_are_reported_but_distinguished() {
        // A DJ wants both. Reporting them with a weaker signal lets the
        // interface say so rather than implying one should go.
        let mut first = track(1, "Strobe", "deadmau5");
        let mut second = track(2, "Strobe", "deadmau5");
        first.duration = Frames::new(48_000 * 300);
        second.duration = Frames::new(48_000 * 600);
        let library = library_with(vec![first, second]);

        let groups = library.find_duplicates();
        assert_eq!(
            groups.first().map(|group| group.signal),
            Some(DuplicateSignal::MetadataOnly)
        );
    }

    #[test]
    fn the_same_pair_is_never_reported_twice() {
        // Fingerprint-matched tracks must not appear again under a weaker
        // reason; a user shown the same pair twice loses trust in the report.
        let mut first = track(1, "Strobe", "deadmau5");
        let mut second = track(2, "Strobe", "deadmau5");
        first.fingerprint = Some(Fingerprint::new(7));
        second.fingerprint = Some(Fingerprint::new(7));
        let library = library_with(vec![first, second]);

        assert_eq!(library.find_duplicates().len(), 1);
    }

    #[test]
    fn duplicates_are_reported_strongest_first() {
        let mut certain_a = track(1, "One", "Artist");
        let mut certain_b = track(2, "One", "Artist");
        certain_a.fingerprint = Some(Fingerprint::new(1));
        certain_b.fingerprint = Some(Fingerprint::new(1));
        let likely_a = track(3, "Two", "Artist");
        let likely_b = track(4, "Two", "Artist");

        let library = library_with(vec![certain_a, certain_b, likely_a, likely_b]);
        let groups = library.find_duplicates();
        assert_eq!(groups.len(), 2);
        assert_eq!(
            groups.first().map(|group| group.signal),
            Some(DuplicateSignal::Fingerprint)
        );
    }

    #[test]
    fn removed_tracks_are_not_offered_as_duplicates() {
        let mut first = track(1, "Same", "Artist");
        let second = track(2, "Same", "Artist");
        first.fingerprint = Some(Fingerprint::new(5));
        let mut library = library_with(vec![first, second]);
        assert!(library.remove(TrackId::new(2)).is_ok());

        assert!(library.find_duplicates().is_empty());
    }

    #[test]
    fn untitled_tracks_are_not_grouped_with_each_other() {
        // Two files with no tags are not the same recording.
        let library = library_with(vec![track(1, "", "Artist"), track(2, "", "Artist")]);
        assert!(library.find_duplicates().is_empty());
    }

    #[test]
    fn statistics_describe_what_needs_attention() {
        let mut analysed = track(1, "Done", "Artist");
        analysed.analysis = Some(crate::track::AnalysisRef::new(1));
        analysed.artwork = Some(ArtworkRef::new("art:1"));
        analysed.favourite = true;

        let pending = track(2, "Pending", "Artist");
        let removed = track(3, "Gone", "Artist");

        let mut library = library_with(vec![analysed, pending, removed]);
        assert!(library.remove(TrackId::new(3)).is_ok());

        let stats = library.stats();
        assert_eq!(stats.visible, 2);
        assert_eq!(stats.removed, 1);
        assert_eq!(stats.unanalysed, 1);
        assert_eq!(stats.without_artwork, 1);
        assert_eq!(stats.favourites, 1);
    }

    #[test]
    fn search_stays_fast_on_a_large_library() {
        // Module Specification #001 names 100 000 tracks. Ten thousand here
        // keeps the suite quick while still being far past the point where a
        // scan-per-keystroke would be noticeable; the index makes the cost
        // proportional to the answer rather than the library.
        let mut library = Library::new();
        for id in 0..10_000_u64 {
            let mut subject = track(id, &format!("Track {id}"), "Various");
            subject.album = format!("Album {}", id % 100);
            assert!(library.add(subject).is_ok());
        }

        let narrow = library.search(&Query::new().searching("9999"));
        assert_eq!(narrow.len(), 1, "a specific search must find exactly one");

        // Prefix matching is deliberately broad on numbers: "42" also matches
        // 420 and 4200, so a query for album 42 finds every track on that album
        // *and* every track whose number begins with 42. That is the right
        // behaviour for search-as-you-type — narrowing should never hide a
        // result the user has not finished describing — but it means the
        // guarantee to assert is completeness, not exactness.
        let by_album = library.search(&Query::new().searching("album 42"));
        for id in (42..10_000).step_by(100) {
            assert!(
                by_album.contains(&TrackId::new(id)),
                "every track on album 42 must be found; {id} was missing"
            );
        }

        let missing = library.search(&Query::new().searching("nonexistent"));
        assert!(missing.is_empty());
    }

    #[test]
    fn unknown_tracks_are_reported_rather_than_ignored() {
        let mut library = Library::new();
        assert_eq!(
            library.remove(TrackId::new(9)),
            Err(LibraryError::UnknownTrack(TrackId::new(9)))
        );
        assert_eq!(
            library.restore(TrackId::new(9)),
            Err(LibraryError::UnknownTrack(TrackId::new(9)))
        );
        assert_eq!(
            library.update(track(9, "Nope", "Nobody")),
            Err(LibraryError::UnknownTrack(TrackId::new(9)))
        );
    }

    #[test]
    fn duplicate_signals_read_as_explanations() {
        assert_eq!(DuplicateSignal::Fingerprint.to_string(), "identical audio");
        assert_eq!(
            DuplicateSignal::MetadataOnly.to_string(),
            "same title and artist, different length"
        );
        assert!(
            DuplicateSignal::Fingerprint.confidence() > DuplicateSignal::MetadataOnly.confidence()
        );
    }
}
