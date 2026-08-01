use core::cmp::Ordering;
use core::fmt;

use prv_harmony::{compatibility, CamelotCode, HarmonicSafety};
use prv_time::Frames;

use crate::track::{Rating, Track, TrackStatus};

/// The facts about a track that come from analysis and are needed for browsing.
///
/// # Why the library keeps a copy of these
///
/// Master Prompt #20 makes the analysis engine the owner of tempo, key and
/// energy, and versions each stage independently. Module Specification #001
/// nonetheless requires the library to filter by all three, and a filter that
/// had to consult another module for every track would make a 100 000-track
/// query a 100 000-call fan-out.
///
/// So the library keeps a small projection: the handful of values browsing needs,
/// refreshed when analysis completes. It is a cache with a single writer and an
/// obvious rebuild path, not a second source of truth — which is why it lives
/// beside the track record rather than inside it, and why every field is
/// optional. A track that has not been analysed is a normal track, not a broken
/// one.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct AnalysisFacts {
    /// Detected tempo, in beats per minute.
    pub tempo_bpm: Option<f32>,
    /// Detected key, as a position on the Camelot wheel.
    pub camelot: Option<CamelotCode>,
    /// Overall energy, from zero to one.
    pub energy: Option<f32>,
}

/// One condition a track must satisfy.
///
/// Filters compose by conjunction: a query holds several and a track must
/// satisfy all of them. Module Specification #001 requires filters to be
/// composable, and conjunction is what a person means when they tick two boxes.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Filter {
    /// Genre matches, ignoring case.
    Genre(String),
    /// The track carries a tag, ignoring case.
    Tag(String),
    /// Favourite, or not.
    Favourite(bool),
    /// Rated at least this many stars.
    RatingAtLeast(Rating),
    /// Length within a range, inclusive.
    DurationBetween(Frames, Frames),
    /// Imported at or after a moment.
    ImportedAfter(i64),
    /// Played at least once.
    Played(bool),
    /// In a particular state.
    Status(TrackStatus),
    /// Tempo within a range, inclusive. Excludes unanalysed tracks.
    TempoBetween(f32, f32),
    /// Exactly this key.
    KeyIs(CamelotCode),
    /// A key that mixes safely with this one.
    ///
    /// Uses the same harmonic model the planner uses, so what a user sees when
    /// they filter is what the planner will consider — rather than two different
    /// notions of "compatible" that disagree at the edges.
    KeyCompatibleWith(CamelotCode),
    /// Energy within a range, inclusive. Excludes unanalysed tracks.
    EnergyBetween(f32, f32),
    /// Never analysed.
    NeedsAnalysis,
    /// No artwork.
    NeedsArtwork,
}

impl Filter {
    /// Whether a track satisfies this condition.
    #[must_use]
    pub fn matches(&self, track: &Track, facts: Option<&AnalysisFacts>) -> bool {
        match self {
            Self::Genre(genre) => track.genre.eq_ignore_ascii_case(genre),
            Self::Tag(tag) => track
                .tags
                .iter()
                .any(|existing| existing.eq_ignore_ascii_case(tag)),
            Self::Favourite(wanted) => track.favourite == *wanted,
            Self::RatingAtLeast(minimum) => track.rating >= *minimum,
            Self::DurationBetween(low, high) => track.duration >= *low && track.duration <= *high,
            Self::ImportedAfter(moment) => track.imported_at_micros >= *moment,
            Self::Played(wanted) => (track.play_count > 0) == *wanted,
            Self::Status(status) => track.status == *status,
            Self::TempoBetween(low, high) => facts
                .and_then(|facts| facts.tempo_bpm)
                .is_some_and(|tempo| tempo >= *low && tempo <= *high),
            Self::KeyIs(code) => facts
                .and_then(|facts| facts.camelot)
                .is_some_and(|key| key == *code),
            Self::KeyCompatibleWith(code) => {
                facts.and_then(|facts| facts.camelot).is_some_and(|key| {
                    compatibility(code.to_key(), key.to_key()).safety == HarmonicSafety::Safe
                })
            }
            Self::EnergyBetween(low, high) => facts
                .and_then(|facts| facts.energy)
                .is_some_and(|energy| energy >= *low && energy <= *high),
            Self::NeedsAnalysis => track.needs_analysis(),
            Self::NeedsArtwork => track.needs_artwork(),
        }
    }
}

