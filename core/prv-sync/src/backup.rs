//! Which points in a project's history are worth keeping.
//!
//! # There is nothing to build a restore point *out of*
//!
//! `prv-project` already makes them: a named version is a position in the log,
//! and branching from one costs nothing. What was missing is the policy — how
//! many to keep, and which to let go — and a policy is the part that decides
//! whether a user finds what they are looking for a month later.
//!
//! # Thinning, not truncating
//!
//! The obvious rule is "keep the last fifty". It is also the rule that loses the
//! only version anybody wanted: a user who worked on a set in March and comes
//! back in June has fifty automatic points from June and nothing from March.
//!
//! So the points are thinned by *age band*: everything recent, then one per
//! hour, then one per day, then one per week. The count stays bounded and the
//! history stays legible — which is what somebody scrolling back is actually
//! looking for.
//!
//! # A named point is never discarded
//!
//! Somebody typed a name. That is the strongest signal available that a version
//! matters, and no automatic rule outranks it. If the named points alone exceed
//! the bound, the bound gives way — Master Prompt #9 says the user owns their
//! work, and a retention policy that deletes something they deliberately kept is
//! not a policy, it is data loss with a schedule.

use core::fmt;

/// Why a restore point exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum PointKind {
    /// Made automatically as the user worked.
    Automatic,
    /// Made because the user asked, and usually named.
    ///
    /// Never discarded by thinning. Somebody typed a name.
    Deliberate,
}

impl PointKind {
    /// A stable identifier, for storage and localisation.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Automatic => "point.automatic",
            Self::Deliberate => "point.deliberate",
        }
    }
}

/// A position in a project's history, worth being able to go back to.
///
/// `age_seconds` is supplied by the caller rather than computed: ADR-0001 keeps
/// the core away from the clock, and the platform is the only layer that knows
/// what time it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RestorePoint {
    /// Where in the operation log it sits.
    pub position: usize,
    /// How long ago it was made, in seconds.
    pub age_seconds: u64,
    /// Why it exists.
    pub kind: PointKind,
}

/// How much history to keep.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetentionPolicy {
    /// Points younger than this are all kept, in seconds.
    pub keep_everything_under: u64,
    /// Above that, one per this many seconds, up to `thin_hourly_until`.
    pub hourly: u64,
    /// Above that, one per day.
    pub daily: u64,
    /// The most automatic points to keep in total.
    pub maximum_automatic: usize,
}

impl Default for RetentionPolicy {
    fn default() -> Self {
        Self::STANDARD
    }
}

impl RetentionPolicy {
    /// The policy the product ships with.
    ///
    /// Everything from the last hour, then one an hour for a day, then one a
    /// day — bounded at 64 automatic points. Sixty-four is enough that a user
    /// scrolling back finds the shape of a month's work, and small enough that
    /// the list is readable without a search box.
    pub const STANDARD: Self = Self {
        keep_everything_under: 3_600,
        hourly: 3_600,
        daily: 86_400,
        maximum_automatic: 64,
    };

    /// Which band a point of a given age falls into.
    ///
    /// Points in the same band are interchangeable to the policy, so exactly one
    /// of them is kept — the newest, because it is the one whose work the next
    /// point builds on.
    #[allow(
        clippy::integer_division,
        reason = "truncation is the banding: everything within the same hour must \
                  produce the same number, which is what the remainder being \
                  discarded means"
    )]
    const fn band(self, age_seconds: u64) -> u64 {
        if age_seconds < self.keep_everything_under {
            // Every recent point is its own band, so none of them is thinned.
            age_seconds
        } else if age_seconds < self.daily {
            let step = if self.hourly == 0 { 1 } else { self.hourly };
            self.keep_everything_under
                .saturating_add(age_seconds / step)
        } else {
            let step = if self.daily == 0 { 1 } else { self.daily };
            self.keep_everything_under
                .saturating_add(self.daily)
                .saturating_add(age_seconds / step)
        }
    }
}

/// Decides which points to keep.
///
/// Returns them oldest last — newest first — because that is the order a user
/// reads a history in, and building the list in display order means nothing
/// downstream has to remember to reverse it.
///
/// Deliberate points are all kept. Automatic ones are thinned by age band and
/// then bounded, newest first.
#[must_use]
pub fn thin(points: &[RestorePoint], policy: RetentionPolicy) -> Vec<RestorePoint> {
    let mut sorted: Vec<RestorePoint> = points.to_vec();
    sorted.sort_by_key(|point| (point.age_seconds, point.position));

    let mut kept: Vec<RestorePoint> = Vec::with_capacity(sorted.len());
    let mut bands: Vec<u64> = Vec::new();
    let mut automatic = 0_usize;

    for point in sorted {
        if point.kind == PointKind::Deliberate {
            // No automatic rule outranks somebody having typed a name. If these
            // alone exceed the bound, the bound gives way.
            kept.push(point);
            continue;
        }
        if automatic >= policy.maximum_automatic {
            continue;
        }
        let band = policy.band(point.age_seconds);
        if bands.contains(&band) {
            continue;
        }
        bands.push(band);
        kept.push(point);
        automatic += 1;
    }

    kept
}

