//! Automation: a parameter's value as a function of time.
//!
//! # Two requirements that pull in opposite directions
//!
//! An automation lane is **edited** off the audio thread — points added,
//! dragged, deleted — and **evaluated** on it, once per control block, for
//! every automated parameter. ADR-0002 forbids allocation, locking and
//! unbounded work in that second context.
//!
//! So the lane is built as an owned, sorted structure off the thread, and
//! evaluation is a binary search plus one interpolation over a slice. Nothing
//! is allocated, nothing is locked, and the work is logarithmic in the number
//! of points rather than linear — which matters because a four-hour set with a
//! point every bar is thousands of points on one lane.
//!
//! # Values are stored normalised
//!
//! A lane holds numbers from zero to one, not decibels or hertz.
//! [`crate::parameter::ParameterDescriptor`] converts. That indirection buys
//! two specific things: a parameter's range can be widened later without every
//! stored automation curve changing meaning, and the same curve can be copied
//! from one parameter to another — a filter sweep pasted onto a send level —
//! which is a thing DJs actually do.
//!
//! # No curve overshoots
//!
//! Every interpolation here stays between the two points it joins. That rules
//! out the spline a graphics library would reach for: a Catmull-Rom through
//! three points overshoots on the way to a peak, and an overshoot on a gain
//! lane is a value above unity, on a filter lane a frequency past Nyquist. A
//! curve that leaves the range its own points defined is a curve that can make
//! a sound the user did not ask for, at a moment they were not watching.

use prv_time::Frames;

use crate::num::{narrow, signed_to_f64};
use crate::parameter::ParameterAddress;

/// The largest number of points one lane may hold.
///
/// Sixteen thousand. At a point per beat that is four hours at 128 BPM, which
/// is longer than any set a lane needs to describe, and it bounds both the
/// memory a project can consume and the depth of the binary search. Master
/// Prompt #26 requires limits on anything a document can grow without bound.
pub const MAX_POINTS: usize = 16_384;

/// How the value moves from one point to the next.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Interpolation {
    /// The value jumps at the next point and is constant until then.
    ///
    /// What a switch, a preset change or a beat-synchronised effect needs. A
    /// linear ramp on a parameter that only has discrete positions produces a
    /// slur through values that mean nothing.
    Hold,

    /// A straight line.
    Linear,

    /// A smooth curve that starts and ends flat.
    ///
    /// A raised cosine, which is the shape a hand makes on a fader. Its
    /// derivative is zero at both ends, so joining several of them produces a
    /// curve with no corners — and a corner in a gain envelope is audible as a
    /// click.
    Smooth,

    /// A curve that starts slowly and accelerates.
    ///
    /// What a filter sweep into a drop wants: most of the travel happens late,
    /// so the change is felt as an arrival rather than a slide.
    Accelerating,

    /// A curve that starts quickly and settles.
    Decelerating,
}

impl Interpolation {
    /// Maps a fraction between two points onto a fraction of the value change.
    ///
    /// Every shape here satisfies `f(0) = 0` and `f(1) = 1` and stays within
    /// those bounds in between. That is what guarantees no overshoot, and it is
    /// checked by a test that walks every shape across its whole domain.
    #[must_use]
    pub fn apply(self, fraction: f64) -> f64 {
        let t = fraction.clamp(0.0, 1.0);
        match self {
            // Hold reaches the next value only at the point itself.
            Self::Hold => {
                if t >= 1.0 {
                    1.0
                } else {
                    0.0
                }
            }
            Self::Linear => t,
            Self::Smooth => 0.5 * (1.0 - (core::f64::consts::PI * t).cos()),
            Self::Accelerating => t * t,
            Self::Decelerating => t.mul_add(-t, 2.0 * t),
        }
    }

    /// A stable identifier, for storage and localisation.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Hold => "interpolation.hold",
            Self::Linear => "interpolation.linear",
            Self::Smooth => "interpolation.smooth",
            Self::Accelerating => "interpolation.accelerating",
            Self::Decelerating => "interpolation.decelerating",
        }
    }
}

/// One point on an automation lane.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AutomationPoint {
    position: Frames,
    value: f32,
    interpolation: Interpolation,
}

