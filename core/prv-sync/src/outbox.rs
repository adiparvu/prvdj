//! Work that has been done here and not yet reached anywhere else.
//!
//! # A full outbox refuses; it does not forget
//!
//! `prv-security`'s audit log, faced with the same situation, discards its
//! oldest entry and counts the loss. This one refuses to accept anything new
//! instead, and the difference is worth stating because the two look like the
//! same problem and are not.
//!
//! An audit entry is a *record of something that happened*; losing the oldest
//! one costs history. An outbox entry is *the user's work*, and the operation
//! log is append-only precisely so that nothing they did can be discarded
//! (Master Prompt #9). An outbox that dropped its oldest entry would silently
//! delete the first hour of a session that ran long offline — and would do it
//! most reliably to the user who was working hardest.
//!
//! So the bound is a back-pressure signal, not a retention policy. It is set
//! high enough that reaching it means something is wrong (a sync that has not
//! run in weeks, a server that is refusing everything), which is a thing to tell
//! the user about rather than a thing to handle quietly.
//!
//! # Acknowledgement is by identity, and re-sending is free
//!
//! A network that loses a reply makes a client send twice. Because operations
//! carry identities allocated without coordination (`prv-project`), the second
//! delivery is recognisable, and acknowledging something already acknowledged is
//! a no-op rather than a corruption.

use std::collections::BTreeSet;

use core::fmt;

use prv_project::{DeviceId, OperationId, VersionVector};

/// Why the outbox would not take something.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum OutboxError {
    /// It holds as much as it will.
    ///
    /// Not a condition to recover from quietly: reaching it means a session has
    /// been offline for a very long time or a server has been refusing
    /// everything, and the user needs to know either way.
    Full {
        /// How many entries that is.
        limit: usize,
    },
}

impl fmt::Display for OutboxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::Full { limit } => {
                write!(f, "{limit} edits are already waiting to be synchronised")
            }
        }
    }
}

impl core::error::Error for OutboxError {}

/// Operations authored here that the other side has not confirmed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Outbox {
    device: Option<DeviceId>,
    waiting: BTreeSet<OperationId>,
    acknowledged: VersionVector,
}

impl Outbox {
    /// How many operations may wait.
    ///
    /// Sixty-five thousand. A long offline session is thousands of operations,
    /// not tens of thousands, so reaching this is a signal rather than a normal
    /// state — which is exactly why it is a refusal the user hears about.
    pub const LIMIT: usize = 65_536;

    /// An empty outbox.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Records that an operation was authored here and needs to travel.
    ///
    /// Adding one that is already waiting, or one already acknowledged, changes
    /// nothing — which is what makes a caller that replays its own log on
    /// startup safe rather than duplicating everything.
    ///
    /// # Errors
    ///
    /// Returns [`OutboxError::Full`] rather than discarding anything.
    pub fn hold(&mut self, id: OperationId) -> Result<(), OutboxError> {
        if self.acknowledged.has_seen(id) || self.waiting.contains(&id) {
            return Ok(());
        }
        if self.waiting.len() >= Self::LIMIT {
            return Err(OutboxError::Full { limit: Self::LIMIT });
        }
        self.device.get_or_insert(id.device);
        self.waiting.insert(id);
        Ok(())
    }

    /// Records that the other side has it.
    ///
    /// Acknowledging something twice, or something never held, is a no-op. A
    /// network that loses a reply makes a client send twice, and the second
    /// delivery must be boring.
    pub fn acknowledge(&mut self, id: OperationId) {
        self.acknowledged.observe(id);
        self.waiting.remove(&id);
    }

    /// Records that everything up to a point has arrived.
    ///
    /// What a server that answers "I have everything through here" produces,
    /// and the reason a reconnection after a long silence costs one exchange
    /// rather than one per operation.
    pub fn acknowledge_through(&mut self, seen: &VersionVector) {
        self.waiting.retain(|id| !seen.has_seen(*id));
        self.acknowledged.merge_from(seen);
    }

    /// What is still waiting, oldest first.
    ///
    /// Ordered by identity, which for one device is the order it authored them —
    /// so a receiver applying them in this order never sees an operation before
    /// the one it was built on.
    #[must_use]
    pub fn waiting(&self) -> Vec<OperationId> {
        self.waiting.iter().copied().collect()
    }

    /// How many are waiting.
    #[must_use]
    pub fn len(&self) -> usize {
        self.waiting.len()
    }