/// What results are ordered by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum SortKey {
    /// Alphabetically by title.
    #[default]
    Title,
    /// Alphabetically by artist, then album, then title.
    Artist,
    /// Alphabetically by album, then title.
    Album,
    /// Most recently imported first, when descending.
    DateAdded,
    /// Most recently played first, when descending.
    LastPlayed,
    /// Most played first, when descending.
    PlayCount,
    /// Highest rated first, when descending.
    Rating,
    /// Longest first, when descending.
    Duration,
    /// Fastest first, when descending. Unanalysed tracks sort last.
    Tempo,
    /// Most energetic first, when descending. Unanalysed tracks sort last.
    Energy,
}

impl fmt::Display for SortKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Title => "title",
            Self::Artist => "artist",
            Self::Album => "album",
            Self::DateAdded => "date added",
            Self::LastPlayed => "last played",
            Self::PlayCount => "play count",
            Self::Rating => "rating",
            Self::Duration => "duration",
            Self::Tempo => "tempo",
            Self::Energy => "energy",
        })
    }
}

/// Which direction a sort runs in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortOrder {
    /// Smallest, earliest or alphabetically first.
    #[default]
    Ascending,
    /// Largest, latest or alphabetically last.
    Descending,
}

/// A request for tracks.
///
/// Text and filters compose: a track must match the text *and* every filter.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Query {
    /// Text the user typed. Matched by prefix against every searchable field.
    pub text: Option<String>,
    /// Conditions, all of which must hold.
    pub filters: Vec<Filter>,
    /// What to order by.
    pub sort: SortKey,
    /// Which direction.
    pub order: SortOrder,
    /// Most results to return. `None` means all of them.
    pub limit: Option<usize>,
}

impl Query {
    /// An unfiltered query, sorted by title.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the search text.
    #[must_use]
    pub fn searching(mut self, text: impl Into<String>) -> Self {
        let text = text.into();
        self.text = if text.trim().is_empty() {
            None
        } else {
            Some(text)
        };
        self
    }

    /// Adds a condition.
    #[must_use]
    pub fn filtered_by(mut self, filter: Filter) -> Self {
        self.filters.push(filter);
        self
    }

    /// Sets the ordering.
    #[must_use]
    pub const fn sorted_by(mut self, sort: SortKey, order: SortOrder) -> Self {
        self.sort = sort;
        self.order = order;
        self
    }

    /// Caps the number of results.
    #[must_use]
    pub const fn limited_to(mut self, limit: usize) -> Self {
        self.limit = Some(limit);
        self
    }

    /// Whether a track satisfies every filter.
    #[must_use]
    pub fn matches_filters(&self, track: &Track, facts: Option<&AnalysisFacts>) -> bool {
        self.filters
            .iter()
            .all(|filter| filter.matches(track, facts))
    }
}

/// Compares two tracks under a sort key.
///
/// # Why the tie-break exists
///
/// Sorting by rating puts many tracks at the same value. Without a stable
/// tie-break the order within a group would depend on where each track happened
/// to sit beforehand, and a list would appear to shuffle itself when the user
/// changed something unrelated. Falling back to the identifier makes the order
/// total and reproducible.
///
/// # Why unanalysed tracks sort last
///
/// A track with no tempo is not a track with a tempo of zero. Sorting it as
/// though it were would bury or promote it arbitrarily; placing it after
/// everything that *has* a value, in both directions, is what a person reading
/// the list expects.
#[must_use]
pub(crate) fn compare(
    left: &Track,
    left_facts: Option<&AnalysisFacts>,
    right: &Track,
    right_facts: Option<&AnalysisFacts>,
    key: SortKey,
    order: SortOrder,
) -> Ordering {
    let primary = match key {
        SortKey::Title => compare_text(&left.title, &right.title),
        SortKey::Artist => compare_text(&left.artist, &right.artist)
            .then_with(|| compare_text(&left.album, &right.album))
            .then_with(|| compare_text(&left.title, &right.title)),
        SortKey::Album => compare_text(&left.album, &right.album)
            .then_with(|| compare_text(&left.title, &right.title)),
        SortKey::DateAdded => left.imported_at_micros.cmp(&right.imported_at_micros),
        SortKey::LastPlayed => left.last_played_micros.cmp(&right.last_played_micros),
        SortKey::PlayCount => left.play_count.cmp(&right.play_count),
        SortKey::Rating => left.rating.cmp(&right.rating),
        SortKey::Duration => left.duration.cmp(&right.duration),
        SortKey::Tempo => {
            return with_missing_last(
                left_facts.and_then(|facts| facts.tempo_bpm),
                right_facts.and_then(|facts| facts.tempo_bpm),
                order,
            )
            .unwrap_or_else(|| {
                apply_order(
                    compare_optional_float(
                        left_facts.and_then(|facts| facts.tempo_bpm),
                        right_facts.and_then(|facts| facts.tempo_bpm),
                    ),
                    order,
                )
                .then_with(|| left.id.cmp(&right.id))
            })
        }
        SortKey::Energy => {
            return with_missing_last(
                left_facts.and_then(|facts| facts.energy),
                right_facts.and_then(|facts| facts.energy),
                order,
            )
            .unwrap_or_else(|| {
                apply_order(
                    compare_optional_float(
                        left_facts.and_then(|facts| facts.energy),
                        right_facts.and_then(|facts| facts.energy),
                    ),
                    order,
                )
                .then_with(|| left.id.cmp(&right.id))
            })
        }
    };

    apply_order(primary, order).then_with(|| left.id.cmp(&right.id))
}

