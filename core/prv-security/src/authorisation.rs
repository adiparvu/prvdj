//! Who may do what.
//!
//! # One place, or it cannot be audited
//!
//! Master Prompt #26 requires authorisation rules to be centralised, and the
//! reason is worth stating rather than assuming: a permission check scattered
//! across forty call sites is not a rule, it is forty rules that happen to agree
//! today. Nobody can answer "what can a viewer do" by reading them, which means
//! nobody can answer it at all.
//!
//! [`authorise`] is the only decision point. Everything else in the product asks
//! it and obeys the answer.
//!
//! # This is not entitlement, and the two never merge
//!
//! `prv-entitlements` answers *what did this person pay for*. This module
//! answers *is this person allowed*. They are different axes and they fail
//! differently: an entitlement problem is solved by an upgrade, an
//! authorisation problem is solved by the owner of the project, and offering
//! the wrong remedy is worse than offering none. There is no dependency between
//! the two crates in either direction, deliberately.
//!
//! # Some authority is never delegated
//!
//! ADR-0005 gives plugins a declared, signed, revocable permission manifest.
//! [`Capability::may_be_delegated_to_a_plugin`] marks the capabilities that no
//! manifest can request whatever it is signed with: reading secrets, changing
//! what the user has consented to, changing the licence, managing
//! collaborators, deleting projects, installing further plugins. A sandbox that
//! can grant itself permissions is not a sandbox, and every one of these is a
//! way to do exactly that.
//!
//! The rule is enforced twice — [`PermissionSet::request`] refuses to store one,
//! and [`authorise`] refuses to honour one. The redundancy is deliberate: the
//! first makes the invariant true of the data, and the second keeps it true if
//! a future constructor forgets.

use core::fmt;

/// Something a subject might be permitted to do.
///
/// One vocabulary for users and plugins alike. Two vocabularies would be two
/// decision points wearing one name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum Capability {
    /// Open a project and see its contents.
    ViewProject,
    /// Change a project.
    EditProject,
    /// Render a project to a file.
    ExportProject,
    /// Make a project reachable by someone else.
    ShareProject,
    /// Remove a project.
    DeleteProject,
    /// Add or remove the people who can open a project.
    ManageCollaborators,

    /// Browse and search the library.
    ViewLibrary,
    /// Add, remove or re-tag library entries.
    ModifyLibrary,

    /// Read audio as it plays.
    ReadAudioStream,
    /// Write files outside the application's own storage.
    WriteFiles,
    /// Open a network connection.
    UseNetwork,

    /// Read credential material.
    ReadSecrets,
    /// Change what the user has agreed to.
    ManageConsent,
    /// Change the licence in force.
    ManageLicence,
    /// Load further plugins.
    InstallPlugin,
}

impl Capability {
    /// Every capability, so that a permission screen cannot omit one.
    pub const ALL: [Self; 15] = [
        Self::ViewProject,
        Self::EditProject,
        Self::ExportProject,
        Self::ShareProject,
        Self::DeleteProject,
        Self::ManageCollaborators,
        Self::ViewLibrary,
        Self::ModifyLibrary,
        Self::ReadAudioStream,
        Self::WriteFiles,
        Self::UseNetwork,
        Self::ReadSecrets,
        Self::ManageConsent,
        Self::ManageLicence,
        Self::InstallPlugin,
    ];

    /// This capability's fixed position in a stored permission set.
    ///
    /// Written out rather than derived from declaration order, because a
    /// permission set is stored in a signed plugin manifest and a manifest
    /// signed last year must keep its meaning. Reordering the variants above
    /// must not silently re-interpret anybody's permissions.
    #[must_use]
    pub const fn bit(self) -> u32 {
        match self {
            Self::ViewProject => 0,
            Self::EditProject => 1,
            Self::ExportProject => 2,
            Self::ShareProject => 3,
            Self::DeleteProject => 4,
            Self::ManageCollaborators => 5,
            Self::ViewLibrary => 6,
            Self::ModifyLibrary => 7,
            Self::ReadAudioStream => 8,
            Self::WriteFiles => 9,
            Self::UseNetwork => 10,
            Self::ReadSecrets => 11,
            Self::ManageConsent => 12,
            Self::ManageLicence => 13,
            Self::InstallPlugin => 14,
        }
    }