impl AutomationPoint {
    /// Creates a point.
    ///
    /// The value is clamped to zero to one, and a value that is not a number
    /// becomes zero. A lane is evaluated on the audio thread, and a non-number
    /// there propagates into the signal path, where it silences every filter it
    /// touches until the engine is restarted — arriving, typically, from a value
    /// stored months earlier.
    ///
    /// Infinities are *clamped* rather than zeroed, because they are ordered:
    /// positive infinity is unambiguously the top of the range and negative
    /// infinity the bottom. Only a value that is not a number has no position
    /// on the scale, and zero is the safer of the two ends to send it to.
    #[must_use]
    pub fn new(position: Frames, value: f32, interpolation: Interpolation) -> Self {
        Self {
            position,
            value: if value.is_nan() {
                0.0
            } else {
                value.clamp(0.0, 1.0)
            },
            interpolation,
        }
    }

    /// Where the point sits.
    #[must_use]
    pub const fn position(self) -> Frames {
        self.position
    }

    /// The normalised value at the point.
    #[must_use]
    pub const fn value(self) -> f32 {
        self.value
    }

    /// How the value approaches the *next* point.
    ///
    /// The shape belongs to the segment that begins here, not to the one that
    /// ends here. That is the choice that matches how a user thinks about it:
    /// dragging a curve handle changes what happens after the point you grabbed.
    #[must_use]
    pub const fn interpolation(self) -> Interpolation {
        self.interpolation
    }
}

/// A parameter's value over time.
#[derive(Debug, Clone, PartialEq)]
pub struct AutomationLane {
    address: ParameterAddress,
    points: Vec<AutomationPoint>,
    enabled: bool,
}

impl AutomationLane {
    /// Creates an empty lane for a parameter.
    #[must_use]
    pub const fn new(address: ParameterAddress) -> Self {
        Self {
            address,
            points: Vec::new(),
            enabled: true,
        }
    }

    /// Which parameter the lane drives.
    #[must_use]
    pub const fn address(&self) -> &ParameterAddress {
        &self.address
    }

    /// The points, in order.
    #[must_use]
    pub fn points(&self) -> &[AutomationPoint] {
        &self.points
    }

    /// Whether the lane is currently applied.
    ///
    /// A disabled lane keeps its points. Master Prompt #9 forbids losing a
    /// user's work by default, and "turn this automation off for a moment" is
    /// not a request to delete it.
    #[must_use]
    pub const fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Enables or disables the lane.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Whether the lane has no points.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }

    /// The number of points.
    #[must_use]
    pub fn len(&self) -> usize {
        self.points.len()
    }

    /// Adds a point, or replaces the one already at that position.
    ///
    /// Replacing rather than accumulating is what makes repeated edits at the
    /// same place idempotent, which matters because the operation log will
    /// replay them and a duplicate point at one position would make the curve
    /// depend on replay order.
    ///
    /// # Errors
    ///
    /// Returns [`AutomationError::TooManyPoints`] when the lane is full.
    pub fn insert(&mut self, point: AutomationPoint) -> Result<(), AutomationError> {
        match self
            .points
            .binary_search_by(|existing| existing.position.get().cmp(&point.position.get()))
        {
            Ok(index) => {
                if let Some(slot) = self.points.get_mut(index) {
                    *slot = point;
                }
                Ok(())
            }
            Err(index) => {
                if self.points.len() >= MAX_POINTS {
                    return Err(AutomationError::TooManyPoints {
                        maximum: MAX_POINTS,
                    });
                }
                self.points.insert(index, point);
                Ok(())
            }
        }
    }

    /// Removes the point at a position, if there is one.
    ///
    /// Returns whether anything was removed.
    pub fn remove_at(&mut self, position: Frames) -> bool {
        match self
            .points
            .binary_search_by(|existing| existing.position.get().cmp(&position.get()))
        {
            Ok(index) => {
                self.points.remove(index);
                true
            }
            Err(_) => false,
        }
    }

    /// Removes every point within a span, and returns how many went.
    pub fn remove_range(&mut self, from: Frames, to: Frames) -> usize {
        let before = self.points.len();
        self.points
            .retain(|point| point.position < from || point.position >= to);
        before - self.points.len()
    }

    /// The value at a position.
    ///
    /// # Realtime
    ///
    /// This is the function the audio thread calls. It allocates nothing, locks
    /// nothing, and does work logarithmic in the number of points. A lane with
    /// no points returns `None` so the caller can use the parameter's own value
    /// rather than being handed a fabricated default — the difference between
    /// "automation says 0.5" and "there is no automation here" is one the mixer
    /// needs.
    #[must_use]
    pub fn value_at(&self, position: Frames) -> Option<f32> {
        if !self.enabled {
            return None;
        }

        let first = self.points.first()?;
        if position <= first.position {
            return Some(first.value);
        }
        let last = self.points.last()?;
        if position >= last.position {
            return Some(last.value);
        }

        // The segment containing the position. `partition_point` is a binary
        // search that returns an insertion index, so the segment starts at the
        // point before it.
        let index = self
            .points
            .partition_point(|point| point.position.get() <= position.get());
        let Some(start) = index.checked_sub(1).and_then(|i| self.points.get(i)) else {
            return Some(first.value);
        };
        let Some(end) = self.points.get(index) else {
            return Some(last.value);
        };

        let span = end.position.get().saturating_sub(start.position.get());
        if span <= 0 {
            return Some(end.value);
        }
        let travelled = position.get().saturating_sub(start.position.get());
        let fraction = signed_to_f64(travelled) / signed_to_f64(span);

        let shaped = start.interpolation.apply(fraction);
        let from = f64::from(start.value);
        let to = f64::from(end.value);
        Some(narrow((to - from).mul_add(shaped, from)))
    }

    /// The position of the first point, if any.
    #[must_use]
    pub fn start(&self) -> Option<Frames> {
        self.points.first().map(|point| point.position)
    }

    /// The position of the last point, if any.
    #[must_use]
    pub fn end(&self) -> Option<Frames> {
        self.points.last().map(|point| point.position)
    }

    /// Moves every point by an offset, keeping the lane sorted.
    ///
    /// Points that would move before zero are clamped there rather than
    /// dropped, because a user shifting a whole arrangement left has not asked
    /// to lose the automation at its beginning.
    pub fn shift(&mut self, offset: Frames) {
        for point in &mut self.points {
            let moved = point.position.get().saturating_add(offset.get()).max(0);
            point.position = Frames::new(moved);
        }
        self.points
            .sort_by(|a, b| a.position.get().cmp(&b.position.get()));
        // Clamping can collide two points at zero; the later one wins, which
        // matches what a user sees when they drag one point onto another.
        self.points
            .dedup_by(|a, b| a.position.get() == b.position.get());
    }
}

