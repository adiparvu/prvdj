//! What a plugin declares about itself.
//!
//! # A manifest is a claim, not a fact
//!
//! Everything here came from outside the process and is therefore an assertion
//! by the author, checked where it can be checked and distrusted where it cannot.
//! The permissions are narrowed to what a plugin may ever hold before they are
//! stored (`prv-security`); the declared latency is bounded; the identifier is
//! validated; and the signature is *recorded rather than believed* — verifying
//! one needs a cryptographic implementation and a trust root, both of which sit
//! outside a crate with no dependencies.
//!
//! # Unsigned code is never loaded without the user saying so
//!
//! Master Prompt #26 requires it, and [`Manifest::may_load_without_asking`] is
//! where it is decided. The answer is no for anything not signed by a trusted
//! key, at every tier, whatever else the manifest says.

use core::fmt;

use prv_security::{Capability, PermissionSet, PluginId};

use crate::tier::IsolationTier;

/// What the platform found when it checked the code's signature.
///
/// Recorded, not decided. Verification is the platform's; this is the answer it
/// came back with, and the policy that reads it lives here so that the policy is
/// in one place rather than in whichever loader ran first.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Signature {
    /// Signed by a key the platform trusts.
    Trusted {
        /// A printable fingerprint of the signing key, for the user to compare.
        ///
        /// Not a secret: a public key's fingerprint is meant to be published,
        /// and showing it is how a user tells one publisher from another.
        fingerprint: String,
    },
    /// Signed, but not by anything the platform trusts.
    ///
    /// Distinct from unsigned because it is *worse*: something went to the
    /// trouble of appearing signed. An interface should say so differently.
    Untrusted,
    /// Not signed at all.
    Unsigned,
}

impl Signature {
    /// Whether the platform vouched for the signature.
    #[must_use]
    pub const fn is_trusted(&self) -> bool {
        matches!(self, Self::Trusted { .. })
    }

    /// A stable identifier, for storage and localisation.
    #[must_use]
    pub const fn key(&self) -> &'static str {
        match self {
            Self::Trusted { .. } => "signature.trusted",
            Self::Untrusted => "signature.untrusted",
            Self::Unsigned => "signature.unsigned",
        }
    }
}

/// A published version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version {
    /// Incompatible changes.
    pub major: u16,
    /// Compatible additions.
    pub minor: u16,
    /// Fixes.
    pub patch: u16,
}

impl Version {
    /// A version.
    #[must_use]
    pub const fn new(major: u16, minor: u16, patch: u16) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// Why a manifest was not accepted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ManifestError {
    /// The identifier was empty, too long, or not in reverse-domain form.
    IdentifierNotUsable,
    /// The display name was empty or too long.
    NameNotUsable,
    /// The declared latency exceeds what the graph will compensate.
    LatencyTooHigh {
        /// What was declared, in frames.
        declared: u32,
        /// The most that will be compensated.
        limit: u32,
    },
    /// The plugin claimed a host interface version this build does not speak.
    UnsupportedApi {
        /// What it claimed.
        claimed: u16,
        /// What this build speaks.
        supported: u16,
    },
    /// A tier that only certification opens was claimed by a manifest.
    ///
    /// The manifest is the wrong place to ask: certification is a review, and a
    /// review that a file can assert it passed is not a review.
    TierNotSelfAssignable {
        /// Which tier.
        tier: IsolationTier,
    },
}

impl fmt::Display for ManifestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::IdentifierNotUsable => f.write_str("the plugin identifier is not usable"),
            Self::NameNotUsable => f.write_str("the plugin name is not usable"),
            Self::LatencyTooHigh { declared, limit } => write!(
                f,
                "a declared latency of {declared} frames exceeds the limit of {limit}"
            ),
            Self::UnsupportedApi { claimed, supported } => write!(
                f,
                "the plugin asks for host interface {claimed}; this build speaks {supported}"
            ),
            Self::TierNotSelfAssignable { tier } => {
                write!(f, "{tier} is opened by certification, not by a manifest")
            }
        }
    }
}

impl core::error::Error for ManifestError {}

/// Everything a plugin says about itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    id: PluginId,
    identifier: String,
    name: String,
    version: Version,
    tier: IsolationTier,
    requested: PermissionSet,
    latency_frames: u32,
    signature: Signature,
}