    /// Whether a plugin manifest may ask for this at all.
    ///
    /// False for the six that would let sandboxed code widen its own authority
    /// or reach the user's credentials. A sandbox that can grant itself
    /// permissions is not a sandbox.
    #[must_use]
    pub const fn may_be_delegated_to_a_plugin(self) -> bool {
        !matches!(
            self,
            Self::ReadSecrets
                | Self::ManageConsent
                | Self::ManageLicence
                | Self::ManageCollaborators
                | Self::DeleteProject
                | Self::InstallPlugin
        )
    }

    /// A stable identifier, for storage and localisation.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::ViewProject => "capability.view_project",
            Self::EditProject => "capability.edit_project",
            Self::ExportProject => "capability.export_project",
            Self::ShareProject => "capability.share_project",
            Self::DeleteProject => "capability.delete_project",
            Self::ManageCollaborators => "capability.manage_collaborators",
            Self::ViewLibrary => "capability.view_library",
            Self::ModifyLibrary => "capability.modify_library",
            Self::ReadAudioStream => "capability.read_audio_stream",
            Self::WriteFiles => "capability.write_files",
            Self::UseNetwork => "capability.use_network",
            Self::ReadSecrets => "capability.read_secrets",
            Self::ManageConsent => "capability.manage_consent",
            Self::ManageLicence => "capability.manage_licence",
            Self::InstallPlugin => "capability.install_plugin",
        }
    }
}

impl fmt::Display for Capability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.key())
    }
}

/// What a person is to a project.
///
/// Ordered, and cumulative: a higher role grants everything a lower one does.
/// The alternative — a set of capabilities per role — reads more flexible and
/// makes "did the upgrade take anything away" a question nobody can answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum Role {
    /// Can open it and nothing else.
    Viewer,
    /// Can change it and get it out.
    Editor,
    /// Made it.
    Owner,
}

impl Role {
    /// Every role, least authority first.
    pub const ALL: [Self; 3] = [Self::Viewer, Self::Editor, Self::Owner];

    /// Whether this role carries a capability.
    ///
    /// A few capabilities exist mainly to constrain plugins —
    /// [`Capability::UseNetwork`], [`Capability::WriteFiles`],
    /// [`Capability::ReadSecrets`]. A role still has to answer for them,
    /// because the vocabulary is shared, and the answer is that they are the
    /// owner's device and the owner's authority: someone who was invited to
    /// edit a set was not invited to reach the machine it is on.
    #[must_use]
    pub const fn grants(self, capability: Capability) -> bool {
        match capability {
            // Reading is what every role is for.
            Capability::ViewProject | Capability::ViewLibrary => true,

            // Making and exporting work. Export is here rather than reserved to
            // the owner because getting work out is not a privilege — an editor
            // who contributed to a set can take a copy of it.
            Capability::EditProject
            | Capability::ExportProject
            | Capability::ModifyLibrary
            | Capability::ReadAudioStream => matches!(self, Self::Editor | Self::Owner),

            // Decisions about the project as a possession, not as a document.
            Capability::ShareProject
            | Capability::DeleteProject
            | Capability::ManageCollaborators
            | Capability::ManageConsent
            | Capability::ManageLicence
            | Capability::InstallPlugin
            | Capability::WriteFiles
            | Capability::UseNetwork
            | Capability::ReadSecrets => matches!(self, Self::Owner),
        }
    }

    /// A stable identifier, for storage and localisation.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Viewer => "role.viewer",
            Self::Editor => "role.editor",
            Self::Owner => "role.owner",
        }
    }
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.key())
    }
}

/// Which plugin.
///
/// Opaque and numeric: an identifier for a piece of software, never for a
/// person, so it is safe to put in a diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PluginId(u64);

impl PluginId {
    /// Names a plugin.
    #[must_use]
    pub const fn new(id: u64) -> Self {
        Self(id)
    }

    /// The underlying value.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl fmt::Display for PluginId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// The permissions a plugin's manifest declares and the user approved.
///
/// A bit set, because it is stored inside a signed manifest and a compact fixed
/// representation is one fewer thing for a signature to disagree about.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct PermissionSet {
    bits: u32,
}

impl PermissionSet {
    /// No permissions at all — where every plugin starts.
    pub const NONE: Self = Self { bits: 0 };

    /// Adds a permission, if it is one a plugin may ever hold.
    ///
    /// Returns whether it was added. A capability that
    /// [`Capability::may_be_delegated_to_a_plugin`] refuses is *not stored*,
    /// so no later reader has to remember to re-check it.
    pub fn request(&mut self, capability: Capability) -> bool {
        if !capability.may_be_delegated_to_a_plugin() {
            return false;
        }
        self.bits |= 1_u32 << capability.bit();
        true
    }

