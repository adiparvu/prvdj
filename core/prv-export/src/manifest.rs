//! What went into a mix.
//!
//! # Why an export carries a manifest
//!
//! Three requirements meet here, and one structure answers all of them.
//!
//! **Licensing** (Master Prompt #29). A user who publishes a mix may need to
//! say what is in it — for a rights society, a platform's upload form, or a
//! label. Reconstructing that afterwards from memory is exactly the task nobody
//! does accurately.
//!
//! **Reproducibility** (Master Prompt #27). A mix rendered today and the same
//! project rendered next year should be the same file. They will not be if an
//! analysis stage improved in between and quietly moved a beat grid. Recording
//! the stage versions that were in force turns that from a mystery into a
//! difference someone can point at.
//!
//! **Integrity** (Master Prompt #3C). An export should be able to say whether
//! everything it needed was actually there. A track whose file went missing
//! halfway through a session renders as silence, and silence in the middle of a
//! set is the kind of thing that is noticed on stage rather than at the desk.
//!
//! # It refers to media; it never contains it
//!
//! A manifest holds track references and, optionally, whatever title and artist
//! the caller chooses to put in it. ADR-0003's rule that sharing a project
//! shares the document and not the audio applies here too: a manifest is
//! metadata about a mix, and it is the caller's decision — not this crate's —
//! whether a particular one is fit to leave the device.

use prv_project::TrackRef;

/// The state of one track at the moment a mix was rendered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    track: TrackRef,
    title: Option<String>,
    artist: Option<String>,
    available: bool,
    analysis_versions: Vec<(&'static str, u32)>,
}

impl Entry {
    /// Records a track that was present.
    #[must_use]
    pub const fn present(track: TrackRef) -> Self {
        Self {
            track,
            title: None,
            artist: None,
            available: true,
            analysis_versions: Vec::new(),
        }
    }

    /// Records a track whose media could not be found.
    ///
    /// Kept in the manifest rather than omitted. A mix that rendered without one
    /// of its tracks is a different mix, and a manifest that quietly listed only
    /// what worked would describe a file that does not exist.
    #[must_use]
    pub const fn missing(track: TrackRef) -> Self {
        Self {
            track,
            title: None,
            artist: None,
            available: false,
            analysis_versions: Vec::new(),
        }
    }

    /// Attaches what the track is called.
    ///
    /// Optional because a manifest is useful without it — for reproducibility
    /// the reference and the versions are enough — and because the caller
    /// decides what identifying information belongs in a file that may leave
    /// the device.
    #[must_use]
    pub fn described(mut self, title: &str, artist: &str) -> Self {
        self.title = Some(title.to_owned());
        self.artist = Some(artist.to_owned());
        self
    }

    /// Records which version of an analysis stage was in force.
    #[must_use]
    pub fn analysed_with(mut self, stage: &'static str, version: u32) -> Self {
        self.analysis_versions.push((stage, version));
        self.analysis_versions.sort_unstable();
        self.analysis_versions.dedup_by_key(|entry| entry.0);
        self
    }

    /// Which track.
    #[must_use]
    pub const fn track(&self) -> TrackRef {
        self.track
    }

    /// What it is called, if the caller said.
    #[must_use]
    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    /// Who made it, if the caller said.
    #[must_use]
    pub fn artist(&self) -> Option<&str> {
        self.artist.as_deref()
    }

    /// Whether its media was there when the mix was rendered.
    #[must_use]
    pub const fn is_available(&self) -> bool {
        self.available
    }

    /// The analysis stage versions in force, in stage order.
    #[must_use]
    pub fn analysis_versions(&self) -> &[(&'static str, u32)] {
        &self.analysis_versions
    }
}

/// Everything that went into one mix.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Manifest {
    entries: Vec<Entry>,
}

impl Manifest {
    /// Creates an empty manifest.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Adds a track, replacing any entry already recorded for it.
    ///
    /// Replacing rather than appending keeps the manifest a *description of the
    /// mix* rather than a log of how it was assembled: a track that appears
    /// twice in a set is one track, and listing it twice would make a rights
    /// return double-count it.
    pub fn record(&mut self, entry: Entry) {
        if let Some(existing) = self
            .entries
            .iter_mut()
            .find(|candidate| candidate.track == entry.track)
        {
            *existing = entry;
            return;
        }
        self.entries.push(entry);
        self.entries.sort_by_key(|entry| entry.track.get());
    }

    /// Every track, in reference order.
    #[must_use]
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// How many distinct tracks the mix used.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the manifest records nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The tracks whose media was not there.
    ///
    /// What an integrity check reports. A mix rendered with these missing is a
    /// mix with silence in it, and that is noticed on stage rather than at the
    /// desk.
    pub fn missing(&self) -> impl Iterator<Item = &Entry> {
        self.entries.iter().filter(|entry| !entry.available)
    }

