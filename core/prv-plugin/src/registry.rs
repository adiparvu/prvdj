//! What is installed, what it was granted, and what the graph owes it.
//!
//! # Granted is not requested
//!
//! A manifest says what a plugin wants. The registry holds what the user
//! actually approved, and every authorisation question is answered from the
//! second. The two are separate fields rather than one mutated in place, so an
//! interface can show "asked for the network, not granted" — which is the state
//! a user needs to see to make sense of a plugin that is not working.
//!
//! # A bypass must not move the music in time
//!
//! This is the part that is easy to get wrong and impossible to miss once it
//! happens. If latency compensation counted only *running* plugins, bypassing
//! one mid-set would shorten the chain's reported latency and shift everything
//! against the beat — a plugin failing would knock the set out of time, which is
//! worse than the failure. So [`Registry::compensated_latency_frames`] counts
//! every plugin whose slot exists, running or not, and the pass-through delays
//! by the same amount the plugin declared.

use std::collections::BTreeMap;

use core::fmt;

use prv_security::{authorise, Capability, PermissionSet, PluginId, Refusal, Subject};

use crate::lifecycle::{advance, AudioBehaviour, LifecycleError, LifecycleEvent, LifecycleState};
use crate::manifest::Manifest;
use crate::watchdog::{BypassReason, Verdict, Watchdog};

/// Why the registry refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RegistryError {
    /// No plugin with that identity is installed.
    NotInstalled {
        /// Which.
        id: PluginId,
    },
    /// A plugin with that identity is already installed.
    AlreadyInstalled {
        /// Which.
        id: PluginId,
    },
    /// The registry holds as many plugins as it will.
    Full {
        /// How many that is.
        limit: usize,
    },
    /// The lifecycle refused the change.
    Lifecycle(LifecycleError),
}

impl fmt::Display for RegistryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::NotInstalled { id } => write!(f, "plugin {id} is not installed"),
            Self::AlreadyInstalled { id } => write!(f, "plugin {id} is already installed"),
            Self::Full { limit } => write!(f, "no more than {limit} plugins may be installed"),
            Self::Lifecycle(error) => write!(f, "{error}"),
        }
    }
}

impl core::error::Error for RegistryError {}

impl From<LifecycleError> for RegistryError {
    fn from(error: LifecycleError) -> Self {
        Self::Lifecycle(error)
    }
}

/// One installed plugin.
#[derive(Debug, Clone, PartialEq)]
pub struct Installed {
    manifest: Manifest,
    granted: PermissionSet,
    state: LifecycleState,
    watchdog: Watchdog,
    bypasses: u32,
}

impl Installed {
    /// What it says about itself.
    #[must_use]
    pub const fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    /// What the user approved.
    #[must_use]
    pub const fn granted(&self) -> PermissionSet {
        self.granted
    }

    /// Where it is in its life.
    #[must_use]
    pub const fn state(&self) -> LifecycleState {
        self.state
    }

    /// Its record of overruns.
    #[must_use]
    pub const fn watchdog(&self) -> Watchdog {
        self.watchdog
    }

    /// How many times it has been passed over.
    ///
    /// What an interface uses to stop offering to restart something that has
    /// failed four times, and what a user uses to decide to remove it.
    #[must_use]
    pub const fn bypasses(&self) -> u32 {
        self.bypasses
    }

    /// What the graph does with its slot right now.
    #[must_use]
    pub const fn audio_behaviour(&self) -> AudioBehaviour {
        self.state.audio_behaviour()
    }

    /// The latency its slot occupies, running or not.
    #[must_use]
    pub const fn slot_latency_frames(&self) -> u32 {
        match self.state {
            // A revoked plugin has no slot: it is gone, and the graph is rebuilt
            // without it. Everything else keeps its place in the chain.
            LifecycleState::Revoked => 0,
            _ => self.manifest.latency_frames(),
        }
    }

    /// Whether it asked for something the user did not grant.
    ///
    /// The state a user needs to see to make sense of a plugin that is not
    /// working: it is installed, it is running, and it cannot do the thing it
    /// was installed for.
    #[must_use]
    pub fn asked_for_more_than_it_got(&self) -> Vec<Capability> {
        self.manifest
            .requested()
            .held()
            .into_iter()
            .filter(|capability| !self.granted.contains(*capability))
            .collect()
    }
}

