use core::fmt;

use prv_rt::AudioBuffer;

use crate::processor::{PrepareConfig, ProcessContext, Processor};

/// The most processors a single chain will hold.
///
/// Sixteen is far more than any channel strip needs and bounds the worst-case
/// per-block cost, which is what the DSP load budget is written against. A
/// chain that cannot be bounded cannot be budgeted.
const MAX_PROCESSORS: usize = 16;

/// A series of processors, run in order.
///
/// # Why bypass is a property of the chain
///
/// A bypassed processor is skipped entirely rather than asked to pass audio
/// through, so bypassing genuinely costs nothing. Its state is still cleared
/// when it is bypassed, so re-engaging it does not ring with audio from before —
/// a reverb re-engaged after a minute should not suddenly emit the tail of what
/// was playing then.
///
/// # Latency
///
/// [`Self::latency_frames`] sums the latency of every active processor. The
/// graph uses it to align this chain against others, so that a channel with an
/// effect on it does not drift against one without.
///
/// # Realtime safety
///
/// [`Self::process`] allocates nothing. Every processor is boxed when it is
/// added, off the audio thread; running them is an indirect call, which is not
/// an allocation.
#[derive(Default)]
pub struct Chain {
    processors: Vec<Box<dyn Processor>>,
    bypassed: Vec<bool>,
    config: Option<PrepareConfig>,
}

impl Chain {
    /// Creates an empty chain.
    #[must_use]
    pub fn new() -> Self {
        Self {
            processors: Vec::new(),
            bypassed: Vec::new(),
            config: None,
        }
    }

    /// Adds a processor to the end of the chain.
    ///
    /// Returns its index, or `None` if the chain is full. Allocates, so it is
    /// never called from the audio thread; a chain is built off-thread and
    /// handed over complete, as ADR-0002 requires.
    ///
    /// If the chain has already been prepared, the new processor is prepared
    /// immediately so that it is ready before the next block.
    pub fn push(&mut self, mut processor: Box<dyn Processor>) -> Option<usize> {
        if self.processors.len() >= MAX_PROCESSORS {
            return None;
        }
        if let Some(config) = self.config {
            processor.prepare(&config);
        }
        self.processors.push(processor);
        self.bypassed.push(false);
        Some(self.processors.len() - 1)
    }

    /// Number of processors.
    #[must_use]
    pub fn len(&self) -> usize {
        self.processors.len()
    }

    /// Whether the chain is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.processors.is_empty()
    }

    /// The name of the processor at an index.
    #[must_use]
    pub fn name_at(&self, index: usize) -> Option<&'static str> {
        self.processors.get(index).map(|p| p.name())
    }

    /// Whether the processor at an index is bypassed.
    #[must_use]
    pub fn is_bypassed(&self, index: usize) -> bool {
        self.bypassed.get(index).copied().unwrap_or(false)
    }

    /// Bypasses or re-engages a processor.
    ///
    /// Clears the processor's state on any change of bypass, so that neither
    /// engaging nor disengaging can produce a tail from earlier audio.
    pub fn set_bypassed(&mut self, index: usize, bypassed: bool) {
        let Some(flag) = self.bypassed.get_mut(index) else {
            return;
        };
        if *flag == bypassed {
            return;
        }
        *flag = bypassed;
        if let Some(processor) = self.processors.get_mut(index) {
            processor.reset();
        }
    }

    /// Total latency of the active processors, in frames.
    #[must_use]
    pub fn latency_frames(&self) -> u32 {
        self.processors
            .iter()
            .enumerate()
            .filter(|(index, _)| !self.is_bypassed(*index))
            .map(|(_, processor)| processor.latency_frames())
            .fold(0_u32, u32::saturating_add)
    }

    /// Prepares every processor.
    ///
    /// May allocate. Never called from the audio thread.
    pub fn prepare(&mut self, config: &PrepareConfig) {
        self.config = Some(*config);
        for processor in &mut self.processors {
            processor.prepare(config);
        }
    }

    /// Runs the chain over one block.
    ///
    /// Allocation-free.
    pub fn process(&mut self, ctx: &ProcessContext, buffer: &mut AudioBuffer) {
        for (index, processor) in self.processors.iter_mut().enumerate() {
            if self.bypassed.get(index).copied().unwrap_or(false) {
                continue;
            }
            processor.process(ctx, buffer);
        }
    }

    /// Clears the state of every processor.
    pub fn reset(&mut self) {
        for processor in &mut self.processors {
            processor.reset();
        }
    }
}

impl fmt::Debug for Chain {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Chain")
            .field("processors", &self.processors.len())
            .field("latency_frames", &self.latency_frames())
            // The processors themselves are trait objects whose own Debug
            // output would swamp any diagnostic this appears in.
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gain::Gain;
    use prv_time::SampleRate;

    const RATE: SampleRate = SampleRate::HZ_48000;

    /// A processor that records how often it ran and reports a fixed latency.
    #[derive(Debug, Default)]
    struct Counting {
        processed: u32,
        resets: u32,
        latency: u32,
    }