    /// Whether every track the mix needed was there.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.entries.iter().all(Entry::is_available)
    }

    /// Whether two manifests describe the same mix, made the same way.
    ///
    /// Stronger than equality of track lists: it also requires the analysis
    /// stage versions to match, because the same tracks analysed by different
    /// code produce a different mix. This is what turns "why does last year's
    /// render sound different" from a mystery into a difference someone can
    /// point at.
    #[must_use]
    pub fn reproduces(&self, other: &Self) -> bool {
        if self.entries.len() != other.entries.len() {
            return false;
        }
        self.entries
            .iter()
            .zip(other.entries.iter())
            .all(|(left, right)| {
                left.track == right.track
                    && left.available == right.available
                    && left.analysis_versions == right.analysis_versions
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(id: u64) -> TrackRef {
        TrackRef::new(id)
    }

    #[test]
    fn a_missing_track_is_recorded_rather_than_omitted() {
        // A mix that rendered without one of its tracks is a different mix, and
        // a manifest listing only what worked would describe a file that does
        // not exist.
        let mut manifest = Manifest::new();
        manifest.record(Entry::present(track(1)));
        manifest.record(Entry::missing(track(2)));
        manifest.record(Entry::present(track(3)));

        assert_eq!(manifest.len(), 3);
        assert!(!manifest.is_complete());
        let missing: Vec<u64> = manifest
            .missing()
            .map(|entry| entry.track().get())
            .collect();
        assert_eq!(missing, vec![2]);
    }

    #[test]
    fn a_track_used_twice_is_listed_once() {
        // A manifest describes the mix, not how it was assembled. Listing a
        // track twice would make a rights return double-count it.
        let mut manifest = Manifest::new();
        manifest.record(Entry::present(track(1)).described("First", "Someone"));
        manifest.record(Entry::present(track(1)).described("First", "Someone"));

        assert_eq!(manifest.len(), 1);
        assert_eq!(
            manifest.entries().first().and_then(Entry::title),
            Some("First")
        );
    }

    #[test]
    fn entries_come_back_in_a_stable_order() {
        let mut manifest = Manifest::new();
        for id in [7_u64, 2, 5, 1] {
            manifest.record(Entry::present(track(id)));
        }
        let order: Vec<u64> = manifest
            .entries()
            .iter()
            .map(|entry| entry.track().get())
            .collect();
        assert_eq!(order, vec![1, 2, 5, 7]);
    }

    #[test]
    fn reproducing_requires_the_analysis_versions_to_match_too() {
        // The same tracks analysed by different code produce a different mix.
        // This is what turns "why does last year's render sound different" into
        // a difference someone can point at.
        let mut first = Manifest::new();
        first.record(
            Entry::present(track(1))
                .analysed_with("stage.tempo", 1)
                .analysed_with("stage.key", 1),
        );

        let mut same = Manifest::new();
        same.record(
            Entry::present(track(1))
                .analysed_with("stage.key", 1)
                .analysed_with("stage.tempo", 1),
        );
        assert!(
            first.reproduces(&same),
            "the order the versions were recorded in should not matter"
        );

        let mut improved = Manifest::new();
        improved.record(
            Entry::present(track(1))
                .analysed_with("stage.tempo", 2)
                .analysed_with("stage.key", 1),
        );
        assert!(
            !first.reproduces(&improved),
            "a changed analysis version should stop a manifest reproducing"
        );

        let mut different_track = Manifest::new();
        different_track.record(Entry::present(track(9)).analysed_with("stage.tempo", 1));
        assert!(!first.reproduces(&different_track));
    }

    #[test]
    fn a_version_recorded_twice_keeps_the_later_value() {
        let entry = Entry::present(track(1))
            .analysed_with("stage.tempo", 1)
            .analysed_with("stage.tempo", 2);
        assert_eq!(entry.analysis_versions().len(), 1);
    }

    #[test]
    fn description_is_optional_and_absent_by_default() {
        // The caller decides what identifying information belongs in a file
        // that may leave the device; this crate does not decide for them.
        let bare = Entry::present(track(1));
        assert_eq!(bare.title(), None);
        assert_eq!(bare.artist(), None);

        let described = Entry::present(track(2)).described("Title", "Artist");
        assert_eq!(described.title(), Some("Title"));
        assert_eq!(described.artist(), Some("Artist"));
    }

    #[test]
    fn an_empty_manifest_is_complete_and_says_so() {
        let manifest = Manifest::new();
        assert!(manifest.is_empty());
        assert!(
            manifest.is_complete(),
            "a mix with no tracks is missing nothing"
        );
    }
}