/// Everything installed.
#[derive(Debug, Clone, Default)]
pub struct Registry {
    plugins: BTreeMap<u64, Installed>,
}

impl Registry {
    /// How many plugins may be installed.
    ///
    /// Bounded because every one of them occupies a slot in a chain whose
    /// latency the graph compensates, and an unbounded chain is an unbounded
    /// delay. Generous enough that no real setup meets it.
    pub const MAX_PLUGINS: usize = 256;

    /// An empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Installs a plugin, granting it nothing.
    ///
    /// Nothing, deliberately: approval is a separate act by a person, and a
    /// registry that granted what a manifest asked for at install time would
    /// make the approval prompt decorative.
    ///
    /// # Errors
    ///
    /// Returns [`RegistryError`] if a plugin with that identity is already
    /// installed or the registry is full.
    pub fn install(&mut self, manifest: Manifest) -> Result<(), RegistryError> {
        let id = manifest.id();
        if self.plugins.contains_key(&id.get()) {
            return Err(RegistryError::AlreadyInstalled { id });
        }
        if self.plugins.len() >= Self::MAX_PLUGINS {
            return Err(RegistryError::Full {
                limit: Self::MAX_PLUGINS,
            });
        }
        self.plugins.insert(
            id.get(),
            Installed {
                manifest,
                granted: PermissionSet::NONE,
                state: LifecycleState::Discovered,
                watchdog: Watchdog::new(),
                bypasses: 0,
            },
        );
        Ok(())
    }

    /// Records what the user approved and moves the plugin on.
    ///
    /// The granted set is intersected with what the manifest asked for. A user
    /// interface cannot grant a capability the plugin never requested, because
    /// that would be the host inventing an authority nobody asked for — and
    /// because a plugin that later starts using it would be doing so without the
    /// user ever having read it in a prompt.
    ///
    /// # Errors
    ///
    /// Returns [`RegistryError`] if the plugin is not installed or the lifecycle
    /// refuses.
    pub fn approve(&mut self, id: PluginId, approved: PermissionSet) -> Result<(), RegistryError> {
        let installed = self
            .plugins
            .get_mut(&id.get())
            .ok_or(RegistryError::NotInstalled { id })?;

        let mut granted = PermissionSet::NONE;
        for capability in approved.held() {
            if installed.manifest.requested().contains(capability) {
                granted.request(capability);
            }
        }
        installed.granted = granted;
        installed.state = advance(installed.state, LifecycleEvent::Approve)?;
        Ok(())
    }

    /// Withdraws a capability from a plugin that has it.
    ///
    /// Immediate: the next authorisation question is answered from the new set.
    /// Master Prompt #23 requires permissions to be revocable, and a revocation
    /// that took effect at the next restart would leave the plugin using it for
    /// the rest of a live set.
    ///
    /// # Errors
    ///
    /// Returns [`RegistryError::NotInstalled`] if there is no such plugin.
    pub fn withdraw(&mut self, id: PluginId, capability: Capability) -> Result<(), RegistryError> {
        let installed = self
            .plugins
            .get_mut(&id.get())
            .ok_or(RegistryError::NotInstalled { id })?;
        installed.granted.revoke(capability);
        Ok(())
    }

    /// Applies a lifecycle event.
    ///
    /// # Errors
    ///
    /// Returns [`RegistryError`] if the plugin is not installed or the event
    /// means nothing where it is.
    pub fn advance(&mut self, id: PluginId, event: LifecycleEvent) -> Result<(), RegistryError> {
        let installed = self
            .plugins
            .get_mut(&id.get())
            .ok_or(RegistryError::NotInstalled { id })?;
        installed.state = advance(installed.state, event)?;
        if event == LifecycleEvent::Recover {
            installed.watchdog.recovered();
        }
        Ok(())
    }