/// Places a track with no value after one that has a value, whichever direction
/// the sort runs in.
///
/// Returns `None` when both have a value or neither does, leaving the caller to
/// compare them normally.
fn with_missing_last(left: Option<f32>, right: Option<f32>, order: SortOrder) -> Option<Ordering> {
    let _ = order;
    match (left, right) {
        (Some(_), None) => Some(Ordering::Less),
        (None, Some(_)) => Some(Ordering::Greater),
        _ => None,
    }
}

/// Compares two optional floats, treating absence as equal.
fn compare_optional_float(left: Option<f32>, right: Option<f32>) -> Ordering {
    match (left, right) {
        (Some(left), Some(right)) => left.partial_cmp(&right).unwrap_or(Ordering::Equal),
        _ => Ordering::Equal,
    }
}

/// Case-insensitive comparison, so a library does not sort by capitalisation.
fn compare_text(left: &str, right: &str) -> Ordering {
    left.to_lowercase().cmp(&right.to_lowercase())
}

/// Reverses an ordering when the sort is descending.
const fn apply_order(ordering: Ordering, order: SortOrder) -> Ordering {
    match order {
        SortOrder::Ascending => ordering,
        SortOrder::Descending => ordering.reverse(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::track::{MediaRef, TrackId};
    use prv_harmony::{Key, PitchClass, Wheel};

    fn track(id: u64, title: &str) -> Track {
        Track::new(TrackId::new(id), title, MediaRef::new(format!("m{id}")))
    }

    fn camelot(number: u8, wheel: Wheel) -> CamelotCode {
        CamelotCode::new(number, wheel)
            .unwrap_or_else(|| CamelotCode::from_key(Key::minor(PitchClass::A)))
    }

    #[test]
    fn filters_compose_by_conjunction() {
        // What a person means when they tick two boxes.
        let mut subject = track(1, "Strobe");
        subject.genre = String::from("House");
        subject.favourite = true;

        let query = Query::new()
            .filtered_by(Filter::Genre(String::from("house")))
            .filtered_by(Filter::Favourite(true));
        assert!(query.matches_filters(&subject, None));

        let stricter = query
            .clone()
            .filtered_by(Filter::RatingAtLeast(Rating::new(4)));
        assert!(!stricter.matches_filters(&subject, None));
    }

    #[test]
    fn genre_and_tag_matching_ignores_case() {
        let mut subject = track(1, "Test");
        subject.genre = String::from("Melodic Techno");
        subject.tags.insert(String::from("Peak-Time"));

        assert!(Filter::Genre(String::from("melodic techno")).matches(&subject, None));
        assert!(Filter::Tag(String::from("peak-time")).matches(&subject, None));
        assert!(!Filter::Tag(String::from("warmup")).matches(&subject, None));
    }

    #[test]
    fn an_unanalysed_track_never_matches_an_analysis_filter() {
        // Not a match with a default value: absence is absence.
        let subject = track(1, "Unknown");
        assert!(!Filter::TempoBetween(120.0, 130.0).matches(&subject, None));
        assert!(!Filter::EnergyBetween(0.0, 1.0).matches(&subject, None));
        assert!(!Filter::KeyIs(camelot(8, Wheel::A)).matches(&subject, None));
        assert!(Filter::NeedsAnalysis.matches(&subject, None));
    }

    #[test]
    fn tempo_and_energy_ranges_are_inclusive() {
        let subject = track(1, "Test");
        let facts = AnalysisFacts {
            tempo_bpm: Some(128.0),
            camelot: None,
            energy: Some(0.75),
        };
        assert!(Filter::TempoBetween(128.0, 130.0).matches(&subject, Some(&facts)));
        assert!(Filter::TempoBetween(120.0, 128.0).matches(&subject, Some(&facts)));
        assert!(!Filter::TempoBetween(129.0, 140.0).matches(&subject, Some(&facts)));
        assert!(Filter::EnergyBetween(0.75, 0.75).matches(&subject, Some(&facts)));
    }

    #[test]
    fn harmonic_filtering_uses_the_same_model_as_the_planner() {
        // What a user sees when they filter must be what the planner will
        // consider, rather than two notions of "compatible" that disagree.
        let subject = track(1, "Test");
        let a_minor = camelot(8, Wheel::A);
        let facts = AnalysisFacts {
            tempo_bpm: None,
            camelot: Some(a_minor),
            energy: None,
        };

        // Its relative major and its neighbours are safe.
        assert!(Filter::KeyCompatibleWith(camelot(8, Wheel::B)).matches(&subject, Some(&facts)));
        assert!(Filter::KeyCompatibleWith(camelot(9, Wheel::A)).matches(&subject, Some(&facts)));
        // The far side of the wheel is not.
        assert!(!Filter::KeyCompatibleWith(camelot(2, Wheel::A)).matches(&subject, Some(&facts)));
    }

    #[test]
    fn sorting_is_case_insensitive() {
        // A library that sorted by capitalisation would put "abba" after "ZZ".
        let lower = track(1, "abba");
        let upper = track(2, "ZZ Top");
        assert_eq!(
            compare(
                &lower,
                None,
                &upper,
                None,
                SortKey::Title,
                SortOrder::Ascending
            ),
            Ordering::Less
        );
    }

    #[test]
    fn sorting_is_total_so_lists_do_not_shuffle() {
        // Two tracks with the same rating must have a stable relative order, or
        // the list appears to reshuffle when something unrelated changes.
        let mut left = track(1, "Same");
        let mut right = track(2, "Same");
        left.rating = Rating::new(4);
        right.rating = Rating::new(4);

        let forward = compare(
            &left,
            None,
            &right,
            None,
            SortKey::Rating,
            SortOrder::Ascending,
        );
        let backward = compare(
            &right,
            None,
            &left,
            None,
            SortKey::Rating,
            SortOrder::Ascending,
        );
        assert_eq!(forward, Ordering::Less);
        assert_eq!(backward, Ordering::Greater);
    }

    #[test]
    fn descending_reverses_the_primary_key_but_not_the_tie_break() {
        let mut left = track(1, "Same");
        let mut right = track(2, "Same");
        left.play_count = 5;
        right.play_count = 5;

        // Equal primaries fall through to the identifier, which stays ascending
        // so that the order remains reproducible in both directions.
        assert_eq!(
            compare(
                &left,
                None,
                &right,
                None,
                SortKey::PlayCount,
                SortOrder::Descending
            ),
            Ordering::Less
        );
    }

    #[test]
    fn unanalysed_tracks_sort_last_in_both_directions() {
        // A track with no tempo is not a track with a tempo of zero.
        let analysed = track(1, "Known");
        let unanalysed = track(2, "Unknown");
        let facts = AnalysisFacts {
            tempo_bpm: Some(128.0),
            camelot: None,
            energy: None,
        };

        for order in [SortOrder::Ascending, SortOrder::Descending] {
            assert_eq!(
                compare(
                    &analysed,
                    Some(&facts),
                    &unanalysed,
                    None,
                    SortKey::Tempo,
                    order
                ),
                Ordering::Less,
                "a track with a value must precede one without, sorting {order:?}"
            );
            assert_eq!(
                compare(
                    &unanalysed,
                    None,
                    &analysed,
                    Some(&facts),
                    SortKey::Tempo,
                    order
                ),
                Ordering::Greater
            );
        }
    }

    #[test]
    fn artist_sort_falls_through_album_then_title() {
        let mut first = track(1, "B side");
        let mut second = track(2, "A side");
        first.artist = String::from("Same");
        second.artist = String::from("Same");
        first.album = String::from("Album");
        second.album = String::from("Album");

        assert_eq!(
            compare(
                &first,
                None,
                &second,
                None,
                SortKey::Artist,
                SortOrder::Ascending
            ),
            Ordering::Greater,
            "with artist and album equal, the title decides"
        );
    }

    #[test]
    fn blank_search_text_is_treated_as_no_search() {
        assert_eq!(Query::new().searching("   ").text, None);
        assert_eq!(Query::new().searching("").text, None);
        assert_eq!(
            Query::new().searching("strobe").text,
            Some(String::from("strobe"))
        );
    }

    #[test]
    fn a_query_reads_as_a_sentence_when_built() {
        let query = Query::new()
            .searching("house")
            .filtered_by(Filter::Favourite(true))
            .sorted_by(SortKey::Rating, SortOrder::Descending)
            .limited_to(50);
        assert_eq!(query.filters.len(), 1);
        assert_eq!(query.sort, SortKey::Rating);
        assert_eq!(query.order, SortOrder::Descending);
        assert_eq!(query.limit, Some(50));
        assert_eq!(SortKey::Rating.to_string(), "rating");
    }
}
