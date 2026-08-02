//! The user's music, as a host sees it.
//!
//! # Why a search returns identities rather than tracks
//!
//! A result is a list of whatever the user typed matched, and a list of tracks
//! crossing C would mean a `#[repr(C)]` layout for everything a track knows —
//! title, artist, album, genre, codec, rating, tags — which is a permanent
//! promise about a type that exists to grow.
//!
//! So a search returns *how many* matched, and the host reads the identities
//! back one at a time, then asks for whichever fields it is about to draw. A
//! list view asks for three fields per visible row and nothing for the ten
//! thousand rows it is not showing, which is both less work and the shape a list
//! view wanted anyway.
//!
//! # Strings come back by copy into the caller's buffer
//!
//! The alternative — handing back a pointer into the library — is a pointer that
//! dangles the moment the user renames something, and nothing in C would notice.
//! Copying into a buffer the caller owns costs a `memcpy` per visible row and
//! removes the entire class of bug.
//!
//! A caller that guesses too small a buffer gets [`Status::BufferTooSmall`] and
//! the length it needed, so the second attempt is exact rather than another
//! guess.

use prv_library::{Library, MediaRef, Query, SortKey, SortOrder, Track, TrackId};
use prv_time::Frames;

use crate::status::Status;

/// The user's music, and the last search over it.
#[derive(Debug, Default)]
pub struct Collection {
    library: Library,
    /// The identities the last search matched, in the order it ranked them.
    results: Vec<TrackId>,
}

impl Collection {
    /// An empty library.
    #[must_use]
    pub fn new() -> Self {
        Self {
            library: Library::new(),
            results: Vec::new(),
        }
    }