    /// Records how a block went for a plugin, bypassing it if it has had enough
    /// chances.
    ///
    /// Returns the reason if this observation was the one that crossed the line,
    /// so a caller reports a bypass exactly once.
    ///
    /// # Errors
    ///
    /// Returns [`RegistryError::NotInstalled`] if there is no such plugin.
    pub fn observe(
        &mut self,
        id: PluginId,
        cost: u64,
        budget: u64,
    ) -> Result<Option<BypassReason>, RegistryError> {
        let installed = self
            .plugins
            .get_mut(&id.get())
            .ok_or(RegistryError::NotInstalled { id })?;

        match installed.watchdog.observe(cost, budget) {
            Verdict::Continue => Ok(None),
            Verdict::Bypass { reason } => {
                // The state machine may refuse — a plugin the user disabled
                // between blocks is no longer running. The watchdog's verdict
                // still stands; there is simply nothing left to bypass.
                if let Ok(next) = advance(installed.state, LifecycleEvent::Overran) {
                    installed.state = next;
                    installed.bypasses = installed.bypasses.saturating_add(1);
                }
                Ok(Some(reason))
            }
        }
    }

    /// Whether a plugin may do something.
    ///
    /// Answered by `prv-security` from the *granted* set. The decision lives
    /// there and this is the lookup that feeds it, which is what keeps one
    /// decision point rather than two.
    ///
    /// # Errors
    ///
    /// Returns [`Refusal`] naming what was refused. A plugin that is not
    /// installed, or not running, is refused as if it held nothing — an
    /// uninstalled plugin asking a question is a bug or an attack, and neither
    /// is a reason to answer yes.
    pub fn authorise(&self, id: PluginId, capability: Capability) -> Result<(), Refusal> {
        let permissions = self
            .plugins
            .get(&id.get())
            .filter(|installed| !installed.state.is_terminal())
            .map_or(PermissionSet::NONE, |installed| installed.granted);
        authorise(Subject::plugin(id, permissions), capability)
    }

    /// One plugin.
    #[must_use]
    pub fn get(&self, id: PluginId) -> Option<&Installed> {
        self.plugins.get(&id.get())
    }

    /// Every plugin, in identity order.
    pub fn installed(&self) -> impl Iterator<Item = &Installed> {
        self.plugins.values()
    }

    /// How many are installed.
    #[must_use]
    pub fn len(&self) -> usize {
        self.plugins.len()
    }

