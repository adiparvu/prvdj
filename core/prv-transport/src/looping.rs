use core::fmt;

use prv_time::Frames;

/// A region of the timeline that repeats.
///
/// # Why the region is validated on construction
///
/// A loop whose end is at or before its start has no defined behaviour: it would
/// either produce silence, spin without advancing, or wrap every block. Rather
/// than defend against that at every use, the region cannot be built in that
/// shape. Master Prompt #21's snapping guarantees keep it that way — a loop end
/// snapped forward can never land before its own start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoopRegion {
    start: Frames,
    end: Frames,
    enabled: bool,
}

impl LoopRegion {
    /// Creates a disabled region covering nothing.
    ///
    /// Used as the initial value; enabling it before setting bounds is a no-op
    /// because the bounds are empty.
    #[must_use]
    pub const fn none() -> Self {
        Self {
            start: Frames::ZERO,
            end: Frames::ZERO,
            enabled: false,
        }
    }

    /// Creates a region, returning `None` if `end` is not after `start`.
    #[must_use]
    pub fn new(start: Frames, end: Frames) -> Option<Self> {
        if end <= start {
            return None;
        }
        Some(Self {
            start,
            end,
            enabled: false,
        })
    }

    /// Start of the region.
    #[must_use]
    pub const fn start(self) -> Frames {
        self.start
    }

    /// End of the region. Exclusive: the frame at `end` is the first frame of
    /// the repeat, not the last frame of the loop.
    #[must_use]
    pub const fn end(self) -> Frames {
        self.end
    }

    /// Length in frames. Always positive for a region built by [`Self::new`].
    #[must_use]
    pub fn length(self) -> Frames {
        self.end - self.start
    }

    /// Whether the loop is engaged.
    #[must_use]
    pub const fn is_enabled(self) -> bool {
        self.enabled
    }

    /// Returns the region with the loop engaged.
    ///
    /// A region of zero length cannot be engaged, so enabling [`Self::none`]
    /// leaves it disabled.
    #[must_use]
    pub fn enabled(mut self) -> Self {
        self.enabled = self.end > self.start;
        self
    }

    /// Returns the region with the loop disengaged.
    #[must_use]
    pub const fn disabled(mut self) -> Self {
        self.enabled = false;
        self
    }

    /// Returns `true` if a position falls inside the region.
    #[must_use]
    pub fn contains(self, position: Frames) -> bool {
        position >= self.start && position < self.end
    }

    /// Wraps a position that has run past the end back into the region.
    ///
    /// Handles an overshoot larger than the loop itself, which happens when the
    /// block size exceeds a very short loop — a beat roll at a high tempo, for
    /// instance. Wrapping by the remainder rather than jumping to the start
    /// keeps the phase correct, so a short loop does not slowly slide out of
    /// time with the rest of the mix.
    #[must_use]
    pub fn wrap(self, position: Frames) -> Frames {
        let length = self.length().get();
        if length <= 0 || position < self.end {
            return position;
        }
        let overshoot = position.get().saturating_sub(self.end.get());
        Frames::new(
            self.start
                .get()
                .saturating_add(overshoot.rem_euclid(length)),
        )
    }
}

impl fmt::Display for LoopRegion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.enabled {
            write!(f, "loop {}..{}", self.start.get(), self.end.get())
        } else {
            write!(f, "loop off ({}..{})", self.start.get(), self.end.get())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn region(start: i64, end: i64) -> LoopRegion {
        LoopRegion::new(Frames::new(start), Frames::new(end)).unwrap_or(LoopRegion::none())
    }

    #[test]
    fn a_region_must_have_positive_length() {
        assert!(LoopRegion::new(Frames::new(100), Frames::new(200)).is_some());
        assert!(LoopRegion::new(Frames::new(100), Frames::new(100)).is_none());
        assert!(LoopRegion::new(Frames::new(200), Frames::new(100)).is_none());
    }

    #[test]
    fn an_empty_region_cannot_be_engaged() {
        let empty = LoopRegion::none();
        assert!(!empty.enabled().is_enabled());
    }

    #[test]
    fn engaging_and_disengaging_preserves_the_bounds() {
        let region = region(1_000, 5_000);
        let engaged = region.enabled();
        assert!(engaged.is_enabled());
        assert_eq!(engaged.start(), Frames::new(1_000));
        assert_eq!(engaged.end(), Frames::new(5_000));
        assert!(!engaged.disabled().is_enabled());
    }

    #[test]
    fn the_end_is_exclusive() {
        let region = region(100, 200);
        assert!(region.contains(Frames::new(100)));
        assert!(region.contains(Frames::new(199)));
        assert!(!region.contains(Frames::new(200)));
        assert!(!region.contains(Frames::new(99)));
    }

    #[test]
    fn wrapping_returns_a_position_inside_the_region() {
        let region = region(1_000, 1_400);
        assert_eq!(region.wrap(Frames::new(1_400)), Frames::new(1_000));
        assert_eq!(region.wrap(Frames::new(1_450)), Frames::new(1_050));
        // A position still inside is untouched.
        assert_eq!(region.wrap(Frames::new(1_200)), Frames::new(1_200));
    }

    #[test]
    fn an_overshoot_larger_than_the_loop_preserves_phase() {
        // A 100-frame loop with a 512-frame block: the naive answer is to jump
        // to the start, which loses 12 frames of phase every block and slides
        // the loop out of time. Wrapping by the remainder does not.
        let region = region(0, 100);
        assert_eq!(region.wrap(Frames::new(512)), Frames::new(12));
        assert_eq!(region.wrap(Frames::new(1_000)), Frames::new(0));
        assert_eq!(region.wrap(Frames::new(1_099)), Frames::new(99));
    }

    #[test]
    fn wrapping_an_empty_region_is_a_no_op() {
        let empty = LoopRegion::none();
        assert_eq!(empty.wrap(Frames::new(500)), Frames::new(500));
    }
}