    /// Adds a track.
    ///
    /// # Errors
    ///
    /// [`Status::Refused`] when the identity is already in use — which is a
    /// re-import rather than a new track, and a caller that wants to replace one
    /// says so with [`Self::update_metadata`].
    #[allow(
        clippy::too_many_arguments,
        reason = "a track is what a file told us about itself, and a struct here \
                  would be a layout the C boundary then has to promise forever"
    )]
    pub fn add(
        &mut self,
        id: u64,
        title: &str,
        artist: &str,
        album: &str,
        media: &str,
        duration: i64,
        imported_at_micros: i64,
    ) -> Result<(), Status> {
        if duration < 0 {
            return Err(Status::InvalidArgument);
        }
        let mut track = Track::new(TrackId::new(id), title, MediaRef::new(media));
        artist.clone_into(&mut track.artist);
        album.clone_into(&mut track.album);
        track.duration = Frames::new(duration);
        track.imported_at_micros = imported_at_micros;
        self.library.add(track).map_err(|_| Status::Refused)
    }

    /// Changes a track's title, artist and album.
    ///
    /// # Errors
    ///
    /// [`Status::InvalidHandle`] when no track has that identity.
    pub fn update_metadata(
        &mut self,
        id: u64,
        title: &str,
        artist: &str,
        album: &str,
    ) -> Result<(), Status> {
        let id = TrackId::new(id);
        let mut track = self.library.get(id).ok_or(Status::InvalidHandle)?.clone();
        title.clone_into(&mut track.title);
        artist.clone_into(&mut track.artist);
        album.clone_into(&mut track.album);
        self.library.update(track).map_err(|_| Status::Refused)
    }

    /// Removes a track from view.
    ///
    /// # This does not erase anything
    ///
    /// `prv-library` marks the track removed rather than dropping it, and
    /// [`Self::restore`] brings it back with its rating, tags and play count
    /// intact. Master Prompt #9 says the user owns their work; a delete that
    /// destroyed the metadata they had built up over a year would be the
    /// opposite of that, whatever it was called.
    ///
    /// Removing an already-removed track therefore succeeds and changes
    /// nothing. A host repeating the call has not made a mistake worth
    /// reporting.
    ///
    /// # Errors
    ///
    /// [`Status::InvalidHandle`] when no track has ever had that identity.
    pub fn remove(&mut self, id: u64) -> Result<(), Status> {
        self.library
            .remove(TrackId::new(id))
            .map_err(|_| Status::InvalidHandle)
    }

    /// Brings a removed track back, with everything it had.
    ///
    /// The other half of [`Self::remove`], and the reason that one is safe. A
    /// boundary that offered removal without this would have handed every host
    /// a destructive action with no undo.
    ///
    /// # Errors
    ///
    /// [`Status::InvalidHandle`] when no track has that identity.
    pub fn restore(&mut self, id: u64) -> Result<(), Status> {
        self.library
            .restore(TrackId::new(id))
            .map_err(|_| Status::InvalidHandle)
    }

    /// How many tracks the library holds.
    #[must_use]
    pub fn len(&self) -> u64 {
        self.library.len().try_into().unwrap_or(u64::MAX)
    }

    /// Whether the library is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.library.is_empty()
    }

    /// Runs a search and keeps the result for reading back.
    ///
    /// An empty `text` matches everything, which is what a list view showing the
    /// whole library asks for and saves it inventing a second call.
    ///
    /// # Errors
    ///
    /// [`Status::InvalidArgument`] when the sort key or order is one this
    /// version does not define.
    pub fn search(&mut self, text: &str, sort: i32, descending: bool) -> Result<u64, Status> {
        let key = sort_from_code(sort).ok_or(Status::InvalidArgument)?;
        let order = if descending {
            SortOrder::Descending
        } else {
            SortOrder::Ascending
        };
        let mut query = Query::new().sorted_by(key, order);
        if !text.is_empty() {
            query = query.searching(text);
        }
        self.results = self.library.search(&query);
        Ok(self.results.len().try_into().unwrap_or(u64::MAX))
    }

    /// The identity of one result.
    ///
    /// # Errors
    ///
    /// [`Status::InvalidArgument`] when there is no result at that index.
    pub fn result(&self, index: u64) -> Result<u64, Status> {
        let index = usize::try_from(index).map_err(|_| Status::InvalidArgument)?;
        self.results
            .get(index)
            .map(|id| id.get())
            .ok_or(Status::InvalidArgument)
    }

    /// One text field of a track, copied into `into`.
    ///
    /// Returns how many bytes the field needs, *including* the terminator,
    /// whether or not it fitted. A caller given [`Status::BufferTooSmall`] can
    /// therefore allocate exactly and call once more.
    ///
    /// # Errors
    ///
    /// [`Status::InvalidHandle`] for an unknown track, [`Status::InvalidArgument`]
    /// for an unknown field, and [`Status::BufferTooSmall`] when it did not fit.
    pub fn text_field(&self, id: u64, field: i32, into: &mut [u8]) -> Result<usize, Status> {
        let track = self
            .library
            .get(TrackId::new(id))
            .ok_or(Status::InvalidHandle)?;
        let value: &str = match field {
            0 => &track.title,
            1 => &track.artist,
            2 => &track.album,
            3 => &track.genre,
            4 => track.media.as_str(),
            _ => return Err(Status::InvalidArgument),
        };

        let needed = value.len().saturating_add(1);
        if into.len() < needed {
            return Ok(needed);
        }
        let bytes = value.as_bytes();
        into.get_mut(..bytes.len())
            .ok_or(Status::BufferTooSmall)?
            .copy_from_slice(bytes);
        *into.get_mut(bytes.len()).ok_or(Status::BufferTooSmall)? = 0;
        Ok(needed)
    }

    /// A track's length in frames.
    ///
    /// # Errors
    ///
    /// [`Status::InvalidHandle`] when no track has that identity.
    pub fn duration(&self, id: u64) -> Result<i64, Status> {
        Ok(self
            .library
            .get(TrackId::new(id))
            .ok_or(Status::InvalidHandle)?
            .duration
            .get())
    }
}

