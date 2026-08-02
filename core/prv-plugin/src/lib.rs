//! Plugins: what they declare, what they are trusted with, and what happens
//! when they misbehave.
//!
//! # The tension this crate exists to resolve
//!
//! Master Prompt #23 requires plugins to be sandboxed, permission-based, signed
//! and revocable, and requires that a crashing plugin never stops playback.
//! Master Prompt #18 forbids allocation, locking, blocking and syscalls on the
//! audio thread. Those two pull in opposite directions, because the usual
//! sandbox is another process and the usual cost of another process is a
//! scheduling dependency inside a 2.7 millisecond budget.
//!
//! ADR-0005 resolved it by refusing to answer once: the isolation mechanism is
//! matched to what a plugin actually needs to touch. This crate is that decision
//! expressed in types, plus the three mechanisms that make it hold —
//! [`manifest`] for what is claimed, [`lifecycle`] for where a plugin is, and
//! [`watchdog`] for when it has had enough chances.
//!
//! # What is here and what is not
//!
//! Here: the policy. Which tier a plugin belongs to, what its manifest may
//! assert, what the user has to approve, when a plugin is passed over, and what
//! the graph owes it in latency.
//!
//! Not here: running any of it. There is no WebAssembly runtime, no child
//! process, no signature verification and no dynamic loading, because all four
//! are input and output and ADR-0001 keeps those outside the core. What this
//! crate provides is the set of decisions that host has to obey, in a form that
//! can be tested without one.
//!
//! # Authorisation is asked, not answered
//!
//! [`Registry::authorise`] delegates to `prv-security`. The registry knows what
//! a plugin was granted; it does not decide what that means. Master Prompt #26
//! requires one decision point, and a plugin manager that answered permission
//! questions itself would be the second.

pub mod lifecycle;
pub mod manifest;
pub mod registry;
pub mod tier;
pub mod watchdog;

pub use lifecycle::{AudioBehaviour, LifecycleError, LifecycleEvent, LifecycleState};
pub use manifest::{Manifest, ManifestError, Signature, Version};
pub use registry::{Installed, Registry, RegistryError};
pub use tier::IsolationTier;
pub use watchdog::{BypassReason, Verdict, Watchdog};

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::panic,
        reason = "a test that cannot build its own fixture should fail loudly"
    )]

    use super::*;
    use prv_security::{Capability, PermissionSet, PluginId};

    fn a_hostile_manifest() -> Manifest {
        // Asks for everything, is not signed by anything we trust, and claims a
        // long latency. All three are ordinary and all three are handled.
        Manifest::read(
            PluginId::new(7),
            "com.example.helpful",
            "Very Helpful Plugin",
            Version::new(2, 0, 0),
            IsolationTier::Wasm,
            u32::MAX,
            1024,
            Manifest::HOST_API_VERSION,
            Signature::Untrusted,
        )
        .expect("a manifest that is valid and still not to be trusted")
    }

    #[test]
    fn the_whole_path_from_a_hostile_manifest_to_a_bypass() {
        // One test for the shape of the crate. Nothing here is exotic; every
        // step is what happens to an ordinary plugin, and the hostile manifest
        // only makes each step visible.
        let manifest = a_hostile_manifest();
        let id = manifest.id();

        // It asked for authority over the user's credentials and did not get it.
        assert!(!manifest.asks_for(Capability::ReadSecrets));
        assert!(manifest.asks_for(Capability::UseNetwork));

        // It is not signed by anything we recognise, so it cannot load quietly.
        assert!(!manifest.may_load_without_asking());

        let mut registry = Registry::new();
        registry.install(manifest).expect("install");

        // The user approves the network and nothing else.
        let mut approved = PermissionSet::NONE;
        approved.request(Capability::UseNetwork);
        registry.approve(id, approved).expect("approve");
        assert!(registry.authorise(id, Capability::UseNetwork).is_ok());
        assert!(registry.authorise(id, Capability::WriteFiles).is_err());

        registry.advance(id, LifecycleEvent::Load).expect("load");
        registry.advance(id, LifecycleEvent::Start).expect("start");
        assert_eq!(registry.compensated_latency_frames(), 1024);

        // It then takes twice its budget, three blocks running.
        let mut reported = None;
        for _ in 0..3 {
            if let Some(reason) = registry.observe(id, 200, 100).expect("observe") {
                reported = Some(reason);
            }
        }
        assert!(reported.is_some(), "it was never passed over");

        // The music continues, in time, without it.
        let installed = registry.get(id).expect("installed");
        assert_eq!(installed.state(), LifecycleState::Bypassed);
        assert_eq!(installed.audio_behaviour(), AudioBehaviour::PassesThrough);
        assert_eq!(
            registry.compensated_latency_frames(),
            1024,
            "the set moved in time when a plugin failed"
        );

        // And the user can remove it, from where it is, without stopping first.
        registry
            .advance(id, LifecycleEvent::Revoke)
            .expect("revoke");
        assert!(registry.authorise(id, Capability::UseNetwork).is_err());
    }

    #[test]
    fn a_plugin_cannot_widen_its_own_authority_at_any_step() {
        // Checked at each of the three places it could be attempted: in the
        // manifest, at approval, and at the question.
        let manifest = a_hostile_manifest();
        let id = manifest.id();

        let mut registry = Registry::new();
        registry.install(manifest).expect("install");

        let mut everything = PermissionSet::NONE;
        for capability in Capability::ALL {
            everything.request(capability);
        }
        registry.approve(id, everything).expect("approve");

        for capability in Capability::ALL {
            if capability.may_be_delegated_to_a_plugin() {
                continue;
            }
            assert!(
                registry.authorise(id, capability).is_err(),
                "a plugin reached {capability}"
            );
        }
    }
}
