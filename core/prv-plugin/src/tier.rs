//! How much a plugin is trusted with, and what that costs.
//!
//! ADR-0005 answers a question that has only bad answers if it is asked once:
//! genuine isolation normally means another process, and crossing a process
//! boundary inside a 2.7 millisecond audio budget introduces exactly the
//! scheduling risk Master Prompt #18 forbids. The decision was to stop asking it
//! once. The isolation mechanism is matched to what the plugin actually needs to
//! touch, and this module is that decision in the type system.
//!
//! # The tier is assigned by capability, not by vendor
//!
//! A first-party plugin that only reads metadata runs out of process, like any
//! other metadata provider. Being ours is not a reason to be trusted with the
//! audio thread; needing the audio thread is, and only after the code has been
//! reviewed against the contract in ADR-0002.

use core::fmt;

/// How a plugin is isolated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum IsolationTier {
    /// A separate, sandboxed process.
    ///
    /// Analysis engines, importers, exporters, metadata and AI providers.
    /// Nothing here is on the audio path, so latency is irrelevant and the
    /// strongest isolation available costs nothing. A crash affects one process.
    OutOfProcess,

    /// WebAssembly, in process, on the audio thread.
    ///
    /// Third-party effects. Memory-safe by construction, no syscalls, no
    /// ambient authority, deterministic — everything the audio thread needs and
    /// nothing more. The execution overhead relative to native code is accepted:
    /// Master Prompt #15 ranks reliability above performance, and a plugin that
    /// cannot crash the host during a live set is worth more than one that runs
    /// marginally faster.
    Wasm,

    /// Native code bound by the same contract as the core.
    ///
    /// The built-in effect set, and processors that pass a certification
    /// programme: reviewed against the audio-thread contract and covered by
    /// allocation and timing tests. Deliberately narrow, and never open by
    /// default — this is the tier where a mistake takes the process down.
    Native,
}

impl IsolationTier {
    /// Every tier, most isolated first.
    pub const ALL: [Self; 3] = [Self::OutOfProcess, Self::Wasm, Self::Native];

    /// Whether a plugin in this tier may run inside the audio callback.
    #[must_use]
    pub const fn runs_on_the_audio_thread(self) -> bool {
        matches!(self, Self::Wasm | Self::Native)
    }

    /// Whether its execution can be interrupted mid-block.
    ///
    /// True only for WebAssembly, where execution is metered. Native code cannot
    /// be interrupted safely, which is the whole reason that tier requires
    /// review rather than a manifest: for native processors the guarantee has to
    /// come from the code, because it cannot come from the runtime.
    #[must_use]
    pub const fn execution_can_be_interrupted(self) -> bool {
        matches!(self, Self::Wasm)
    }

    /// Whether a plugin may only enter this tier through certification.
    #[must_use]
    pub const fn requires_certification(self) -> bool {
        matches!(self, Self::Native)
    }

    /// Whether a crash in this tier can take the application down.
    ///
    /// The reason [`Self::Native`] is narrow. Out-of-process and WebAssembly
    /// failures are contained; native failures are not, and no watchdog can
    /// contain them after the fact.
    #[must_use]
    pub const fn a_crash_reaches_the_host(self) -> bool {
        matches!(self, Self::Native)
    }

    /// A stable identifier, for storage and localisation.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::OutOfProcess => "tier.out_of_process",
            Self::Wasm => "tier.wasm",
            Self::Native => "tier.native",
        }
    }

    /// Reads a stored identifier.
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|tier| tier.key() == key)
    }
}

impl fmt::Display for IsolationTier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.key())
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

    #[test]
    fn only_the_native_tier_can_take_the_application_down() {
        // The property that makes the narrowness of that tier a decision rather
        // than a preference.
        for tier in IsolationTier::ALL {
            assert_eq!(
                tier.a_crash_reaches_the_host(),
                tier == IsolationTier::Native,
                "{tier} disagrees about whether a crash is contained"
            );
        }
    }

    #[test]
    fn a_tier_that_cannot_be_interrupted_must_be_certified() {
        // The pairing ADR-0005 rests on. A plugin on the audio thread is either
        // interruptible by the runtime or reviewed by a person; a tier that was
        // neither would have no protection at all.
        for tier in IsolationTier::ALL {
            if tier.runs_on_the_audio_thread() && !tier.execution_can_be_interrupted() {
                assert!(
                    tier.requires_certification(),
                    "{tier} runs on the audio thread, cannot be interrupted, and needs no review"
                );
            }
        }
    }

    #[test]
    fn isolation_weakens_exactly_once_along_the_ordering() {
        // The ordering is meaningful: OutOfProcess is more isolated than Wasm,
        // which is more isolated than Native. A future tier inserted in the
        // wrong place would read as safer than it is.
        assert!(IsolationTier::OutOfProcess < IsolationTier::Wasm);
        assert!(IsolationTier::Wasm < IsolationTier::Native);
        assert!(!IsolationTier::OutOfProcess.runs_on_the_audio_thread());
    }

    #[test]
    fn tier_keys_round_trip_and_are_distinct() {
        for tier in IsolationTier::ALL {
            assert_eq!(IsolationTier::from_key(tier.key()), Some(tier));
        }
        assert_eq!(IsolationTier::from_key("tier.invented"), None);

        let keys: Vec<&str> = IsolationTier::ALL.iter().map(|t| t.key()).collect();
        for (index, key) in keys.iter().enumerate() {
            for (other, value) in keys.iter().enumerate() {
                assert!(index == other || key != value, "two tiers share {key}");
            }
        }
    }
}