impl Manifest {
    /// The host interface version this build speaks.
    pub const HOST_API_VERSION: u16 = 1;

    /// The most latency the graph will compensate, in frames.
    ///
    /// Two seconds at 48 kHz. Beyond that a plugin is not adding latency, it is
    /// adding a delay, and compensating it would silently push everything else
    /// in the project backwards by a length the user never asked for.
    pub const MAX_LATENCY_FRAMES: u32 = 96_000;

    /// The longest identifier accepted.
    pub const MAX_IDENTIFIER_BYTES: usize = 128;

    /// The longest display name accepted.
    pub const MAX_NAME_BYTES: usize = 64;

    /// Reads a manifest, refusing what cannot be honoured.
    ///
    /// The permission set is *narrowed* rather than refused: a manifest that
    /// asks for authority over the user's credentials is not an error to reject
    /// the plugin over, it is a claim that is not honoured. Refusing the whole
    /// plugin would let a hostile manifest deny service by asking for something
    /// it knows it cannot have.
    ///
    /// # Errors
    ///
    /// Returns [`ManifestError`] for an unusable identifier or name, a latency
    /// beyond [`Self::MAX_LATENCY_FRAMES`], an unsupported host interface
    /// version, or a self-assigned certified tier.
    #[allow(
        clippy::too_many_arguments,
        reason = "a manifest is a record with this many fields; grouping them into \
                  a builder would hide which are validated"
    )]
    pub fn read(
        id: PluginId,
        identifier: &str,
        name: &str,
        version: Version,
        tier: IsolationTier,
        requested_bits: u32,
        latency_frames: u32,
        api_version: u16,
        signature: Signature,
    ) -> Result<Self, ManifestError> {
        if api_version != Self::HOST_API_VERSION {
            return Err(ManifestError::UnsupportedApi {
                claimed: api_version,
                supported: Self::HOST_API_VERSION,
            });
        }
        if !is_reverse_domain(identifier) {
            return Err(ManifestError::IdentifierNotUsable);
        }
        if name.trim().is_empty() || name.len() > Self::MAX_NAME_BYTES {
            return Err(ManifestError::NameNotUsable);
        }
        if tier.requires_certification() {
            return Err(ManifestError::TierNotSelfAssignable { tier });
        }
        if latency_frames > Self::MAX_LATENCY_FRAMES {
            return Err(ManifestError::LatencyTooHigh {
                declared: latency_frames,
                limit: Self::MAX_LATENCY_FRAMES,
            });
        }
        if !tier.runs_on_the_audio_thread() && latency_frames != 0 {
            // A plugin that is not on the audio path has no processing latency
            // to compensate. Taking its word for one would make the graph delay
            // everything else to line up with something that is not in it.
            return Ok(Self::build(
                id,
                identifier,
                name,
                version,
                tier,
                requested_bits,
                0,
                signature,
            ));
        }
        Ok(Self::build(
            id,
            identifier,
            name,
            version,
            tier,
            requested_bits,
            latency_frames,
            signature,
        ))
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "private constructor mirroring the validated record"
    )]
    fn build(
        id: PluginId,
        identifier: &str,
        name: &str,
        version: Version,
        tier: IsolationTier,
        requested_bits: u32,
        latency_frames: u32,
        signature: Signature,
    ) -> Self {
        Self {
            id,
            identifier: identifier.to_owned(),
            name: name.to_owned(),
            version,
            tier,
            requested: PermissionSet::from_bits(requested_bits),
            latency_frames,
            signature,
        }
    }

    /// Promotes a manifest into the certified native tier.
    ///
    /// Deliberately a separate call rather than a field a manifest can set. It
    /// is the certification programme's decision expressed in code, and the
    /// caller is a part of the build that has the review's outcome — not a file
    /// found on disk.
    #[must_use]
    pub fn certified_as_native(mut self) -> Self {
        self.tier = IsolationTier::Native;
        self
    }

    /// Which plugin.
    #[must_use]
    pub const fn id(&self) -> PluginId {
        self.id
    }

    /// Its reverse-domain identifier.
    #[must_use]
    pub fn identifier(&self) -> &str {
        &self.identifier
    }

    /// What it calls itself.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Its version.
    #[must_use]
    pub const fn version(&self) -> Version {
        self.version
    }

    /// How it is isolated.
    #[must_use]
    pub const fn tier(&self) -> IsolationTier {
        self.tier
    }

    /// What it asked for, after everything it may never hold was removed.
    #[must_use]
    pub const fn requested(&self) -> PermissionSet {
        self.requested
    }

    /// The processing latency it declares, in frames.
    #[must_use]
    pub const fn latency_frames(&self) -> u32 {
        self.latency_frames
    }

    /// What the platform found when it checked the signature.
    #[must_use]
    pub const fn signature(&self) -> &Signature {
        &self.signature
    }

    /// Whether this may be loaded without asking the user first.
    ///
    /// No, unless the signature is trusted. Master Prompt #26 requires explicit
    /// approval for unsigned code, and the same applies to code signed by
    /// something we do not recognise — which is worse, not better, because
    /// somebody went to the trouble of making it look signed.
    #[must_use]
    pub const fn may_load_without_asking(&self) -> bool {
        self.signature.is_trusted()
    }

    /// Whether it asked for anything at all.
    #[must_use]
    pub fn asks_for_nothing(&self) -> bool {
        self.requested.is_empty()
    }

    /// Whether it asked for a capability.
    #[must_use]
    pub fn asks_for(&self, capability: Capability) -> bool {
        self.requested.contains(capability)
    }
}

