//! When a plugin has had enough chances.
//!
//! # Two rules, because one is not enough
//!
//! ADR-0005 says a plugin that exceeds its budget repeatedly is bypassed. The
//! obvious reading is a run of consecutive overruns, and on its own that rule
//! misses the worst case: a plugin that overruns every third block never has
//! three in a row and is unusable anyway. So there are two rules —
//! [`Watchdog::MAX_CONSECUTIVE`] and [`Watchdog::MAX_IN_WINDOW`] over the last
//! [`Watchdog::WINDOW`] blocks — and either one is enough.
//!
//! # This runs on the audio thread
//!
//! Every function here is total, allocation-free and branch-bounded. The history
//! is a 64-bit word shifted once per block, which is why the window is 64 and
//! not a rounder number: the shape of the data structure is the reason the
//! measurement costs nothing.
//!
//! # One overrun is not a fault
//!
//! A single late block happens on any machine — a page fault, a core parked by
//! the scheduler, a background index. Bypassing a plugin the first time it is
//! unlucky would make the product feel broken on hardware that is fine. What is
//! not tolerable is a *pattern*, and that is what these two rules describe.

/// How a block went.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Verdict {
    /// Keep going.
    Continue,
    /// Pass this plugin over from now on.
    ///
    /// Returned once, at the moment the threshold is crossed, so that a caller
    /// reports the bypass exactly one time rather than on every block for the
    /// rest of the set.
    Bypass {
        /// Which rule ran out.
        reason: BypassReason,
    },
}

/// Which rule was exceeded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum BypassReason {
    /// Too many blocks in a row.
    Consecutive {
        /// How many.
        count: u32,
    },
    /// Too many within the recent window.
    TooFrequent {
        /// How many, out of [`Watchdog::WINDOW`].
        count: u32,
    },
}

impl BypassReason {
    /// A stable identifier, for storage and localisation.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Consecutive { .. } => "bypass.consecutive",
            Self::TooFrequent { .. } => "bypass.too_frequent",
        }
    }
}

/// Watches one plugin's cost per block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Watchdog {
    history: u64,
    consecutive: u32,
    bypassed: bool,
    overruns: u64,
    blocks: u64,
}

impl Default for Watchdog {
    fn default() -> Self {
        Self::new()
    }
}

impl Watchdog {
    /// How many blocks the recent window covers.
    ///
    /// Sixty-four, because the history is a 64-bit word and shifting one is a
    /// single instruction. At a 256-frame block and 48 kHz that is about a third
    /// of a second — long enough to distinguish a pattern from bad luck, short
    /// enough that a plugin which has genuinely failed is passed over before a
    /// listener decides the system is broken.
    pub const WINDOW: u32 = 64;

    /// How many blocks in a row may overrun.
    ///
    /// Three. One is bad luck on any machine; two is still plausible; three in
    /// succession is the plugin.
    pub const MAX_CONSECUTIVE: u32 = 3;

    /// How many of the last [`Self::WINDOW`] blocks may overrun.
    ///
    /// Eight, which is one block in eight. A plugin missing that often is not
    /// occasionally unlucky, and the audible result — a dropout every quarter of
    /// a second — is worse than not having the effect at all.
    pub const MAX_IN_WINDOW: u32 = 8;