    /// Whether nothing is installed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.plugins.is_empty()
    }

    /// The latency the graph must compensate for, in frames.
    ///
    /// Counts every plugin that still occupies a slot, whether it is processing
    /// or being passed over. A bypass must not move the music in time.
    #[must_use]
    pub fn compensated_latency_frames(&self) -> u64 {
        self.plugins
            .values()
            .map(|installed| u64::from(installed.slot_latency_frames()))
            .sum()
    }

    /// Removes a plugin entirely.
    ///
    /// Distinct from revoking it: revocation leaves a record that the user
    /// withdrew it, and this is the record being deleted too. Returns whether
    /// anything was removed.
    pub fn uninstall(&mut self, id: PluginId) -> bool {
        self.plugins.remove(&id.get()).is_some()
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
    use crate::manifest::{Signature, Version};
    use crate::tier::IsolationTier;

    fn trusted() -> Signature {
        Signature::Trusted {
            fingerprint: "AB12".to_owned(),
        }
    }

    fn manifest(id: u64, wants: &[Capability], latency: u32) -> Manifest {
        let mut requested = PermissionSet::NONE;
        for capability in wants {
            requested.request(*capability);
        }
        Manifest::read(
            PluginId::new(id),
            "com.example.reverb",
            "Example Reverb",
            Version::new(1, 0, 0),
            IsolationTier::Wasm,
            requested.bits(),
            latency,
            Manifest::HOST_API_VERSION,
            trusted(),
        )
        .expect("a valid manifest")
    }

    fn permissions(of: &[Capability]) -> PermissionSet {
        let mut set = PermissionSet::NONE;
        for capability in of {
            set.request(*capability);
        }
        set
    }

    fn running(registry: &mut Registry, id: PluginId) {
        registry.advance(id, LifecycleEvent::Load).expect("load");
        registry.advance(id, LifecycleEvent::Start).expect("start");
    }

    #[test]
    fn a_plugin_is_installed_holding_nothing() {
        // A registry that granted what a manifest asked for would make the
        // approval prompt decorative.
        let mut registry = Registry::new();
        registry
            .install(manifest(1, &[Capability::UseNetwork], 0))
            .expect("install");

        let installed = registry.get(PluginId::new(1)).expect("installed");
        assert!(installed.granted().is_empty());
        assert_eq!(installed.state(), LifecycleState::Discovered);
        assert!(registry
            .authorise(PluginId::new(1), Capability::UseNetwork)
            .is_err());
    }

    #[test]
    fn a_plugin_gets_what_the_user_approved_and_never_more_than_it_asked_for() {
        // The host must not invent an authority nobody asked for: a plugin that
        // later started using it would be doing so without the user ever having
        // read it in a prompt.
        let mut registry = Registry::new();
        registry
            .install(manifest(1, &[Capability::UseNetwork], 0))
            .expect("install");
        registry
            .approve(
                PluginId::new(1),
                permissions(&[Capability::UseNetwork, Capability::WriteFiles]),
            )
            .expect("approve");

        assert!(registry
            .authorise(PluginId::new(1), Capability::UseNetwork)
            .is_ok());
        assert!(
            registry
                .authorise(PluginId::new(1), Capability::WriteFiles)
                .is_err(),
            "the host granted something the plugin never requested"
        );
    }

    #[test]
    fn a_plugin_that_asked_for_more_than_it_got_can_say_so() {
        // The state a user needs to make sense of a plugin that is installed,
        // running, and cannot do what it was installed for.
        let mut registry = Registry::new();
        registry
            .install(manifest(
                1,
                &[Capability::UseNetwork, Capability::WriteFiles],
                0,
            ))
            .expect("install");
        registry
            .approve(PluginId::new(1), permissions(&[Capability::UseNetwork]))
            .expect("approve");

        let installed = registry.get(PluginId::new(1)).expect("installed");
        assert_eq!(
            installed.asked_for_more_than_it_got(),
            vec![Capability::WriteFiles]
        );
    }

    #[test]
    fn withdrawing_a_permission_takes_effect_on_the_next_question() {
        // A revocation that took effect at the next restart would leave the
        // plugin using it for the rest of a live set.
        let mut registry = Registry::new();
        registry
            .install(manifest(1, &[Capability::UseNetwork], 0))
            .expect("install");
        registry
            .approve(PluginId::new(1), permissions(&[Capability::UseNetwork]))
            .expect("approve");
        assert!(registry
            .authorise(PluginId::new(1), Capability::UseNetwork)
            .is_ok());

        registry
            .withdraw(PluginId::new(1), Capability::UseNetwork)
            .expect("withdraw");
        assert!(registry
            .authorise(PluginId::new(1), Capability::UseNetwork)
            .is_err());
    }

    #[test]
    fn bypassing_a_plugin_does_not_move_the_music_in_time() {
        // The one that is impossible to miss once it happens: if compensation
        // counted only running plugins, a plugin failing would knock the set
        // out of time, which is worse than the failure.
        let mut registry = Registry::new();
        registry.install(manifest(1, &[], 512)).expect("install");
        registry
            .approve(PluginId::new(1), PermissionSet::NONE)
            .expect("approve");
        running(&mut registry, PluginId::new(1));
        assert_eq!(registry.compensated_latency_frames(), 512);

        for _ in 0..3 {
            registry
                .observe(PluginId::new(1), 200, 100)
                .expect("observe");
        }
        let installed = registry.get(PluginId::new(1)).expect("installed");
        assert_eq!(installed.state(), LifecycleState::Bypassed);
        assert_eq!(installed.audio_behaviour(), AudioBehaviour::PassesThrough);
        assert_eq!(
            registry.compensated_latency_frames(),
            512,
            "the chain's latency changed when a plugin was bypassed"
        );
    }

    #[test]
    fn a_revoked_plugin_gives_its_slot_back_and_answers_no_to_everything() {
        let mut registry = Registry::new();
        registry
            .install(manifest(1, &[Capability::UseNetwork], 256))
            .expect("install");
        registry
            .approve(PluginId::new(1), permissions(&[Capability::UseNetwork]))
            .expect("approve");
        running(&mut registry, PluginId::new(1));
        assert_eq!(registry.compensated_latency_frames(), 256);

        registry
            .advance(PluginId::new(1), LifecycleEvent::Revoke)
            .expect("revoke");
        assert_eq!(registry.compensated_latency_frames(), 0);
        assert!(registry
            .authorise(PluginId::new(1), Capability::UseNetwork)
            .is_err());
    }

    #[test]
    fn a_plugin_nobody_installed_is_refused_rather_than_answered() {
        // An uninstalled plugin asking a question is a bug or an attack, and
        // neither is a reason to answer yes.
        let registry = Registry::new();
        assert!(registry
            .authorise(PluginId::new(99), Capability::ViewLibrary)
            .is_err());
        assert_eq!(registry.get(PluginId::new(99)).map(Installed::state), None);
    }

    #[test]
    fn a_bypass_is_counted_and_recovery_clears_the_recent_record() {
        let mut registry = Registry::new();
        registry.install(manifest(1, &[], 0)).expect("install");
        registry
            .approve(PluginId::new(1), PermissionSet::NONE)
            .expect("approve");
        running(&mut registry, PluginId::new(1));

        let mut reported = 0;
        for _ in 0..10 {
            if registry
                .observe(PluginId::new(1), 200, 100)
                .expect("observe")
                .is_some()
            {
                reported += 1;
            }
        }
        assert_eq!(reported, 1, "the bypass was reported more than once");

        let installed = registry.get(PluginId::new(1)).expect("installed");
        assert_eq!(installed.bypasses(), 1);
        assert_eq!(installed.watchdog().overruns(), 10);

        registry
            .advance(PluginId::new(1), LifecycleEvent::Recover)
            .expect("recover");
        let recovered = registry.get(PluginId::new(1)).expect("installed");
        assert_eq!(recovered.state(), LifecycleState::Loaded);
        assert_eq!(recovered.watchdog().recent_overruns(), 0);
        assert_eq!(
            recovered.watchdog().overruns(),
            10,
            "a restart should not make a badly behaved plugin look new"
        );
        assert_eq!(recovered.bypasses(), 1, "the bypass count is its history");
    }

    #[test]
    fn installing_the_same_plugin_twice_is_refused() {
        let mut registry = Registry::new();
        registry.install(manifest(1, &[], 0)).expect("install");
        assert_eq!(
            registry.install(manifest(1, &[], 0)).err(),
            Some(RegistryError::AlreadyInstalled {
                id: PluginId::new(1)
            })
        );
        assert_eq!(registry.len(), 1);
        assert!(!registry.is_empty());
    }

    #[test]
    fn the_registry_is_bounded_because_the_chain_it_feeds_is() {
        let mut registry = Registry::new();
        for id in 0..Registry::MAX_PLUGINS {
            registry
                .install(manifest(id as u64, &[], 0))
                .expect("within the limit");
        }
        assert_eq!(
            registry.install(manifest(9999, &[], 0)).err(),
            Some(RegistryError::Full {
                limit: Registry::MAX_PLUGINS
            })
        );
    }

    #[test]
    fn uninstalling_removes_the_record_and_the_slot() {
        let mut registry = Registry::new();
        registry.install(manifest(1, &[], 128)).expect("install");
        assert_eq!(registry.compensated_latency_frames(), 128);

        assert!(registry.uninstall(PluginId::new(1)));
        assert!(!registry.uninstall(PluginId::new(1)));
        assert!(registry.is_empty());
        assert_eq!(registry.compensated_latency_frames(), 0);
        assert_eq!(registry.installed().count(), 0);
    }

    #[test]
    fn an_event_that_means_nothing_reaches_the_caller_as_an_error() {
        let mut registry = Registry::new();
        registry.install(manifest(1, &[], 0)).expect("install");
        assert!(matches!(
            registry.advance(PluginId::new(1), LifecycleEvent::Start),
            Err(RegistryError::Lifecycle(
                LifecycleError::NotApplicable { .. }
            ))
        ));
        assert_eq!(
            registry
                .advance(PluginId::new(7), LifecycleEvent::Load)
                .err(),
            Some(RegistryError::NotInstalled {
                id: PluginId::new(7)
            })
        );
    }
}