/// Whether an identifier is in usable reverse-domain form.
///
/// Lower-case letters, digits, hyphens and dots, at least two parts, no empty
/// part. Narrow on purpose: an identifier is a key in storage, in a settings
/// document and in a support conversation, and one that differs from another
/// only by case or by an invisible character is a defect that surfaces as
/// "sometimes my plugin disappears".
fn is_reverse_domain(identifier: &str) -> bool {
    if identifier.is_empty() || identifier.len() > Manifest::MAX_IDENTIFIER_BYTES {
        return false;
    }
    if !identifier.bytes().all(|byte| {
        byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-' || byte == b'.'
    }) {
        return false;
    }
    let parts: Vec<&str> = identifier.split('.').collect();
    parts.len() >= 2 && parts.iter().all(|part| !part.is_empty())
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::panic,
        reason = "a test that cannot build its own fixture should fail loudly"
    )]

    use super::*;

    fn trusted() -> Signature {
        Signature::Trusted {
            fingerprint: "AB12 CD34".to_owned(),
        }
    }

    fn manifest(tier: IsolationTier, bits: u32, signature: Signature) -> Manifest {
        Manifest::read(
            PluginId::new(1),
            "com.example.reverb",
            "Example Reverb",
            Version::new(1, 0, 0),
            tier,
            bits,
            64,
            Manifest::HOST_API_VERSION,
            signature,
        )
        .expect("a valid manifest")
    }

    #[test]
    fn unsigned_code_is_never_loaded_without_asking() {
        // Master Prompt #26. Untrusted is worse than unsigned, not better:
        // somebody went to the trouble of making it look signed.
        for signature in [Signature::Unsigned, Signature::Untrusted] {
            let plugin = manifest(IsolationTier::Wasm, 0, signature);
            assert!(
                !plugin.may_load_without_asking(),
                "{} was loadable without approval",
                plugin.signature().key()
            );
        }
        assert!(manifest(IsolationTier::Wasm, 0, trusted()).may_load_without_asking());
    }

    #[test]
    fn a_manifest_asking_for_forbidden_authority_is_narrowed_rather_than_refused() {
        // Refusing the whole plugin would let a hostile manifest deny service by
        // asking for something it knows it cannot have.
        let plugin = manifest(IsolationTier::Wasm, u32::MAX, trusted());
        assert!(plugin.asks_for(Capability::UseNetwork));
        assert!(!plugin.asks_for(Capability::ReadSecrets));
        assert!(!plugin.asks_for(Capability::ManageConsent));
        assert!(!plugin.asks_for(Capability::InstallPlugin));
    }

    #[test]
    fn the_certified_tier_cannot_be_claimed_by_a_file() {
        // A review that a file can assert it passed is not a review.
        let refused = Manifest::read(
            PluginId::new(1),
            "com.example.reverb",
            "Example Reverb",
            Version::new(1, 0, 0),
            IsolationTier::Native,
            0,
            0,
            Manifest::HOST_API_VERSION,
            trusted(),
        );
        assert_eq!(
            refused.err(),
            Some(ManifestError::TierNotSelfAssignable {
                tier: IsolationTier::Native
            })
        );

        let promoted = manifest(IsolationTier::Wasm, 0, trusted()).certified_as_native();
        assert_eq!(promoted.tier(), IsolationTier::Native);
    }

    #[test]
    fn a_plugin_off_the_audio_path_declares_no_latency_whatever_it_says() {
        // Taking its word would make the graph delay everything else to line up
        // with something that is not in it.
        let plugin = manifest(IsolationTier::OutOfProcess, 0, trusted());
        assert_eq!(plugin.latency_frames(), 0);

        let on_the_path = manifest(IsolationTier::Wasm, 0, trusted());
        assert_eq!(on_the_path.latency_frames(), 64);
    }

    #[test]
    fn latency_beyond_what_the_graph_will_compensate_is_refused() {
        // Beyond the limit a plugin is not adding latency, it is adding a delay,
        // and compensating it would push everything else backwards by a length
        // the user never asked for.
        let refused = Manifest::read(
            PluginId::new(1),
            "com.example.reverb",
            "Example Reverb",
            Version::new(1, 0, 0),
            IsolationTier::Wasm,
            0,
            Manifest::MAX_LATENCY_FRAMES + 1,
            Manifest::HOST_API_VERSION,
            trusted(),
        );
        assert_eq!(
            refused.err(),
            Some(ManifestError::LatencyTooHigh {
                declared: Manifest::MAX_LATENCY_FRAMES + 1,
                limit: Manifest::MAX_LATENCY_FRAMES,
            })
        );
    }

    #[test]
    fn a_manifest_for_a_host_interface_we_do_not_speak_is_refused_first() {
        // Checked before anything else, because a manifest written against a
        // different interface may mean something different by every other field.
        let refused = Manifest::read(
            PluginId::new(1),
            "not a valid identifier",
            "",
            Version::new(1, 0, 0),
            IsolationTier::Wasm,
            0,
            0,
            Manifest::HOST_API_VERSION + 1,
            trusted(),
        );
        assert_eq!(
            refused.err(),
            Some(ManifestError::UnsupportedApi {
                claimed: Manifest::HOST_API_VERSION + 1,
                supported: Manifest::HOST_API_VERSION,
            })
        );
    }

    #[test]
    fn identifiers_are_narrow_on_purpose() {
        // One that differs from another only by case or by an invisible
        // character is a defect that surfaces as "sometimes my plugin
        // disappears".
        for identifier in [
            "",
            "single",
            "com..example",
            ".com.example",
            "com.example.",
            "com.Example.reverb",
            "com.example.reverb ",
            "com.example.reverb\u{200b}",
        ] {
            assert!(
                !is_reverse_domain(identifier),
                "{identifier:?} should not be usable"
            );
        }
        for identifier in ["com.example", "com.example.reverb-2", "a.b0"] {
            assert!(
                is_reverse_domain(identifier),
                "{identifier:?} should be usable"
            );
        }
        assert!(!is_reverse_domain(
            &"a.".repeat(Manifest::MAX_IDENTIFIER_BYTES)
        ));
    }

    #[test]
    fn a_manifest_without_a_usable_name_is_refused() {
        // The name is what the user sees in a permission prompt. A blank one
        // turns "X wants to use the network" into a question nobody can answer.
        for name in ["", "   ", &"n".repeat(Manifest::MAX_NAME_BYTES + 1)] {
            let refused = Manifest::read(
                PluginId::new(1),
                "com.example.reverb",
                name,
                Version::new(1, 0, 0),
                IsolationTier::Wasm,
                0,
                0,
                Manifest::HOST_API_VERSION,
                trusted(),
            );
            assert_eq!(
                refused.err(),
                Some(ManifestError::NameNotUsable),
                "{name:?}"
            );
        }
    }

    #[test]
    fn a_manifest_keeps_what_it_was_told() {
        let plugin = manifest(IsolationTier::Wasm, 0, trusted());
        assert_eq!(plugin.id(), PluginId::new(1));
        assert_eq!(plugin.identifier(), "com.example.reverb");
        assert_eq!(plugin.name(), "Example Reverb");
        assert_eq!(plugin.version(), Version::new(1, 0, 0));
        assert_eq!(plugin.version().to_string(), "1.0.0");
        assert!(plugin.asks_for_nothing());
        assert_eq!(plugin.signature().key(), "signature.trusted");
        assert!(Version::new(1, 0, 1) > Version::new(1, 0, 0));
    }
}