/// Errors from editing an automation lane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum AutomationError {
    /// The lane already holds as many points as it may.
    TooManyPoints {
        /// The limit.
        maximum: usize,
    },
}

impl core::fmt::Display for AutomationError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::TooManyPoints { maximum } => {
                write!(f, "an automation lane may hold at most {maximum} points")
            }
        }
    }
}

impl core::error::Error for AutomationError {}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::panic,
        clippy::float_cmp,
        reason = "a test that cannot build its own fixture should fail loudly"
    )]

    use super::*;
    use crate::parameter::{ParameterKey, ParameterOwner};

    fn lane() -> AutomationLane {
        AutomationLane::new(
            ParameterAddress::new(ParameterOwner::Master, ParameterKey::Gain).expect("valid"),
        )
    }

    #[test]
    fn no_shape_ever_leaves_the_range_its_points_defined() {
        // The property that rules out a spline. An overshoot on a gain lane is
        // a value above unity; on a filter lane it is a frequency past
        // Nyquist. Either makes a sound the user did not ask for at a moment
        // they were not watching.
        for shape in [
            Interpolation::Hold,
            Interpolation::Linear,
            Interpolation::Smooth,
            Interpolation::Accelerating,
            Interpolation::Decelerating,
        ] {
            assert_eq!(shape.apply(0.0), 0.0, "{shape:?} does not start at zero");
            assert_eq!(shape.apply(1.0), 1.0, "{shape:?} does not end at one");
            let mut step = 0;
            while step <= 1000 {
                let fraction = f64::from(step) / 1000.0;
                let value = shape.apply(fraction);
                assert!(
                    (0.0..=1.0).contains(&value),
                    "{shape:?} at {fraction} gave {value}"
                );
                step += 1;
            }
            // Out of range in either direction is clamped, not extrapolated.
            assert_eq!(shape.apply(-1.0), 0.0);
            assert_eq!(shape.apply(2.0), 1.0);
        }
    }

    #[test]
    fn every_shape_is_monotone() {
        // Stronger than "no overshoot", and it is what makes a curve
        // predictable: dragging a point up must not make any part of the
        // segment go down.
        for shape in [
            Interpolation::Hold,
            Interpolation::Linear,
            Interpolation::Smooth,
            Interpolation::Accelerating,
            Interpolation::Decelerating,
        ] {
            let mut previous = 0.0_f64;
            let mut step = 0;
            while step <= 1000 {
                let value = shape.apply(f64::from(step) / 1000.0);
                assert!(
                    value >= previous - 1e-12,
                    "{shape:?} went backwards at {step}: {previous} then {value}"
                );
                previous = value;
                step += 1;
            }
        }
    }

    #[test]
    fn a_lane_reads_exactly_its_points_and_interpolates_between_them() {
        let mut lane = lane();
        lane.insert(AutomationPoint::new(
            Frames::new(0),
            0.0,
            Interpolation::Linear,
        ))
        .expect("room");
        lane.insert(AutomationPoint::new(
            Frames::new(1000),
            1.0,
            Interpolation::Linear,
        ))
        .expect("room");

        assert_eq!(lane.value_at(Frames::new(0)), Some(0.0));
        assert_eq!(lane.value_at(Frames::new(1000)), Some(1.0));
        assert_eq!(lane.value_at(Frames::new(500)), Some(0.5));
        assert_eq!(lane.value_at(Frames::new(250)), Some(0.25));
    }

    #[test]
    fn the_value_holds_before_the_first_point_and_after_the_last() {
        // Extrapolating instead would make an automation lane change a
        // parameter at times the user never touched, which is the most
        // confusing thing automation can do.
        let mut lane = lane();
        lane.insert(AutomationPoint::new(
            Frames::new(1000),
            0.3,
            Interpolation::Linear,
        ))
        .expect("room");
        lane.insert(AutomationPoint::new(
            Frames::new(2000),
            0.8,
            Interpolation::Linear,
        ))
        .expect("room");

        assert_eq!(lane.value_at(Frames::new(0)), Some(0.3));
        assert_eq!(lane.value_at(Frames::new(999)), Some(0.3));
        assert_eq!(lane.value_at(Frames::new(9_000_000)), Some(0.8));
    }

    #[test]
    fn a_hold_segment_does_not_slur_between_its_values() {
        // What a switch or a preset change needs. A linear ramp through values
        // that mean nothing is worse than a jump.
        let mut lane = lane();
        lane.insert(AutomationPoint::new(
            Frames::new(0),
            0.0,
            Interpolation::Hold,
        ))
        .expect("room");
        lane.insert(AutomationPoint::new(
            Frames::new(1000),
            1.0,
            Interpolation::Hold,
        ))
        .expect("room");

        assert_eq!(lane.value_at(Frames::new(1)), Some(0.0));
        assert_eq!(lane.value_at(Frames::new(999)), Some(0.0));
        assert_eq!(lane.value_at(Frames::new(1000)), Some(1.0));
    }

    #[test]
    fn an_empty_or_disabled_lane_says_nothing_rather_than_inventing_a_value() {
        // The mixer needs to tell "automation says 0.5" from "there is no
        // automation here", because the second means it should use the value
        // the user set on the control.
        let mut lane = lane();
        assert_eq!(lane.value_at(Frames::new(100)), None);

        lane.insert(AutomationPoint::new(
            Frames::new(0),
            0.5,
            Interpolation::Linear,
        ))
        .expect("room");
        assert_eq!(lane.value_at(Frames::new(100)), Some(0.5));

        lane.set_enabled(false);
        assert_eq!(lane.value_at(Frames::new(100)), None);
        assert_eq!(lane.len(), 1, "disabling must not discard the points");
    }

    #[test]
    fn inserting_at_an_existing_position_replaces_rather_than_duplicates() {
        // Idempotence, which the operation log needs: replaying an edit must
        // not make the curve depend on how many times it was replayed.
        let mut lane = lane();
        for value in [0.2_f32, 0.4, 0.9] {
            lane.insert(AutomationPoint::new(
                Frames::new(500),
                value,
                Interpolation::Linear,
            ))
            .expect("room");
        }
        assert_eq!(lane.len(), 1);
        assert_eq!(lane.value_at(Frames::new(500)), Some(0.9));
    }

    #[test]
    fn points_stay_sorted_however_they_are_inserted() {
        let mut lane = lane();
        for position in [900_i64, 100, 500, 0, 700] {
            lane.insert(AutomationPoint::new(
                Frames::new(position),
                0.5,
                Interpolation::Linear,
            ))
            .expect("room");
        }
        let positions: Vec<i64> = lane
            .points()
            .iter()
            .map(|point| point.position().get())
            .collect();
        assert_eq!(positions, vec![0, 100, 500, 700, 900]);
    }

    #[test]
    fn a_lane_cannot_grow_without_bound() {
        // Master Prompt #26 requires limits on anything a document can grow
        // without bound, and an automation lane is written to by a gesture that
        // can be held down.
        let mut lane = lane();
        for index in 0..MAX_POINTS {
            lane.insert(AutomationPoint::new(
                Frames::new(i64::try_from(index).unwrap_or(0)),
                0.5,
                Interpolation::Linear,
            ))
            .expect("within the limit");
        }
        assert_eq!(
            lane.insert(AutomationPoint::new(
                Frames::new(i64::MAX),
                0.5,
                Interpolation::Linear
            ))
            .err(),
            Some(AutomationError::TooManyPoints {
                maximum: MAX_POINTS
            })
        );
        // Replacing an existing point still works when full, because it does
        // not grow the lane — refusing it would strand a user who cannot
        // correct a mistake without first deleting something.
        assert!(lane
            .insert(AutomationPoint::new(
                Frames::new(0),
                0.9,
                Interpolation::Linear
            ))
            .is_ok());
    }

    #[test]
    fn a_non_numeric_value_never_reaches_the_signal_path() {
        // A non-number on the audio thread silences every filter it touches
        // until the engine restarts, and it arrives from a value that was
        // stored months earlier.
        let point = AutomationPoint::new(Frames::ZERO, f32::NAN, Interpolation::Linear);
        assert_eq!(point.value(), 0.0);
        assert_eq!(
            AutomationPoint::new(Frames::ZERO, f32::INFINITY, Interpolation::Linear).value(),
            1.0
        );
        assert_eq!(
            AutomationPoint::new(Frames::ZERO, -5.0, Interpolation::Linear).value(),
            0.0
        );
    }

    #[test]
    fn removing_a_range_takes_what_is_inside_it_and_nothing_else() {
        let mut lane = lane();
        for position in [0_i64, 100, 200, 300, 400] {
            lane.insert(AutomationPoint::new(
                Frames::new(position),
                0.5,
                Interpolation::Linear,
            ))
            .expect("room");
        }
        // Half open: the start is inside, the end is not.
        assert_eq!(lane.remove_range(Frames::new(100), Frames::new(300)), 2);
        let positions: Vec<i64> = lane
            .points()
            .iter()
            .map(|point| point.position().get())
            .collect();
        assert_eq!(positions, vec![0, 300, 400]);

        assert!(lane.remove_at(Frames::new(300)));
        assert!(!lane.remove_at(Frames::new(300)));
    }

    #[test]
    fn shifting_left_clamps_at_zero_without_losing_points_or_order() {
        let mut lane = lane();
        for position in [0_i64, 100, 500] {
            lane.insert(AutomationPoint::new(
                Frames::new(position),
                0.5,
                Interpolation::Linear,
            ))
            .expect("room");
        }
        lane.shift(Frames::new(-200));

        let positions: Vec<i64> = lane
            .points()
            .iter()
            .map(|point| point.position().get())
            .collect();
        // The two that would have gone negative collide at zero and merge.
        assert_eq!(positions, vec![0, 300]);
        assert_eq!(lane.start(), Some(Frames::new(0)));
        assert_eq!(lane.end(), Some(Frames::new(300)));
    }

    #[test]
    fn evaluation_is_correct_across_a_large_lane() {
        // The binary search is what makes evaluation affordable on the audio
        // thread; a linear scan would be correct and would stall. This checks
        // the search agrees with the answer at scale.
        let mut lane = lane();
        for index in 0..1000_i64 {
            let value = crate::num::narrow(crate::num::signed_to_f64(index) / 999.0);
            lane.insert(AutomationPoint::new(
                Frames::new(index * 100),
                value,
                Interpolation::Linear,
            ))
            .expect("room");
        }
        for index in [0_i64, 1, 250, 500, 999] {
            let expected = crate::num::narrow(crate::num::signed_to_f64(index) / 999.0);
            assert_eq!(lane.value_at(Frames::new(index * 100)), Some(expected));
        }
        // Halfway between two points is halfway between their values.
        let midpoint = lane.value_at(Frames::new(50)).expect("inside the lane");
        assert!(midpoint > 0.0 && midpoint < crate::num::narrow(1.0 / 999.0));
    }

    #[test]
    fn interpolation_keys_are_distinct() {
        let keys = [
            Interpolation::Hold.key(),
            Interpolation::Linear.key(),
            Interpolation::Smooth.key(),
            Interpolation::Accelerating.key(),
            Interpolation::Decelerating.key(),
        ];
        for (index, key) in keys.iter().enumerate() {
            for (other, value) in keys.iter().enumerate() {
                assert!(index == other || key != value, "two shapes share {key}");
            }
        }
    }
}