    /// A watchdog with a clean record.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            history: 0,
            consecutive: 0,
            bypassed: false,
            overruns: 0,
            blocks: 0,
        }
    }

    /// Records how a block went and says whether to keep going.
    ///
    /// `cost` and `budget` are in whatever unit the caller measures in, as long
    /// as it is the same one; the watchdog compares them and never interprets
    /// them. A budget of zero is treated as "no budget declared", which cannot
    /// be exceeded — a plugin is not bypassed because nobody told the host what
    /// to expect of it.
    pub fn observe(&mut self, cost: u64, budget: u64) -> Verdict {
        self.blocks = self.blocks.saturating_add(1);
        let overran = budget > 0 && cost > budget;

        self.history = (self.history << 1) | u64::from(overran);
        if overran {
            self.consecutive = self.consecutive.saturating_add(1);
            self.overruns = self.overruns.saturating_add(1);
        } else {
            self.consecutive = 0;
        }

        if self.bypassed {
            return Verdict::Continue;
        }

        if self.consecutive >= Self::MAX_CONSECUTIVE {
            self.bypassed = true;
            return Verdict::Bypass {
                reason: BypassReason::Consecutive {
                    count: self.consecutive,
                },
            };
        }

        let in_window = self.history.count_ones();
        if in_window >= Self::MAX_IN_WINDOW {
            self.bypassed = true;
            return Verdict::Bypass {
                reason: BypassReason::TooFrequent { count: in_window },
            };
        }

        Verdict::Continue
    }

    /// Whether this plugin is being passed over.
    #[must_use]
    pub const fn is_bypassed(self) -> bool {
        self.bypassed
    }

    /// How many blocks overran, over the whole life of the watchdog.
    ///
    /// What a diagnostic reports. A plugin that was bypassed after eleven
    /// overruns in four hours is a different conversation from one bypassed
    /// after eleven in a second.
    #[must_use]
    pub const fn overruns(self) -> u64 {
        self.overruns
    }

    /// How many blocks were observed.
    #[must_use]
    pub const fn blocks(self) -> u64 {
        self.blocks
    }

    /// How many of the last [`Self::WINDOW`] blocks overran.
    #[must_use]
    pub const fn recent_overruns(self) -> u32 {
        self.history.count_ones()
    }

    /// Clears the recent record after the supervisor restarts the plugin.
    ///
    /// The lifetime totals are kept. A plugin that has been restarted four times
    /// has a history, and losing it would let a badly behaved plugin look new
    /// every few minutes.
    pub fn recovered(&mut self) {
        self.history = 0;
        self.consecutive = 0;
        self.bypassed = false;
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

    const BUDGET: u64 = 100;

    fn run(watchdog: &mut Watchdog, pattern: &[bool]) -> Option<BypassReason> {
        let mut first = None;
        for overran in pattern {
            let cost = if *overran { BUDGET + 1 } else { BUDGET - 1 };
            if let Verdict::Bypass { reason } = watchdog.observe(cost, BUDGET) {
                if first.is_none() {
                    first = Some(reason);
                }
            }
        }
        first
    }

    #[test]
    fn one_late_block_is_not_a_fault() {
        // A page fault, a parked core, a background index. Bypassing the first
        // time a plugin is unlucky would make the product feel broken on
        // hardware that is fine.
        let mut watchdog = Watchdog::new();
        assert_eq!(run(&mut watchdog, &[false, true, false, false]), None);
        assert!(!watchdog.is_bypassed());
        assert_eq!(watchdog.overruns(), 1);
    }

    #[test]
    fn three_in_a_row_is_the_plugin() {
        let mut watchdog = Watchdog::new();
        assert_eq!(
            run(&mut watchdog, &[true, true, true]),
            Some(BypassReason::Consecutive { count: 3 })
        );
        assert!(watchdog.is_bypassed());
    }

    #[test]
    fn a_plugin_that_never_has_three_in_a_row_is_still_caught() {
        // The case the consecutive rule alone misses. Every third block late is
        // a dropout four times a second, which is worse than not having the
        // effect at all.
        let pattern: Vec<bool> = (0..48).map(|index| index % 3 == 0).collect();
        let mut watchdog = Watchdog::new();
        let reason = run(&mut watchdog, &pattern);
        assert!(
            matches!(reason, Some(BypassReason::TooFrequent { .. })),
            "an every-third-block plugin was not caught: {reason:?}"
        );
        assert!(watchdog.is_bypassed());
    }

    #[test]
    fn the_bypass_is_reported_once_and_not_on_every_block_afterwards() {
        // Otherwise a bypassed plugin produces a notification every 5
        // milliseconds for the rest of the set.
        let mut watchdog = Watchdog::new();
        assert!(matches!(
            watchdog.observe(BUDGET + 1, BUDGET),
            Verdict::Continue
        ));
        watchdog.observe(BUDGET + 1, BUDGET);
        assert!(matches!(
            watchdog.observe(BUDGET + 1, BUDGET),
            Verdict::Bypass { .. }
        ));

        for _ in 0..100 {
            assert_eq!(
                watchdog.observe(BUDGET + 1, BUDGET),
                Verdict::Continue,
                "the bypass was reported more than once"
            );
        }
    }

    #[test]
    fn a_plugin_with_no_declared_budget_is_never_bypassed_for_cost() {
        // It is not bypassed because nobody told the host what to expect of it.
        let mut watchdog = Watchdog::new();
        for _ in 0..(Watchdog::WINDOW * 2) {
            assert_eq!(watchdog.observe(u64::MAX, 0), Verdict::Continue);
        }
        assert!(!watchdog.is_bypassed());
        assert_eq!(watchdog.overruns(), 0);
    }

    #[test]
    fn exactly_meeting_the_budget_is_not_an_overrun() {
        // A plugin that declares 100 and takes 100 did what it said.
        let mut watchdog = Watchdog::new();
        for _ in 0..10 {
            assert_eq!(watchdog.observe(BUDGET, BUDGET), Verdict::Continue);
        }
        assert_eq!(watchdog.overruns(), 0);
    }

    #[test]
    fn the_window_forgets_and_the_lifetime_total_does_not() {
        // What makes the frequency rule a rule about the recent past.
        let mut watchdog = Watchdog::new();
        run(&mut watchdog, &[true, false, true, false]);
        assert_eq!(watchdog.recent_overruns(), 2);
        assert_eq!(watchdog.overruns(), 2);

        run(&mut watchdog, &vec![false; Watchdog::WINDOW as usize]);
        assert_eq!(
            watchdog.recent_overruns(),
            0,
            "the window should have moved past them"
        );
        assert_eq!(
            watchdog.overruns(),
            2,
            "the lifetime total should not forget"
        );
        assert_eq!(watchdog.blocks(), 4 + u64::from(Watchdog::WINDOW));
    }

    #[test]
    fn a_restarted_plugin_starts_the_recent_record_again_and_keeps_its_history() {
        // Losing the history would let a badly behaved plugin look new every
        // few minutes.
        let mut watchdog = Watchdog::new();
        run(&mut watchdog, &[true, true, true]);
        assert!(watchdog.is_bypassed());

        watchdog.recovered();
        assert!(!watchdog.is_bypassed());
        assert_eq!(watchdog.recent_overruns(), 0);
        assert_eq!(watchdog.overruns(), 3, "the lifetime total was reset");

        // And it can be bypassed again on its own merits.
        assert_eq!(
            run(&mut watchdog, &[true, true, true]),
            Some(BypassReason::Consecutive { count: 3 })
        );
    }

    #[test]
    fn the_two_rules_are_ordered_so_the_more_specific_one_is_reported() {
        // Three in a row inside a busy window should read as "three in a row",
        // which is the more actionable message.
        let mut watchdog = Watchdog::new();
        let reason = run(
            &mut watchdog,
            &[true, true, true, true, true, true, true, true],
        );
        assert_eq!(reason, Some(BypassReason::Consecutive { count: 3 }));
        assert_eq!(
            BypassReason::Consecutive { count: 3 }.key(),
            "bypass.consecutive"
        );
        assert_eq!(
            BypassReason::TooFrequent { count: 8 }.key(),
            "bypass.too_frequent"
        );
    }
}