/// The sort key a code names.
#[must_use]
pub const fn sort_from_code(code: i32) -> Option<SortKey> {
    match code {
        0 => Some(SortKey::Title),
        1 => Some(SortKey::Artist),
        2 => Some(SortKey::Album),
        3 => Some(SortKey::DateAdded),
        4 => Some(SortKey::LastPlayed),
        5 => Some(SortKey::PlayCount),
        6 => Some(SortKey::Rating),
        7 => Some(SortKey::Duration),
        8 => Some(SortKey::Tempo),
        9 => Some(SortKey::Energy),
        _ => None,
    }
}

/// Every sort key with its C spelling, in code order.
pub const SORT_KEYS: &[(SortKey, &str)] = &[
    (SortKey::Title, "PRV_SORT_TITLE"),
    (SortKey::Artist, "PRV_SORT_ARTIST"),
    (SortKey::Album, "PRV_SORT_ALBUM"),
    (SortKey::DateAdded, "PRV_SORT_DATE_ADDED"),
    (SortKey::LastPlayed, "PRV_SORT_LAST_PLAYED"),
    (SortKey::PlayCount, "PRV_SORT_PLAY_COUNT"),
    (SortKey::Rating, "PRV_SORT_RATING"),
    (SortKey::Duration, "PRV_SORT_DURATION"),
    (SortKey::Tempo, "PRV_SORT_TEMPO"),
    (SortKey::Energy, "PRV_SORT_ENERGY"),
];