    /// Removes a permission.
    ///
    /// Every permission is revocable, which ADR-0005 requires and which is the
    /// half of a permission model that users actually exercise.
    pub fn revoke(&mut self, capability: Capability) {
        self.bits &= !(1_u32 << capability.bit());
    }

    /// Whether the set holds a permission.
    #[must_use]
    pub const fn contains(self, capability: Capability) -> bool {
        self.bits & (1_u32 << capability.bit()) != 0
    }

    /// The stored form, for a manifest.
    #[must_use]
    pub const fn bits(self) -> u32 {
        self.bits
    }

    /// Reads a stored form, discarding anything a plugin may never hold.
    ///
    /// Where a manifest from outside the process arrives. A set that asked for
    /// authority over secrets is not an error to report and refuse the plugin
    /// over — it is a claim we simply do not honour, so the bits are dropped
    /// and what remains is what the plugin gets.
    #[must_use]
    pub fn from_bits(bits: u32) -> Self {
        let mut set = Self::NONE;
        for capability in Capability::ALL {
            if bits & (1_u32 << capability.bit()) != 0 {
                set.request(capability);
            }
        }
        set
    }

    /// Every permission held, in capability order.
    #[must_use]
    pub fn held(self) -> Vec<Capability> {
        Capability::ALL
            .into_iter()
            .filter(|capability| self.contains(*capability))
            .collect()
    }

    /// Whether nothing is held.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.bits == 0
    }
}

/// Who is asking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Subject {
    /// A person, in the role they hold on the thing they are acting on.
    ///
    /// Resolving *which* role someone holds is a lookup, and a lookup is
    /// infrastructure. This module decides; it does not fetch.
    User {
        /// What they are to this project.
        role: Role,
    },
    /// Sandboxed third-party code.
    Plugin {
        /// Which plugin, for the audit record and for revocation.
        id: PluginId,
        /// What its manifest declared and the user approved.
        permissions: PermissionSet,
    },
}

impl Subject {
    /// A person in a role.
    #[must_use]
    pub const fn user(role: Role) -> Self {
        Self::User { role }
    }

    /// A plugin with its approved permissions.
    #[must_use]
    pub const fn plugin(id: PluginId, permissions: PermissionSet) -> Self {
        Self::Plugin { id, permissions }
    }
}

/// Why a request was refused.
///
/// Every variant names what would change the answer, because Master Prompt #10
/// requires an error to explain and "not permitted" explains nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Refusal {
    /// The person's role on this project does not carry the capability.
    RoleLacksCapability {
        /// What they are.
        role: Role,
        /// What they asked to do.
        capability: Capability,
    },
    /// The plugin's manifest did not declare this, or the user did not approve
    /// it.
    PluginNotPermitted {
        /// Which plugin.
        id: PluginId,
        /// What it asked to do.
        capability: Capability,
    },
    /// No plugin may ever hold this, at any signature, with any approval.
    NeverDelegated {
        /// Which plugin.
        id: PluginId,
        /// What it asked to do.
        capability: Capability,
    },
}

impl Refusal {
    /// What was refused.
    #[must_use]
    pub const fn capability(self) -> Capability {
        match self {
            Self::RoleLacksCapability { capability, .. }
            | Self::PluginNotPermitted { capability, .. }
            | Self::NeverDelegated { capability, .. } => capability,
        }
    }

    /// Whether the user could change the answer by approving something.
    ///
    /// False for [`Self::NeverDelegated`], which is the point of that variant:
    /// an interface must not offer a permission prompt that cannot be honoured.
    #[must_use]
    pub const fn is_addressable_by_the_user(self) -> bool {
        !matches!(self, Self::NeverDelegated { .. })
    }

    /// A stable identifier, for storage and localisation.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::RoleLacksCapability { .. } => "refusal.role_lacks_capability",
            Self::PluginNotPermitted { .. } => "refusal.plugin_not_permitted",
            Self::NeverDelegated { .. } => "refusal.never_delegated",
        }
    }
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({})", self.key(), self.capability())
    }
}

impl core::error::Error for Refusal {}