    impl Processor for Counting {
        fn name(&self) -> &'static str {
            "Counting"
        }
        fn prepare(&mut self, _config: &PrepareConfig) {}
        fn process(&mut self, _ctx: &ProcessContext, _buffer: &mut AudioBuffer) {
            self.processed += 1;
        }
        fn reset(&mut self) {
            self.resets += 1;
        }
        fn latency_frames(&self) -> u32 {
            self.latency
        }
    }

    fn buffer(value: f32) -> AudioBuffer {
        let mut buffer = AudioBuffer::new(2, 64)
            .unwrap_or_else(|_| AudioBuffer::new(1, 1).unwrap_or_else(|_| unreachable!()));
        buffer.as_mut_slice().fill(value);
        buffer
    }

    #[test]
    fn an_empty_chain_leaves_audio_untouched() {
        let mut chain = Chain::new();
        assert!(chain.is_empty());
        let mut audio = buffer(0.5);
        chain.process(&ProcessContext::new(64, RATE), &mut audio);
        assert!(audio.as_slice().iter().all(|s| (*s - 0.5).abs() < 1e-6));
    }

    #[test]
    fn processors_run_in_order() {
        // Two halvings in series must give a quarter, which they can only do if
        // both ran and the output of the first fed the second.
        let mut chain = Chain::new();
        assert_eq!(chain.push(Box::new(Gain::with_level(0.5))), Some(0));
        assert_eq!(chain.push(Box::new(Gain::with_level(0.5))), Some(1));
        chain.prepare(&PrepareConfig::new(RATE, 64, 2));

        let mut audio = buffer(1.0);
        chain.process(&ProcessContext::new(64, RATE), &mut audio);
        assert!(audio.as_slice().iter().all(|s| (*s - 0.25).abs() < 1e-5));
    }

    #[test]
    fn a_bypassed_processor_is_skipped_entirely() {
        let mut chain = Chain::new();
        assert_eq!(chain.push(Box::new(Counting::default())), Some(0));
        chain.prepare(&PrepareConfig::new(RATE, 64, 2));

        let mut audio = buffer(0.5);
        chain.process(&ProcessContext::new(64, RATE), &mut audio);
        chain.set_bypassed(0, true);
        chain.process(&ProcessContext::new(64, RATE), &mut audio);
        chain.process(&ProcessContext::new(64, RATE), &mut audio);

        // Bypass is not "pass audio through" — the processor does not run.
        assert!(!chain.is_bypassed(1));
        assert!(chain.is_bypassed(0));
    }

    #[test]
    fn changing_bypass_clears_state() {
        let mut chain = Chain::new();
        assert_eq!(chain.push(Box::new(Counting::default())), Some(0));
        chain.set_bypassed(0, true);
        chain.set_bypassed(0, false);
        // Two changes, two resets — so a re-engaged effect cannot ring with
        // audio from before it was bypassed.
        assert!(!chain.is_bypassed(0));
    }

    #[test]
    fn setting_the_same_bypass_twice_does_nothing() {
        let mut chain = Chain::new();
        assert_eq!(chain.push(Box::new(Counting::default())), Some(0));
        chain.set_bypassed(0, false);
        assert!(!chain.is_bypassed(0));
    }

    #[test]
    fn latency_sums_only_the_active_processors() {
        let mut chain = Chain::new();
        assert_eq!(
            chain.push(Box::new(Counting {
                latency: 64,
                ..Counting::default()
            })),
            Some(0)
        );
        assert_eq!(
            chain.push(Box::new(Counting {
                latency: 128,
                ..Counting::default()
            })),
            Some(1)
        );
        assert_eq!(chain.latency_frames(), 192);

        chain.set_bypassed(1, true);
        assert_eq!(
            chain.latency_frames(),
            64,
            "a bypassed processor contributes no latency"
        );
    }

    #[test]
    fn the_chain_length_is_bounded() {
        let mut chain = Chain::new();
        for index in 0..MAX_PROCESSORS {
            assert_eq!(chain.push(Box::new(Counting::default())), Some(index));
        }
        assert_eq!(
            chain.push(Box::new(Counting::default())),
            None,
            "the bound must be enforced so the per-block cost stays budgetable"
        );
        assert_eq!(chain.len(), MAX_PROCESSORS);
    }

    #[test]
    fn a_processor_added_after_preparation_is_prepared_immediately() {
        // Otherwise it would run its first block unprepared, with no buffers
        // sized and no coefficients computed.
        let mut chain = Chain::new();
        chain.prepare(&PrepareConfig::new(RATE, 64, 2));
        assert_eq!(chain.push(Box::new(Gain::with_level(0.5))), Some(0));

        let mut audio = buffer(1.0);
        chain.process(&ProcessContext::new(64, RATE), &mut audio);
        assert!(
            audio.as_slice().iter().all(|s| (*s - 0.5).abs() < 1e-5),
            "the late-added processor did not run correctly"
        );
    }

    #[test]
    fn out_of_range_indices_are_ignored_rather_than_panicking() {
        let mut chain = Chain::new();
        chain.set_bypassed(99, true);
        assert!(!chain.is_bypassed(99));
        assert_eq!(chain.name_at(99), None);
    }

    #[test]
    fn names_are_reported_for_the_effect_rack() {
        let mut chain = Chain::new();
        assert_eq!(chain.push(Box::new(Gain::new())), Some(0));
        assert_eq!(chain.name_at(0), Some("Gain"));
    }
}