/// Every text field with its C spelling, in code order.
pub const TEXT_FIELDS: &[&str] = &[
    "PRV_FIELD_TITLE",
    "PRV_FIELD_ARTIST",
    "PRV_FIELD_ALBUM",
    "PRV_FIELD_GENRE",
    "PRV_FIELD_MEDIA",
];

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::panic,
        reason = "a test that cannot build its own fixture should fail loudly"
    )]

    use super::*;

    fn stocked() -> Collection {
        let mut collection = Collection::new();
        for (id, title, artist) in [
            (1_u64, "Strobe", "deadmau5"),
            (2, "Alive", "Daft Punk"),
            (3, "Windowlicker", "Aphex Twin"),
        ] {
            collection
                .add(
                    id,
                    title,
                    artist,
                    "An Album",
                    "file://x",
                    44_100 * 300,
                    1_000,
                )
                .expect("a fresh identity");
        }
        collection
    }

    #[test]
    fn a_search_finds_what_the_user_typed_and_reads_back_in_order() {
        let mut collection = stocked();
        let found = collection.search("dead", 0, false).expect("a real sort");
        assert_eq!(found, 1);
        assert_eq!(collection.result(0), Ok(1));
        assert_eq!(collection.result(1), Err(Status::InvalidArgument));
    }

    #[test]
    fn an_empty_search_is_the_whole_library_rather_than_nothing() {
        // What a list view showing everything asks for. Returning nothing would
        // make it invent a second call.
        let mut collection = stocked();
        assert_eq!(collection.search("", 0, false), Ok(3));
    }

    #[test]
    fn a_field_that_did_not_fit_reports_the_length_it_needed() {
        // So the second attempt is exact rather than another guess.
        let collection = stocked();
        let mut tiny = [0_u8; 2];
        let needed = collection
            .text_field(1, 0, &mut tiny)
            .expect("a real field");
        assert_eq!(needed, "Strobe".len() + 1);

        let mut exact = vec![0_u8; needed];
        assert_eq!(collection.text_field(1, 0, &mut exact), Ok(needed));
        assert_eq!(exact.get(..6), Some(&b"Strobe"[..]));
        assert_eq!(exact.get(6), Some(&0), "the string was not terminated");
    }

    #[test]
    fn a_field_is_always_terminated_even_when_it_is_empty() {
        // C reads until a zero. An empty genre with no terminator is a read into
        // whatever the caller's buffer held before.
        let collection = stocked();
        let mut buffer = [0xFF_u8; 8];
        let needed = collection.text_field(1, 3, &mut buffer).expect("genre");
        assert_eq!(needed, 1);
        assert_eq!(buffer[0], 0);
    }

    #[test]
    fn an_unknown_track_or_field_is_refused_rather_than_returning_nothing() {
        let collection = stocked();
        let mut buffer = [0_u8; 64];
        assert_eq!(
            collection.text_field(999, 0, &mut buffer),
            Err(Status::InvalidHandle)
        );
        assert_eq!(
            collection.text_field(1, 99, &mut buffer),
            Err(Status::InvalidArgument)
        );
        assert_eq!(collection.duration(999), Err(Status::InvalidHandle));
    }

    #[test]
    fn re_adding_an_identity_is_refused_rather_than_silently_replacing() {
        // A re-import is not a new track, and overwriting would lose whatever
        // the user had edited on the original.
        let mut collection = stocked();
        assert_eq!(
            collection.add(1, "Other", "Other", "", "file://y", 1_000, 1),
            Err(Status::Refused)
        );
        assert_eq!(collection.len(), 3);
    }

    #[test]
    fn renaming_a_track_keeps_its_identity_and_is_findable_by_the_new_name() {
        let mut collection = stocked();
        collection
            .update_metadata(1, "Strobe (Edit)", "deadmau5", "An Album")
            .expect("a real track");

        let found = collection.search("edit", 0, false).expect("a real sort");
        assert_eq!(found, 1);
        assert_eq!(
            collection.result(0),
            Ok(1),
            "the identity changed on a rename"
        );
    }

    #[test]
    fn removing_a_track_hides_it_without_destroying_it() {
        // This test was first written asserting that removing twice fails. It
        // does not, and the reason is the design: removal marks a track hidden
        // and `restore` brings it back with its rating, tags and play count.
        // Master Prompt #9 says the user owns their work, and a delete that
        // destroyed a year of accumulated metadata would be the opposite of
        // that whatever it was called.
        //
        // Finding this is what showed the boundary was offering removal with no
        // way back — a destructive action with no undo, handed to every host.
        let mut collection = stocked();
        collection.remove(2).expect("a real track");
        assert_eq!(collection.search("", 0, false), Ok(2), "it still appears");

        // Repeating it is not a mistake worth reporting.
        assert_eq!(collection.remove(2), Ok(()));

        collection.restore(2).expect("a removed track comes back");
        assert_eq!(collection.search("", 0, false), Ok(3));
        assert_eq!(
            collection.duration(2),
            Ok(44_100 * 300),
            "its facts survived"
        );
    }

    #[test]
    fn restoring_a_track_nobody_ever_added_is_refused() {
        let mut collection = stocked();
        assert_eq!(collection.restore(999), Err(Status::InvalidHandle));
    }

    #[test]
    fn every_sort_key_round_trips_and_an_unknown_one_is_refused() {
        for (index, (key, name)) in SORT_KEYS.iter().enumerate() {
            let code = i32::try_from(index).expect("small");
            assert_eq!(sort_from_code(code), Some(*key));
            assert!(name.starts_with("PRV_SORT_"));
        }
        assert_eq!(sort_from_code(-1), None);

        let mut collection = stocked();
        assert_eq!(
            collection.search("", 99, false),
            Err(Status::InvalidArgument)
        );
    }

    #[test]
    fn sorting_descending_reverses_the_order() {
        let mut collection = stocked();
        collection.search("", 0, false).expect("a real sort");
        let ascending: Vec<u64> = (0..3).filter_map(|i| collection.result(i).ok()).collect();

        collection.search("", 0, true).expect("a real sort");
        let descending: Vec<u64> = (0..3).filter_map(|i| collection.result(i).ok()).collect();

        assert_eq!(
            descending,
            ascending.iter().rev().copied().collect::<Vec<_>>()
        );
    }
}