impl fmt::Display for RestorePoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} at {} ({}s ago)",
            self.kind.key(),
            self.position,
            self.age_seconds
        )
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::panic,
        reason = "a test that cannot build its own fixture should fail loudly"
    )]

    use super::*;

    fn automatic(position: usize, age_seconds: u64) -> RestorePoint {
        RestorePoint {
            position,
            age_seconds,
            kind: PointKind::Automatic,
        }
    }

    fn deliberate(position: usize, age_seconds: u64) -> RestorePoint {
        RestorePoint {
            position,
            age_seconds,
            kind: PointKind::Deliberate,
        }
    }

    #[test]
    fn a_point_somebody_named_is_never_discarded() {
        // The strongest signal available that a version matters. A retention
        // policy that deletes something the user deliberately kept is not a
        // policy, it is data loss with a schedule.
        let mut points: Vec<RestorePoint> = (0..500)
            .map(|index| automatic(index, index as u64 * 600))
            .collect();
        points.push(deliberate(9_999, 10_000_000));

        let kept = thin(&points, RetentionPolicy::STANDARD);
        assert!(
            kept.iter().any(|point| point.position == 9_999),
            "a named point four months old was thinned away"
        );
    }

    #[test]
    fn the_named_points_alone_may_exceed_the_bound() {
        // If they do, the bound gives way. Master Prompt #9: the user owns their
        // work.
        let points: Vec<RestorePoint> = (0..200)
            .map(|index| deliberate(index, index as u64 * 86_400))
            .collect();

        let kept = thin(&points, RetentionPolicy::STANDARD);
        assert_eq!(kept.len(), 200);
    }

    #[test]
    fn everything_from_the_last_hour_survives() {
        // Recent work is what a user is most likely to want back, and the whole
        // point of an automatic point is the one made two minutes before the
        // mistake.
        let points: Vec<RestorePoint> = (0..30)
            .map(|index| automatic(index, index as u64 * 60))
            .collect();

        let kept = thin(&points, RetentionPolicy::STANDARD);
        assert_eq!(
            kept.len(),
            30,
            "a point from the last half hour was thinned"
        );
    }

    #[test]
    fn a_month_of_work_stays_legible_rather_than_becoming_a_wall() {
        // The rule "keep the last fifty" loses the only version anybody wanted:
        // a user who worked in March and comes back in June has fifty points
        // from June and nothing from March.
        let mut points = Vec::new();
        // Three hundred points spread across thirty days.
        for index in 0..300_usize {
            points.push(automatic(index, index as u64 * 8_640));
        }

        let kept = thin(&points, RetentionPolicy::STANDARD);
        assert!(kept.len() <= RetentionPolicy::STANDARD.maximum_automatic);
        assert!(!kept.is_empty());

        // And the oldest kept point is genuinely old, rather than everything
        // surviving from one afternoon.
        let oldest = kept
            .iter()
            .map(|point| point.age_seconds)
            .max()
            .expect("something was kept");
        assert!(
            oldest > 86_400 * 7,
            "the oldest point kept is only {oldest} seconds old"
        );
    }

    #[test]
    fn thinning_twice_changes_nothing() {
        // What makes it safe to run on every save. A rule that kept thinning
        // would eat the history one launch at a time.
        let points: Vec<RestorePoint> = (0..200)
            .map(|index| automatic(index, index as u64 * 1_800))
            .collect();

        let once = thin(&points, RetentionPolicy::STANDARD);
        let twice = thin(&once, RetentionPolicy::STANDARD);
        assert_eq!(once, twice);
    }

    #[test]
    fn the_newest_point_is_always_kept() {
        // The one somebody just made, which is the one they are most likely to
        // be reaching for.
        let points: Vec<RestorePoint> = (0..100)
            .map(|index| automatic(index, index as u64 * 3_600))
            .collect();

        let kept = thin(&points, RetentionPolicy::STANDARD);
        assert_eq!(
            kept.first().map(|point| point.position),
            Some(0),
            "the newest point was thinned"
        );
    }

    #[test]
    fn the_result_is_in_the_order_a_user_reads_it() {
        // Newest first. Building the list in display order means nothing
        // downstream has to remember to reverse it.
        let points = vec![
            automatic(3, 300_000),
            automatic(1, 100),
            deliberate(2, 5_000),
        ];
        let kept = thin(&points, RetentionPolicy::STANDARD);
        let ages: Vec<u64> = kept.iter().map(|point| point.age_seconds).collect();
        let mut sorted = ages.clone();
        sorted.sort_unstable();
        assert_eq!(ages, sorted);
    }

    #[test]
    fn nothing_thins_to_nothing() {
        assert!(thin(&[], RetentionPolicy::STANDARD).is_empty());
    }

    #[test]
    fn kind_keys_are_distinct() {
        assert_ne!(PointKind::Automatic.key(), PointKind::Deliberate.key());
        assert_eq!(RetentionPolicy::default(), RetentionPolicy::STANDARD);
    }
}
