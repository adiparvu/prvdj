//! Parameter ramps.
//!
//! # Why no parameter is ever applied as a step
//!
//! Master Prompt #18 requires the mixer to "avoid clicks and zipper noise".
//! Master Prompt #15 puts it more precisely: "processing should never surprise
//! the user". Both describe the same physical fact.
//!
//! A gain applied as an instantaneous step introduces a discontinuity into the
//! waveform. A discontinuity contains energy at every frequency, which is heard
//! as a click. Move a fader continuously and the interface sends a stream of
//! discrete values; applied as steps they become a stream of clicks — the
//! artefact known as zipper noise.
//!
//! The fix is to treat every incoming value as a *target* and move toward it
//! over a short interval. A few milliseconds is inaudible as a delay and
//! completely removes the discontinuity.
//!
//! # Why linear rather than exponential
//!
//! An exponential approach never quite arrives, so the parameter is always
//! slightly wrong and the engine can never tell whether it has settled. A linear
//! ramp reaches its target exactly, in a known number of frames, and reports
//! when it is done. That matters for correctness elsewhere: an automation curve
//! whose value only asymptotically approaches its breakpoint would never render
//! identically twice.
//!
//! Perceptual curvature belongs in the mapping from control value to parameter
//! value — decibels to linear gain, for instance — not in the smoothing.

use core::fmt;

/// A parameter that moves linearly toward a target over a fixed number of frames.
///
/// Allocation-free, branch-light and panic-free: designed to be advanced once
/// per frame inside the audio callback.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LinearSmoother {
    current: f32,
    target: f32,
    step: f32,
    remaining: u32,
}

impl LinearSmoother {
    /// Creates a smoother resting at `initial`.
    #[must_use]
    pub const fn new(initial: f32) -> Self {
        Self {
            current: initial,
            target: initial,
            step: 0.0,
            remaining: 0,
        }
    }

    /// Sets a new target to be reached over `frames` frames.
    ///
    /// A `frames` of zero jumps immediately, which is correct for the few
    /// parameters that must change on an exact sample — a loop boundary, for
    /// instance — and wrong for anything a human is moving.
    ///
    /// A non-finite target is ignored: the smoother keeps its current target
    /// rather than propagating a NaN into the signal path, where it would
    /// silence the output and be very hard to trace.
    pub fn set_target(&mut self, target: f32, frames: u32) {
        if !target.is_finite() {
            return;
        }
        if frames == 0 {
            self.set_immediate(target);
            return;
        }
        self.target = target;
        self.remaining = frames;
        // `frames` is non-zero, so this division is well defined.
        self.step = (target - self.current) / frames_as_f32(frames);
    }

    /// Jumps immediately to `value`, cancelling any ramp in progress.
    ///
    /// For initialisation and for discontinuous events such as loading a track,
    /// where there is no previous value to glide from. Ignores non-finite input.
    pub fn set_immediate(&mut self, value: f32) {
        if !value.is_finite() {
            return;
        }
        self.current = value;
        self.target = value;
        self.step = 0.0;
        self.remaining = 0;
    }

    /// Advances one frame and returns the value for that frame.
    #[must_use]
    pub fn next_value(&mut self) -> f32 {
        if self.remaining == 0 {
            return self.current;
        }
        self.remaining -= 1;
        if self.remaining == 0 {
            // Land exactly on the target rather than on the accumulated sum of
            // steps, so that the value is exact once settled.
            self.current = self.target;
        } else {
            self.current += self.step;
        }
        self.current
    }

    /// Advances `frames` frames without producing values.
    ///
    /// Used when a parameter is known not to affect the current block — a muted
    /// channel, for instance — so that it still arrives at the right value.
    pub fn skip(&mut self, frames: u32) {
        if frames == 0 {
            return;
        }
        if frames >= self.remaining {
            self.current = self.target;
            self.remaining = 0;
            self.step = 0.0;
            return;
        }
        self.current += self.step * frames_as_f32(frames);
        self.remaining -= frames;
    }

    /// Fills `output` with successive values, advancing by its length.
    ///
    /// The common case: one call per block instead of one per frame.
    pub fn fill(&mut self, output: &mut [f32]) {
        for sample in output.iter_mut() {
            *sample = self.next_value();
        }
    }

    /// The value at the current frame, without advancing.
    #[must_use]
    pub const fn current(self) -> f32 {
        self.current
    }

    /// The value being moved toward.
    #[must_use]
    pub const fn target(self) -> f32 {
        self.target
    }

    /// Returns `true` while a ramp is in progress.
    #[must_use]
    pub const fn is_smoothing(self) -> bool {
        self.remaining > 0
    }

    /// Frames remaining before the target is reached.
    #[must_use]
    pub const fn remaining(self) -> u32 {
        self.remaining
    }
}

/// Converts a frame count to `f32` for the step calculation.
///
/// Ramp lengths are milliseconds of audio — a few hundred to a few thousand
/// frames — so the conversion is exact in practice. Values beyond 2^24 would
/// lose precision, which would make a ramp very slightly wrong; it would still
/// terminate exactly on target because the final frame assigns the target
/// directly.
#[allow(
    clippy::cast_precision_loss,
    reason = "ramp lengths are far below the exact-integer limit of f32"
)]
const fn frames_as_f32(frames: u32) -> f32 {
    frames as f32
}