    /// Whether everything authored here has arrived.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.waiting.is_empty()
    }

    /// Whether the outbox is close enough to full to be worth saying so.
    ///
    /// At nine tenths. Telling somebody their edits are piling up while there is
    /// still room to fix it is useful; telling them once it has stopped
    /// accepting is an apology.
    #[must_use]
    pub fn is_nearly_full(&self) -> bool {
        self.waiting.len() * 10 >= Self::LIMIT * 9
    }

    /// What this device knows the other side has.
    #[must_use]
    pub const fn acknowledged(&self) -> &VersionVector {
        &self.acknowledged
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

    fn here() -> DeviceId {
        DeviceId::new(1)
    }

    fn op(sequence: u64) -> OperationId {
        OperationId::new(here(), sequence)
    }

    #[test]
    fn a_full_outbox_refuses_rather_than_forgetting() {
        // The difference from an audit log, and the reason it matters: an
        // outbox entry is the user's work. Dropping the oldest would silently
        // delete the first hour of a long offline session, and would do it most
        // reliably to whoever was working hardest.
        let mut outbox = Outbox::new();
        for sequence in 1..=Outbox::LIMIT as u64 {
            outbox.hold(op(sequence)).expect("within the limit");
        }

        assert_eq!(
            outbox.hold(op(Outbox::LIMIT as u64 + 1)).err(),
            Some(OutboxError::Full {
                limit: Outbox::LIMIT
            })
        );
        assert_eq!(outbox.len(), Outbox::LIMIT);
        assert_eq!(
            outbox.waiting().first().copied(),
            Some(op(1)),
            "the oldest edit was discarded"
        );
    }

    #[test]
    fn replaying_a_log_on_startup_does_not_duplicate_anything() {
        // A caller that re-offers everything it authored must be boring.
        let mut outbox = Outbox::new();
        for sequence in 1..=5 {
            outbox.hold(op(sequence)).expect("held");
        }
        for sequence in 1..=5 {
            outbox.hold(op(sequence)).expect("held again");
        }
        assert_eq!(outbox.len(), 5);
    }

    #[test]
    fn acknowledging_twice_is_a_no_op() {
        // A network that loses a reply makes a client send twice. The second
        // delivery must be boring.
        let mut outbox = Outbox::new();
        outbox.hold(op(1)).expect("held");
        outbox.acknowledge(op(1));
        outbox.acknowledge(op(1));
        assert!(outbox.is_empty());

        // And re-offering something already acknowledged does not resurrect it.
        outbox.hold(op(1)).expect("held");
        assert!(outbox.is_empty(), "an acknowledged edit was queued again");
    }

    #[test]
    fn a_reconnection_after_a_long_silence_costs_one_exchange() {
        // The server answers "I have everything through here" rather than
        // acknowledging a thousand operations one at a time.
        let mut outbox = Outbox::new();
        for sequence in 1..=1000 {
            outbox.hold(op(sequence)).expect("held");
        }

        let mut seen = VersionVector::new();
        seen.observe(op(900));
        outbox.acknowledge_through(&seen);

        assert_eq!(outbox.len(), 100);
        assert_eq!(outbox.waiting().first().copied(), Some(op(901)));
        assert!(outbox.acknowledged().has_seen(op(900)));
    }

    #[test]
    fn what_is_waiting_comes_out_in_the_order_it_was_authored() {
        // A receiver applying them in this order never sees an operation before
        // the one it was built on.
        let mut outbox = Outbox::new();
        for sequence in [5, 1, 4, 2, 3] {
            outbox.hold(op(sequence)).expect("held");
        }
        let order: Vec<u64> = outbox.waiting().iter().map(|id| id.sequence).collect();
        assert_eq!(order, vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn edits_from_two_devices_do_not_collide() {
        // Identity without coordination, which is what makes offline editing on
        // two machines work at all.
        let mut outbox = Outbox::new();
        let other = DeviceId::new(2);
        outbox.hold(OperationId::new(here(), 1)).expect("held");
        outbox.hold(OperationId::new(other, 1)).expect("held");
        assert_eq!(outbox.len(), 2);

        outbox.acknowledge(OperationId::new(here(), 1));
        assert_eq!(outbox.len(), 1);
        assert_eq!(
            outbox.waiting().first().map(|id| id.device),
            Some(other),
            "acknowledging one device's edit removed another's"
        );
    }

    #[test]
    fn the_user_is_warned_while_there_is_still_room_to_act() {
        // Telling somebody their edits are piling up once it has stopped
        // accepting is an apology, not a warning.
        let mut outbox = Outbox::new();
        assert!(!outbox.is_nearly_full());

        // Filled until the warning appears rather than up to a computed
        // threshold, so the test measures the property and not my arithmetic.
        let mut sequence = 0_u64;
        while !outbox.is_nearly_full() {
            sequence = sequence.saturating_add(1);
            outbox
                .hold(op(sequence))
                .expect("the warning should arrive before the refusal");
        }
        assert!(
            outbox.len() < Outbox::LIMIT,
            "the warning arrived only once the outbox had stopped accepting"
        );
    }

    #[test]
    fn an_empty_outbox_says_everything_has_arrived() {
        let outbox = Outbox::new();
        assert!(outbox.is_empty());
        assert_eq!(outbox.len(), 0);
        assert!(outbox.waiting().is_empty());
        assert_eq!(outbox, Outbox::default());
    }
}
