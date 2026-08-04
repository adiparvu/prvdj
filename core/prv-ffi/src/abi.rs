//! What version of the boundary a host is talking to.
//!
//! # Why a host must ask
//!
//! The architecture overview requires calls across the language boundary to be
//! *versioned*. A statically linked host cannot get this wrong, but a host that
//! loads the library at run time — every plugin arrangement in ADR-0005, and
//! every developer with a stale build in their library path — absolutely can,
//! and the failure without a version check is a silent one: a function whose
//! arguments moved is still a function, and it still returns.
//!
//! So the first call any host makes is [`version`], and it compares against the
//! numbers it was compiled with. That check is worth more than every other
//! defence in this crate, because it is the one that runs before any pointer has
//! been handed over.
//!
//! # What the parts mean
//!
//! - **Major** changes when an existing call changes meaning, changes signature,
//!   or is withdrawn. A host built for major *n* must refuse major *n+1*.
//! - **Minor** changes when a call is added and nothing existing moves. A host
//!   built for minor *n* works against minor *n+m*; it simply does not use what
//!   it does not know about.
//! - **Patch** changes when only behaviour inside an unchanged contract changes.
//!
//! The packed form is `major << 16 | minor << 8 | patch`, which fits a
//! `uint32_t` and compares in the order a reader expects.

/// The major version. Incompatible when it differs.
pub const MAJOR: u32 = 1;

/// The minor version. Additive.
///
/// Raised to 1 when the planner was added, to 2 when analysis was, to 3 for consent and entitlement, to 4 for the collection, to 5 for delivery, to 6 for settings and notifications, to 7 for synchronisation, to 8 for carrying what a newer build made, to 9 for neighbours, to 10 for the synchronisation state, and to 11 for reading the timeline. Every call that existed at 1.0 has
/// the same signature and the same meaning, which is exactly what a minor
/// version promises — a host built against 1.0 keeps working and simply does
/// not plan.
pub const MINOR: u32 = 11;

/// The patch version.
pub const PATCH: u32 = 0;

/// How many bits each field is shifted by in the packed form.
const MAJOR_SHIFT: u32 = 16;
const MINOR_SHIFT: u32 = 8;

/// The largest value a minor or patch field can hold before it would collide
/// with the field above it.
const FIELD_LIMIT: u32 = 256;

/// A minor version of 256 would read back as a major version bump, which is the
/// exact mistake the version exists to prevent.
///
/// Checked at compile time rather than in a test. This began as a test and was
/// moved: an invariant that cannot be violated is worth more than one that is
/// merely noticed, and a build that refuses to produce a broken version number
/// is a stronger promise than a suite that reports one afterwards.
const _: () = assert!(MINOR < FIELD_LIMIT && PATCH < FIELD_LIMIT);

/// The packed version a host compares against its own.
#[must_use]
pub const fn version() -> u32 {
    (MAJOR << MAJOR_SHIFT) | (MINOR << MINOR_SHIFT) | PATCH
}

/// Whether a host built against `host_major` can use this library.
///
/// Deliberately not "whether the versions are equal". A host that refused every
/// library but its exact build would make adding a single call a breaking change
/// for everyone, which is how a boundary ossifies.
#[must_use]
pub const fn is_compatible_with(host_major: u32) -> bool {
    host_major == MAJOR
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::panic,
        reason = "a test that cannot build its own fixture should fail loudly"
    )]

    use super::*;

    #[test]
    fn the_packed_form_takes_apart_into_what_went_into_it() {
        let packed = version();
        assert_eq!(packed >> MAJOR_SHIFT, MAJOR);
        assert_eq!((packed >> MINOR_SHIFT) & 0xFF, MINOR);
        assert_eq!(packed & 0xFF, PATCH);
    }

    #[test]
    fn a_later_version_compares_greater() {
        // What a host actually does with the number.
        let this = version();
        let next_patch = (MAJOR << MAJOR_SHIFT) | (MINOR << MINOR_SHIFT) | (PATCH + 1);
        let next_minor = (MAJOR << MAJOR_SHIFT) | ((MINOR + 1) << MINOR_SHIFT) | PATCH;
        let next_major = ((MAJOR + 1) << MAJOR_SHIFT) | (MINOR << MINOR_SHIFT) | PATCH;

        assert!(next_patch > this);
        assert!(next_minor > next_patch);
        assert!(next_major > next_minor);
    }

    #[test]
    fn a_host_from_another_major_version_is_turned_away() {
        assert!(is_compatible_with(MAJOR));
        assert!(!is_compatible_with(MAJOR + 1));
        assert!(!is_compatible_with(0));
    }

    #[test]
    fn the_version_is_not_zero() {
        // A host that reads zero has almost certainly failed to resolve the
        // symbol at all, so zero must never be a version this library returns.
        assert_ne!(version(), 0);
    }
}