impl fmt::Display for LinearSmoother {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_smoothing() {
            write!(
                f,
                "{} → {} ({} frames left)",
                self.current, self.target, self.remaining
            )
        } else {
            write!(f, "{}", self.current)
        }
    }
}

#[cfg(test)]
mod tests {
    // Exact comparison is deliberate where it appears below. The central claim
    // of this module is that a ramp lands *exactly* on its target rather than
    // approaching it, so an approximate assertion would test the opposite of
    // what matters. Accumulated intermediate values are compared with a
    // tolerance, using `EPSILON`.
    #![allow(
        clippy::float_cmp,
        reason = "settled values must be exact; intermediate values use EPSILON"
    )]

    use super::*;

    /// Tolerance for comparing accumulated `f32` ramps.
    const EPSILON: f32 = 1e-5;

    #[test]
    fn a_new_smoother_rests_at_its_initial_value() {
        let mut smoother = LinearSmoother::new(0.5);
        assert!(!smoother.is_smoothing());
        assert_eq!(smoother.next_value(), 0.5);
        assert_eq!(smoother.current(), 0.5);
    }

    #[test]
    fn a_ramp_lands_exactly_on_target() {
        // Exactness matters: an asymptotic approach would leave the parameter
        // permanently slightly wrong, and identical renders would not be
        // identical.
        let mut smoother = LinearSmoother::new(0.0);
        smoother.set_target(1.0, 64);
        for _ in 0..64 {
            let _ = smoother.next_value();
        }
        assert!(!smoother.is_smoothing());
        assert_eq!(
            smoother.current(),
            1.0,
            "the ramp must arrive exactly, not approximately"
        );
    }

    #[test]
    fn a_ramp_is_monotonic_and_has_no_discontinuity() {
        // The property that prevents clicks: no single-frame jump larger than
        // one step.
        let mut smoother = LinearSmoother::new(0.0);
        smoother.set_target(1.0, 128);
        let expected_step = 1.0 / 128.0;

        let mut previous = 0.0_f32;
        for _ in 0..128 {
            let value = smoother.next_value();
            let delta = value - previous;
            assert!(delta >= 0.0, "a rising ramp must not fall back");
            assert!(
                delta <= expected_step + EPSILON,
                "no frame may jump more than one step: jumped {delta}"
            );
            previous = value;
        }
    }

    #[test]
    fn a_zero_length_ramp_jumps_immediately() {
        let mut smoother = LinearSmoother::new(0.0);
        smoother.set_target(1.0, 0);
        assert!(!smoother.is_smoothing());
        assert_eq!(smoother.next_value(), 1.0);
    }

    #[test]
    fn retargeting_mid_ramp_starts_from_where_it_is() {
        // A user moving a fader sends a stream of targets. Each must continue
        // from the current value, never jump back to the start.
        let mut smoother = LinearSmoother::new(0.0);
        smoother.set_target(1.0, 100);
        for _ in 0..50 {
            let _ = smoother.next_value();
        }
        let midpoint = smoother.current();
        assert!(
            (midpoint - 0.5).abs() < 0.02,
            "expected about 0.5, got {midpoint}"
        );

        smoother.set_target(0.0, 50);
        assert_eq!(
            smoother.current(),
            midpoint,
            "retargeting must not move the current value"
        );
        for _ in 0..50 {
            let _ = smoother.next_value();
        }
        assert_eq!(smoother.current(), 0.0);
    }

    #[test]
    fn skipping_arrives_at_the_same_place_as_stepping() {
        let mut stepped = LinearSmoother::new(0.0);
        let mut skipped = LinearSmoother::new(0.0);
        stepped.set_target(1.0, 256);
        skipped.set_target(1.0, 256);

        for _ in 0..100 {
            let _ = stepped.next_value();
        }
        skipped.skip(100);

        assert!((stepped.current() - skipped.current()).abs() < EPSILON);
        assert_eq!(stepped.remaining(), skipped.remaining());
    }

    #[test]
    fn skipping_past_the_end_settles_on_target() {
        let mut smoother = LinearSmoother::new(0.0);
        smoother.set_target(1.0, 64);
        smoother.skip(1_000);
        assert!(!smoother.is_smoothing());
        assert_eq!(smoother.current(), 1.0);
    }

    #[test]
    fn fill_produces_the_same_sequence_as_repeated_stepping() {
        let mut stepped = LinearSmoother::new(0.25);
        let mut filled = LinearSmoother::new(0.25);
        stepped.set_target(0.75, 32);
        filled.set_target(0.75, 32);

        let mut buffer = [0.0_f32; 32];
        filled.fill(&mut buffer);

        for (index, sample) in buffer.iter().enumerate() {
            let expected = stepped.next_value();
            assert!(
                (sample - expected).abs() < EPSILON,
                "frame {index}: fill produced {sample}, stepping produced {expected}"
            );
        }
    }

    #[test]
    fn non_finite_input_is_ignored_rather_than_propagated() {
        // A NaN reaching the signal path silences the output and is extremely
        // hard to trace back to its source. It stops here.
        let mut smoother = LinearSmoother::new(0.5);
        smoother.set_target(f32::NAN, 64);
        assert_eq!(smoother.target(), 0.5);
        assert!(!smoother.is_smoothing());

        smoother.set_target(f32::INFINITY, 64);
        assert_eq!(smoother.target(), 0.5);

        smoother.set_immediate(f32::NAN);
        assert_eq!(smoother.current(), 0.5);
    }
}