/// The only authorisation decision in the product.
///
/// # Errors
///
/// Returns [`Refusal`], which names what was refused and whether approving
/// something would change the answer.
pub fn authorise(subject: Subject, capability: Capability) -> Result<(), Refusal> {
    match subject {
        Subject::User { role } => {
            if role.grants(capability) {
                Ok(())
            } else {
                Err(Refusal::RoleLacksCapability { role, capability })
            }
        }
        Subject::Plugin { id, permissions } => {
            // Checked here as well as in `PermissionSet::request`. The set
            // cannot hold one of these today; this keeps the guarantee if a
            // future constructor forgets, and costs one comparison.
            if !capability.may_be_delegated_to_a_plugin() {
                return Err(Refusal::NeverDelegated { id, capability });
            }
            if permissions.contains(capability) {
                Ok(())
            } else {
                Err(Refusal::PluginNotPermitted { id, capability })
            }
        }
    }
}

/// Whether a subject may do something.
#[must_use]
pub fn allows(subject: Subject, capability: Capability) -> bool {
    authorise(subject, capability).is_ok()
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::panic,
        reason = "a test that cannot build its own fixture should fail loudly"
    )]

    use super::*;

    const PLUGIN: PluginId = PluginId::new(3);

    #[test]
    fn roles_are_cumulative() {
        // A higher role must never grant less. Getting this wrong would be
        // invisible in any single check and obvious to the one collaborator who
        // was promoted and lost something.
        for capability in Capability::ALL {
            let mut previously_granted = false;
            for role in Role::ALL {
                let granted = role.grants(capability);
                assert!(
                    granted || !previously_granted,
                    "{role} withdrew {capability}, which a lower role granted"
                );
                previously_granted = granted;
            }
        }
    }

    #[test]
    fn a_viewer_can_look_and_do_nothing_else() {
        // Written as "exactly these two and nothing else" rather than as a list
        // of things to refuse, so that a capability added later is refused to a
        // viewer by default and this test is what says so.
        let viewer = Subject::user(Role::Viewer);
        for capability in Capability::ALL {
            let expected = matches!(
                capability,
                Capability::ViewProject | Capability::ViewLibrary
            );
            assert_eq!(
                allows(viewer, capability),
                expected,
                "a viewer's answer for {capability} is wrong"
            );
        }
        assert_eq!(
            authorise(viewer, Capability::EditProject).err(),
            Some(Refusal::RoleLacksCapability {
                role: Role::Viewer,
                capability: Capability::EditProject
            })
        );
    }

    #[test]
    fn an_editor_can_take_a_copy_of_what_they_worked_on() {
        // Export is not a privilege. Someone who contributed to a set can get
        // it out; reserving that to the owner would make the product a place
        // work goes in and does not come out.
        let editor = Subject::user(Role::Editor);
        assert!(allows(editor, Capability::ExportProject));
        assert!(allows(editor, Capability::EditProject));
        assert!(!allows(editor, Capability::DeleteProject));
        assert!(!allows(editor, Capability::ManageCollaborators));
    }

    #[test]
    fn the_owner_can_do_everything() {
        let owner = Subject::user(Role::Owner);
        for capability in Capability::ALL {
            assert!(
                allows(owner, capability),
                "the owner could not {capability}"
            );
        }
    }

    #[test]
    fn no_manifest_can_hold_authority_over_secrets_consent_or_licensing() {
        // A sandbox that can grant itself permissions is not a sandbox, and
        // each of these is a way to do exactly that.
        let forbidden = [
            Capability::ReadSecrets,
            Capability::ManageConsent,
            Capability::ManageLicence,
            Capability::ManageCollaborators,
            Capability::DeleteProject,
            Capability::InstallPlugin,
        ];
        for capability in forbidden {
            assert!(!capability.may_be_delegated_to_a_plugin(), "{capability}");

            let mut permissions = PermissionSet::NONE;
            assert!(
                !permissions.request(capability),
                "a manifest was allowed to request {capability}"
            );
            assert!(!permissions.contains(capability));

            assert_eq!(
                authorise(Subject::plugin(PLUGIN, permissions), capability).err(),
                Some(Refusal::NeverDelegated {
                    id: PLUGIN,
                    capability
                })
            );
        }
    }

    #[test]
    fn a_manifest_from_outside_cannot_smuggle_forbidden_bits_in() {
        // Where a signed manifest arrives from somewhere we did not write. A
        // set claiming everything is not an error to refuse the plugin over —
        // it is a claim we do not honour.
        let smuggled = PermissionSet::from_bits(u32::MAX);
        for capability in Capability::ALL {
            assert_eq!(
                smuggled.contains(capability),
                capability.may_be_delegated_to_a_plugin(),
                "{capability} survived the wrong way"
            );
        }
        assert!(!smuggled.is_empty());
    }

    #[test]
    fn a_plugin_gets_exactly_what_was_approved() {
        let mut permissions = PermissionSet::NONE;
        assert!(permissions.request(Capability::ReadAudioStream));
        let plugin = Subject::plugin(PLUGIN, permissions);

        assert!(allows(plugin, Capability::ReadAudioStream));
        assert_eq!(
            authorise(plugin, Capability::UseNetwork).err(),
            Some(Refusal::PluginNotPermitted {
                id: PLUGIN,
                capability: Capability::UseNetwork
            })
        );
        assert_eq!(permissions.held(), vec![Capability::ReadAudioStream]);
    }

    #[test]
    fn revoking_a_permission_takes_effect() {
        // The half of a permission model users actually exercise.
        let mut permissions = PermissionSet::NONE;
        permissions.request(Capability::UseNetwork);
        permissions.request(Capability::WriteFiles);
        permissions.revoke(Capability::UseNetwork);

        assert!(!allows(
            Subject::plugin(PLUGIN, permissions),
            Capability::UseNetwork
        ));
        assert!(allows(
            Subject::plugin(PLUGIN, permissions),
            Capability::WriteFiles
        ));
    }

    #[test]
    fn a_plugin_starts_with_nothing() {
        let plugin = Subject::plugin(PLUGIN, PermissionSet::NONE);
        assert!(PermissionSet::NONE.is_empty());
        assert!(PermissionSet::default().is_empty());
        for capability in Capability::ALL {
            assert!(
                !allows(plugin, capability),
                "a bare plugin could {capability}"
            );
        }
    }

    #[test]
    fn a_refusal_says_whether_asking_the_user_would_help() {
        // An interface must not offer a permission prompt that cannot be
        // honoured.
        let permitted_but_unapproved = authorise(
            Subject::plugin(PLUGIN, PermissionSet::NONE),
            Capability::UseNetwork,
        );
        assert!(permitted_but_unapproved
            .err()
            .is_some_and(Refusal::is_addressable_by_the_user));

        let never = authorise(
            Subject::plugin(PLUGIN, PermissionSet::NONE),
            Capability::ReadSecrets,
        );
        assert!(never
            .err()
            .is_some_and(|refusal| !refusal.is_addressable_by_the_user()));
    }

    #[test]
    fn stored_permission_bits_do_not_move() {
        // A permission set lives in a signed manifest, and a manifest signed
        // last year must keep its meaning. If this test fails because the
        // variants were reordered, the fix is to restore the numbers, not to
        // update the expectation.
        assert_eq!(Capability::ViewProject.bit(), 0);
        assert_eq!(Capability::ReadAudioStream.bit(), 8);
        assert_eq!(Capability::InstallPlugin.bit(), 14);

        let mut seen = 0_u32;
        for capability in Capability::ALL {
            let bit = capability.bit();
            assert!(
                bit < 32,
                "{capability} does not fit in a 32-bit permission set"
            );
            let mask = 1_u32 << bit;
            assert!(seen & mask == 0, "two capabilities share bit {bit}");
            seen |= mask;
        }
        assert_eq!(Capability::ALL.len(), 15);
        assert_eq!(
            seen.count_ones(),
            15,
            "a capability is missing from the list"
        );
    }

    #[test]
    fn a_permission_set_round_trips_through_its_stored_form() {
        let mut permissions = PermissionSet::NONE;
        permissions.request(Capability::ReadAudioStream);
        permissions.request(Capability::UseNetwork);
        assert_eq!(PermissionSet::from_bits(permissions.bits()), permissions);
    }

    #[test]
    fn capability_and_role_keys_are_distinct() {
        let keys: Vec<&str> = Capability::ALL.iter().map(|c| c.key()).collect();
        for (index, key) in keys.iter().enumerate() {
            for (other, value) in keys.iter().enumerate() {
                assert!(
                    index == other || key != value,
                    "two capabilities share {key}"
                );
            }
        }

        let roles: Vec<&str> = Role::ALL.iter().map(|r| r.key()).collect();
        for (index, key) in roles.iter().enumerate() {
            for (other, value) in roles.iter().enumerate() {
                assert!(index == other || key != value, "two roles share {key}");
            }
        }
    }
}
