//! The music library.
//!
//! # What this module is, and is not
//!
//! Module Specification #001 draws the boundary sharply, and the list of things
//! the library must *not* do is the more useful half: no playback, no signal
//! processing, no mixing, no effects, no mastering, no waveform generation, no
//! artificial-intelligence orchestration, no synchronisation logic. It stores,
//! indexes, organises and exposes. Nothing else.
//!
//! That restraint is what keeps the module every other subsystem depends on from
//! becoming the place where everything ends up.
//!
//! # It refers to audio; it never reads it
//!
//! ADR-0001 makes the core free of input and output. The library therefore holds
//! a *reference* to media and metadata the host has already extracted. Opening
//! files, decoding tags and computing fingerprints happen in the platform layer;
//! what arrives here is already data.
//!
//! The same rule applies to analysis. A track holds an
//! [`AnalysisRef`](track::AnalysisRef), not a copy of its tempo and key. Master
//! Prompt #20 versions each analysis stage independently so that improving key
//! detection re-runs key detection alone; if the library held its own copy of the
//! result, every such improvement would become a migration of the library
//! schema, and the two modules would be inseparable.
//!
//! # Nothing is deleted
//!
//! Removing a track hides it and keeps everything the user built around it —
//! playlists, ratings, tags, notes, play counts. Master Prompt #9 forbids
//! permanent deletion by default, and the reason is concrete: a track whose file
//! moved is not a track the user wanted to lose, and someone who spent an evening
//! rating five hundred records should not lose that work because a drive was
//! unplugged.
//!
//! # Sized for a real library
//!
//! Module Specification #001 names 10 000, 50 000 and 100 000 tracks, and
//! requires search to feel instantaneous at all three. Text search runs against a
//! prefix index rather than by scanning, so it costs what the *answer* costs
//! rather than what the library costs.

mod library;
mod query;
mod track;

pub use library::{DuplicateGroup, DuplicateSignal, Library, LibraryError, LibraryStats};
pub use query::{AnalysisFacts, Filter, Query, SortKey, SortOrder};
pub use track::{
    AnalysisRef, ArtworkRef, Fingerprint, MediaRef, Rating, Track, TrackId, TrackStatus,
};
