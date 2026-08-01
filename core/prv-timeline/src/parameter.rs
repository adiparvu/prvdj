//! Naming a parameter, so that something else can change it.
//!
//! # The gap this closes
//!
//! `prv-dsp` has had processors with parameters since Sprint 1 and no way to
//! *refer* to one. That was recorded as a deliberate omission rather than
//! improvised around, because three separate features need the same answer and
//! solving it for one of them would have produced something the other two could
//! not use:
//!
//! - **Automation** (Master Prompt #21) needs to say "the low equaliser band of
//!   deck A, over these four bars".
//! - **Plugins** (Master Prompt #23) need to expose their own parameters to the
//!   host without the host knowing what they are in advance.
//! - **The operation log** (ADR-0003) needs to record a parameter change in a
//!   form that still means the same thing when the project is reopened next
//!   year, on another machine, after the effect slots have been reordered.
//!
//! The third requirement is the demanding one, and it is what rules out the
//! obvious designs.
//!
//! # Why not a pointer, an index, or a string
//!
//! **A pointer or a reference** cannot be stored, cannot be sent to another
//! device, and cannot survive the object being rebuilt. It is only an address
//! within one process at one moment.
//!
//! **A numeric index into a flat list** is stable only until the list changes.
//! Insert an effect at slot 2 and every automation lane below it is now
//! pointing at the wrong thing — silently, because the indices are all still
//! valid. This is the failure mode that makes users stop trusting automation.
//!
//! **A free-form string path** survives storage but not review: nothing checks
//! that `"deckA/eq/lo"` matches what the equaliser actually calls its
//! parameter, so a rename becomes a silent data loss discovered months later.
//!
//! # What this is instead
//!
//! An address is a *typed owner* plus a *typed key*. The owner is an identity
//! that the project already guarantees to be stable — a placement identifier
//! from the operation log, a lane number, the master bus — and the key is an
//! enumerated parameter for a built-in processor, or an interned identifier
//! supplied by a plugin.
//!
//! Stability then follows from something already true rather than from a new
//! promise: a placement identifier is stable because the log says it is, and
//! reordering effects does not renumber them because the slot is part of the
//! owner rather than the whole of it.

use core::fmt;

use prv_project::PlacementId;

/// What owns a parameter.
///
/// Deliberately a tree rather than a flat namespace. An effect's parameters
/// belong to the effect, which belongs to a channel — and expressing that
/// nesting is what lets a whole subtree be moved, copied or removed with its
/// automation intact, because every address underneath it changes in one place.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ParameterOwner {
    /// The master bus.
    Master,

    /// A lane of the timeline.
    ///
    /// Lanes are numbered rather than identified, because a lane *is* its
    /// position: moving a lane means moving what is on it, and automation
    /// should follow the content rather than the row.
    Lane(u32),

    /// One placement of a track on the timeline.
    ///
    /// Identified by the log's own identifier, so the address is exactly as
    /// stable as the placement is — which ADR-0003 already guarantees.
    Placement(PlacementId),

    /// An effect in a slot on some owner.
    ///
    /// The slot is part of the address rather than the whole of it, so
    /// reordering the chain re-addresses only the effects that moved.
    Effect {
        /// What the effect sits on.
        host: Box<ParameterOwner>,
        /// Which slot in that owner's chain.
        slot: u8,
    },
}

impl ParameterOwner {
    /// Builds an address for an effect slot on this owner.
    #[must_use]
    pub fn effect(self, slot: u8) -> Self {
        Self::Effect {
            host: Box::new(self),
            slot,
        }
    }

    /// How deeply nested this owner is.
    ///
    /// Bounded in practice by [`ParameterOwner::MAX_DEPTH`], which is checked
    /// on construction of an [`ParameterAddress`] rather than assumed. An
    /// unbounded chain of effects hosting effects would make comparison and
    /// formatting unbounded work, and both are done on paths that must not
    /// stall.
    #[must_use]
    pub fn depth(&self) -> u8 {
        match self {
            Self::Master | Self::Lane(_) | Self::Placement(_) => 0,
            Self::Effect { host, .. } => host.depth().saturating_add(1),
        }
    }

    /// The deepest nesting an address may have.
    ///
    /// Four. An effect inside an effect inside an effect is a plugin hosting a
    /// plugin hosting a plugin, which Master Prompt #23 permits; four levels is
    /// past anything a user builds deliberately and short enough that the
    /// bound is meaningful.
    pub const MAX_DEPTH: u8 = 4;

    /// The owner this one sits on, if any.
    #[must_use]
    pub fn host(&self) -> Option<&Self> {
        match self {
            Self::Master | Self::Lane(_) | Self::Placement(_) => None,
            Self::Effect { host, .. } => Some(host),
        }
    }

    /// Whether this owner is `other`, or sits somewhere beneath it.
    ///
    /// What "remove this channel and everything on it" is implemented in terms
    /// of. Doing it by string prefix would match `lane/10` for `lane/1`, which
    /// is the class of bug that only appears once a user has eleven lanes.
    #[must_use]
    pub fn is_within(&self, other: &Self) -> bool {
        if self == other {
            return true;
        }
        match self {
            Self::Effect { host, .. } => host.is_within(other),
            _ => false,
        }
    }
}

impl fmt::Display for ParameterOwner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Master => f.write_str("master"),
            Self::Lane(index) => write!(f, "lane[{index}]"),
            Self::Placement(id) => write!(f, "placement[{}]", id.get()),
            Self::Effect { host, slot } => write!(f, "{host}/effect[{slot}]"),
        }
    }
}

/// Which parameter of an owner.
///
/// The built-in variants are enumerated because they are part of the engine's
/// own contract: an equaliser has three bands and always will, so naming them
/// in the type means a typo is a compile error rather than an automation lane
/// that silently controls nothing.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum ParameterKey {
    /// Output level.
    Gain,
    /// Low equaliser band.
    EqLow,
    /// Mid equaliser band.
    EqMid,
    /// High equaliser band.
    EqHigh,
    /// The single-knob filter sweep.
    Filter,
    /// Effect wet/dry balance.
    Mix,
    /// Crossfader position.
    Crossfader,

    /// A parameter belonging to a plugin.
    ///
    /// Carries the identifier the plugin declared. It is a string because the
    /// host cannot know a third-party plugin's parameters at compile time, and
    /// that is the one place where a string is the right answer rather than a
    /// shortcut: the plugin's manifest is the authority, the host merely
    /// records what it said.
    ///
    /// Master Prompt #23 requires a plugin to declare its parameters, so this
    /// value is validated against the manifest at load time; an address naming
    /// a parameter the plugin no longer declares is reported rather than
    /// silently dropped.
    Plugin(PluginParameterId),
}

impl ParameterKey {
    /// A stable identifier, for storage and localisation.
    ///
    /// Deliberately not display text. A core crate returning English prose
    /// would make the core the place translations live, which Master Prompt #8
    /// puts in the presentation layer.
    #[must_use]
    pub fn key(&self) -> &str {
        match self {
            Self::Gain => "parameter.gain",
            Self::EqLow => "parameter.eq_low",
            Self::EqMid => "parameter.eq_mid",
            Self::EqHigh => "parameter.eq_high",
            Self::Filter => "parameter.filter",
            Self::Mix => "parameter.mix",
            Self::Crossfader => "parameter.crossfader",
            Self::Plugin(id) => id.as_str(),
        }
    }

    /// Whether this parameter belongs to a plugin rather than the engine.
    #[must_use]
    pub const fn is_plugin(&self) -> bool {
        matches!(self, Self::Plugin(_))
    }
}

impl fmt::Display for ParameterKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.key())
    }
}

/// The longest a plugin parameter identifier may be.
///
/// Sixty-four bytes. Long enough for any readable identifier, short enough that
/// a malicious or broken manifest cannot make the host allocate without bound —
/// which Master Prompt #26 requires of anything read from a plugin.
pub const MAX_PLUGIN_PARAMETER_LENGTH: usize = 64;

/// An identifier a plugin declared for one of its parameters.
///
/// Validated on construction rather than on use. A value that reached storage
/// unvalidated would have to be re-checked everywhere it was read, and the one
/// place it was not would be the vulnerability.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PluginParameterId(String);

impl PluginParameterId {
    /// Creates an identifier from a plugin's declaration.
    ///
    /// # Errors
    ///
    /// Returns [`ParameterError`] for an empty identifier, one longer than
    /// [`MAX_PLUGIN_PARAMETER_LENGTH`], or one containing characters outside
    /// the permitted set. The permitted set is deliberately narrow —
    /// lowercase letters, digits, underscore and dot — because these values are
    /// used as storage keys and in log output, and a value that can contain a
    /// newline or a path separator is a value that can be used to forge either.
    pub fn new(value: &str) -> Result<Self, ParameterError> {
        if value.is_empty() {
            return Err(ParameterError::EmptyPluginParameter);
        }
        if value.len() > MAX_PLUGIN_PARAMETER_LENGTH {
            return Err(ParameterError::PluginParameterTooLong {
                length: value.len(),
                maximum: MAX_PLUGIN_PARAMETER_LENGTH,
            });
        }
        if let Some(offender) = value
            .chars()
            .find(|character| !matches!(character, 'a'..='z' | '0'..='9' | '_' | '.'))
        {
            return Err(ParameterError::InvalidPluginParameter {
                character: offender,
            });
        }
        Ok(Self(value.to_owned()))
    }

    /// The identifier.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A complete parameter address.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ParameterAddress {
    owner: ParameterOwner,
    key: ParameterKey,
}

impl ParameterAddress {
    /// Creates an address.
    ///
    /// # Errors
    ///
    /// Returns [`ParameterError::TooDeep`] when the owner nests more deeply
    /// than [`ParameterOwner::MAX_DEPTH`].
    pub fn new(owner: ParameterOwner, key: ParameterKey) -> Result<Self, ParameterError> {
        let depth = owner.depth();
        if depth > ParameterOwner::MAX_DEPTH {
            return Err(ParameterError::TooDeep {
                depth,
                maximum: ParameterOwner::MAX_DEPTH,
            });
        }
        Ok(Self { owner, key })
    }

    /// What owns the parameter.
    #[must_use]
    pub const fn owner(&self) -> &ParameterOwner {
        &self.owner
    }

    /// Which parameter.
    #[must_use]
    pub const fn key(&self) -> &ParameterKey {
        &self.key
    }

    /// Whether this address refers to something within `owner`.
    #[must_use]
    pub fn is_within(&self, owner: &ParameterOwner) -> bool {
        self.owner.is_within(owner)
    }
}

impl fmt::Display for ParameterAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.owner, self.key)
    }
}

/// What a parameter accepts, and how it should be presented.
///
/// # Why the descriptor is separate from the address
///
/// An address says *which* parameter; a descriptor says what values it takes.
/// Keeping them apart is what lets automation store a normalised value from
/// zero to one — which is stable when a parameter's range is later widened —
/// while an interface still shows the user decibels or hertz.
///
/// It is also what lets a plugin describe its own parameters at load time
/// without the host having compiled anything about them.
#[derive(Debug, Clone, PartialEq)]
pub struct ParameterDescriptor {
    minimum: f32,
    maximum: f32,
    default: f32,
    unit: ParameterUnit,
    curve: ParameterCurve,
}

impl ParameterDescriptor {
    /// Describes a parameter.
    ///
    /// # Errors
    ///
    /// Returns [`ParameterError::InvalidRange`] when the bounds are not
    /// finite or the minimum is not below the maximum, and clamps a default
    /// outside the range rather than rejecting it — a default is a suggestion,
    /// and refusing to load a plugin over one would be disproportionate.
    pub fn new(
        minimum: f32,
        maximum: f32,
        default: f32,
        unit: ParameterUnit,
        curve: ParameterCurve,
    ) -> Result<Self, ParameterError> {
        if !minimum.is_finite() || !maximum.is_finite() || minimum >= maximum {
            return Err(ParameterError::InvalidRange { minimum, maximum });
        }
        let default = if default.is_finite() {
            default.clamp(minimum, maximum)
        } else {
            minimum
        };
        Ok(Self {
            minimum,
            maximum,
            default,
            unit,
            curve,
        })
    }

    /// The lowest permitted value.
    #[must_use]
    pub const fn minimum(&self) -> f32 {
        self.minimum
    }

    /// The highest permitted value.
    #[must_use]
    pub const fn maximum(&self) -> f32 {
        self.maximum
    }

    /// The value the parameter takes before anything sets it.
    #[must_use]
    pub const fn default(&self) -> f32 {
        self.default
    }

    /// What the value means.
    #[must_use]
    pub const fn unit(&self) -> ParameterUnit {
        self.unit
    }

    /// How a normalised position maps onto the range.
    #[must_use]
    pub const fn curve(&self) -> ParameterCurve {
        self.curve
    }

    /// Converts a real value into the normalised zero-to-one form automation
    /// stores.
    #[must_use]
    pub fn normalise(&self, value: f32) -> f32 {
        let span = f64::from(self.maximum) - f64::from(self.minimum);
        if span <= 0.0 {
            return 0.0;
        }
        let clamped = if value.is_finite() {
            value.clamp(self.minimum, self.maximum)
        } else {
            self.minimum
        };
        let linear = (f64::from(clamped) - f64::from(self.minimum)) / span;
        crate::num::narrow(self.curve.to_normalised(linear))
    }

    /// Converts a normalised position back into a real value.
    #[must_use]
    pub fn denormalise(&self, normalised: f32) -> f32 {
        let position = if normalised.is_finite() {
            f64::from(normalised.clamp(0.0, 1.0))
        } else {
            0.0
        };
        let linear = self.curve.to_linear(position);
        let span = f64::from(self.maximum) - f64::from(self.minimum);
        crate::num::narrow(f64::from(self.minimum) + linear * span)
    }
}

/// What a parameter's value means.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ParameterUnit {
    /// A bare number.
    Plain,
    /// Decibels.
    Decibels,
    /// Hertz.
    Hertz,
    /// A percentage from zero to one hundred.
    Percent,
    /// Seconds.
    Seconds,
    /// Beats.
    Beats,
    /// A switch.
    Toggle,
}

/// How a normalised position maps onto a parameter's range.
///
/// The mapping belongs to the parameter rather than to the control that draws
/// it, because automation stores normalised values: a curve stored in the
/// interface would mean the same automation lane produced different sound
/// depending on which control last wrote it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ParameterCurve {
    /// Position maps directly onto value.
    Linear,

    /// Position maps onto the logarithm of the value.
    ///
    /// What frequency controls need. A filter swept linearly from 20 Hz to
    /// 20 kHz spends nine tenths of its travel above 2 kHz, where almost no
    /// musical decisions are made; a logarithmic sweep gives each octave the
    /// same distance, which is how the ear hears it.
    Logarithmic,

    /// Position maps onto the square of the value.
    ///
    /// What level controls need: it puts finer resolution near silence, where
    /// a decibel matters most.
    Squared,
}

impl ParameterCurve {
    /// Maps a linear fraction of the range onto a normalised position.
    fn to_normalised(self, linear: f64) -> f64 {
        let clamped = linear.clamp(0.0, 1.0);
        match self {
            Self::Linear => clamped,
            // A decade of headroom below the top, which spans the useful range
            // of both a filter sweep and a fader without ever taking the
            // logarithm of zero.
            Self::Logarithmic => {
                let floor = 1.0_f64 / 1_000.0;
                let value = floor + clamped * (1.0 - floor);
                (value / floor).log10() / (1.0 / floor).log10()
            }
            Self::Squared => clamped.sqrt(),
        }
    }

    /// Maps a normalised position back onto a linear fraction of the range.
    ///
    /// Named `to_linear` rather than `from_normalised` because it takes `self`:
    /// a `from_` method that consumes a receiver reads as a constructor and is
    /// not one.
    fn to_linear(self, position: f64) -> f64 {
        let clamped = position.clamp(0.0, 1.0);
        match self {
            Self::Linear => clamped,
            Self::Logarithmic => {
                let floor = 1.0_f64 / 1_000.0;
                let value = floor * (1.0 / floor).powf(clamped);
                ((value - floor) / (1.0 - floor)).clamp(0.0, 1.0)
            }
            Self::Squared => clamped * clamped,
        }
    }
}

/// Errors from building a parameter address or descriptor.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ParameterError {
    /// An owner nested more deeply than the engine supports.
    TooDeep {
        /// The depth supplied.
        depth: u8,
        /// The deepest permitted.
        maximum: u8,
    },
    /// A plugin declared a parameter with an empty identifier.
    EmptyPluginParameter,
    /// A plugin declared a parameter identifier that is too long.
    PluginParameterTooLong {
        /// The length supplied.
        length: usize,
        /// The longest permitted.
        maximum: usize,
    },
    /// A plugin declared a parameter identifier containing a forbidden
    /// character.
    InvalidPluginParameter {
        /// The offending character.
        character: char,
    },
    /// A descriptor whose bounds are unusable.
    InvalidRange {
        /// The minimum supplied.
        minimum: f32,
        /// The maximum supplied.
        maximum: f32,
    },
}

impl fmt::Display for ParameterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooDeep { depth, maximum } => {
                write!(
                    f,
                    "parameter owner nests {depth} deep, the maximum is {maximum}"
                )
            }
            Self::EmptyPluginParameter => f.write_str("plugin parameter identifier is empty"),
            Self::PluginParameterTooLong { length, maximum } => write!(
                f,
                "plugin parameter identifier is {length} bytes, the maximum is {maximum}"
            ),
            Self::InvalidPluginParameter { character } => write!(
                f,
                "plugin parameter identifier contains the forbidden character {character:?}"
            ),
            Self::InvalidRange { minimum, maximum } => {
                write!(f, "parameter range {minimum} to {maximum} is unusable")
            }
        }
    }
}

impl core::error::Error for ParameterError {}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::panic,
        clippy::float_cmp,
        reason = "a test that cannot build its own fixture should fail loudly"
    )]

    use super::*;

    #[test]
    fn an_address_survives_being_written_down_and_read_back() {
        // The requirement that rules out pointers and indices: the same address
        // must mean the same thing after the project is closed and reopened.
        // Equality and ordering are what storage and lookup are built on.
        let first = ParameterAddress::new(
            ParameterOwner::Placement(PlacementId::new(7)).effect(2),
            ParameterKey::Filter,
        )
        .expect("valid");
        let rebuilt = ParameterAddress::new(
            ParameterOwner::Placement(PlacementId::new(7)).effect(2),
            ParameterKey::Filter,
        )
        .expect("valid");

        assert_eq!(first, rebuilt);
        assert_eq!(first.to_string(), "placement[7]/effect[2]/parameter.filter");
    }

    #[test]
    fn reordering_effects_re_addresses_only_what_moved() {
        // The failure a flat index has: inserting an effect renumbers
        // everything below it, and every automation lane silently points at
        // the wrong parameter. Here the slot is part of the owner, so an
        // address at slot 0 is unaffected by anything happening at slot 3.
        let lane = ParameterOwner::Lane(1);
        let first =
            ParameterAddress::new(lane.clone().effect(0), ParameterKey::Gain).expect("valid");
        let third =
            ParameterAddress::new(lane.clone().effect(3), ParameterKey::Gain).expect("valid");

        assert_ne!(first, third);
        assert!(first.is_within(&lane));
        assert!(third.is_within(&lane));
    }

    #[test]
    fn containment_is_structural_rather_than_textual() {
        // A string-prefix implementation matches `lane[10]` for `lane[1]`,
        // which is the class of bug that only appears once a user has eleven
        // lanes and then removes the first one.
        let one = ParameterOwner::Lane(1);
        let ten = ParameterOwner::Lane(10);
        assert!(!ten.is_within(&one));
        assert!(one.is_within(&one));

        let nested = one.clone().effect(0).effect(1);
        assert!(nested.is_within(&one));
        assert!(!nested.is_within(&ten));
        assert!(!one.is_within(&nested));
    }

    #[test]
    fn nesting_beyond_the_limit_is_refused_rather_than_accepted_unbounded() {
        let mut owner = ParameterOwner::Master;
        for slot in 0..=ParameterOwner::MAX_DEPTH {
            owner = owner.effect(slot);
        }
        assert_eq!(owner.depth(), ParameterOwner::MAX_DEPTH + 1);
        assert!(matches!(
            ParameterAddress::new(owner, ParameterKey::Gain),
            Err(ParameterError::TooDeep { .. })
        ));
    }

    #[test]
    fn a_plugin_identifier_is_validated_before_it_is_stored() {
        // These values reach storage and log output. A value that can contain a
        // newline or a path separator is a value that can be used to forge
        // either, so the check happens once, at the boundary.
        assert!(PluginParameterId::new("cutoff").is_ok());
        assert!(PluginParameterId::new("reverb.decay_time").is_ok());

        assert_eq!(
            PluginParameterId::new("").err(),
            Some(ParameterError::EmptyPluginParameter)
        );
        assert!(matches!(
            PluginParameterId::new("Cut Off"),
            Err(ParameterError::InvalidPluginParameter { .. })
        ));
        assert!(matches!(
            PluginParameterId::new("../../etc/passwd"),
            Err(ParameterError::InvalidPluginParameter { .. })
        ));
        assert!(matches!(
            PluginParameterId::new("a\nb"),
            Err(ParameterError::InvalidPluginParameter { .. })
        ));
        assert!(matches!(
            PluginParameterId::new(&"x".repeat(MAX_PLUGIN_PARAMETER_LENGTH + 1)),
            Err(ParameterError::PluginParameterTooLong { .. })
        ));
    }

    #[test]
    fn normalising_and_denormalising_round_trip_on_every_curve() {
        // The property automation depends on. A lane stores normalised values;
        // if the round trip were lossy, every automation curve would drift a
        // little each time it was edited.
        for curve in [
            ParameterCurve::Linear,
            ParameterCurve::Logarithmic,
            ParameterCurve::Squared,
        ] {
            let descriptor =
                ParameterDescriptor::new(20.0, 20_000.0, 1_000.0, ParameterUnit::Hertz, curve)
                    .expect("valid");
            let mut step = 0;
            while step <= 100 {
                let position = crate::num::narrow(f64::from(step) / 100.0);
                let value = descriptor.denormalise(position);
                let back = descriptor.normalise(value);
                assert!(
                    (f64::from(back) - f64::from(position)).abs() < 1e-3,
                    "{curve:?} at {position} became {value} and returned {back}"
                );
                step += 1;
            }
        }
    }

    #[test]
    fn a_logarithmic_sweep_gives_each_octave_the_same_travel() {
        // The reason the curve belongs to the parameter. A filter swept
        // linearly from 20 Hz to 20 kHz spends nine tenths of its travel above
        // 2 kHz, where almost no musical decisions are made.
        let descriptor = ParameterDescriptor::new(
            20.0,
            20_000.0,
            1_000.0,
            ParameterUnit::Hertz,
            ParameterCurve::Logarithmic,
        )
        .expect("valid");

        let quarter = descriptor.denormalise(0.25);
        let half = descriptor.denormalise(0.5);
        let three_quarters = descriptor.denormalise(0.75);

        // Each quarter of the travel multiplies the frequency by a similar
        // factor, rather than adding a similar number of hertz.
        let first = f64::from(half) / f64::from(quarter);
        let second = f64::from(three_quarters) / f64::from(half);
        assert!(
            (first / second - 1.0).abs() < 0.35,
            "the ratios {first} and {second} are not comparable, so the sweep is not logarithmic"
        );

        let linear = ParameterDescriptor::new(
            20.0,
            20_000.0,
            1_000.0,
            ParameterUnit::Hertz,
            ParameterCurve::Linear,
        )
        .expect("valid");
        assert!(
            descriptor.denormalise(0.5) < linear.denormalise(0.5) / 4.0,
            "the logarithmic midpoint should sit far below the linear one"
        );
    }

    #[test]
    fn out_of_range_and_non_numeric_values_are_contained() {
        let descriptor = ParameterDescriptor::new(
            -24.0,
            6.0,
            0.0,
            ParameterUnit::Decibels,
            ParameterCurve::Linear,
        )
        .expect("valid");
        assert_eq!(descriptor.normalise(-100.0), 0.0);
        assert_eq!(descriptor.normalise(100.0), 1.0);
        assert_eq!(descriptor.normalise(f32::NAN), 0.0);
        assert_eq!(descriptor.denormalise(-1.0), -24.0);
        assert_eq!(descriptor.denormalise(2.0), 6.0);
        assert_eq!(descriptor.denormalise(f32::NAN), -24.0);
    }

    #[test]
    fn an_unusable_range_is_refused_and_a_stray_default_is_corrected() {
        // A range is a contract and an unusable one is a defect; a default is a
        // suggestion, and refusing to load a plugin over one would be
        // disproportionate.
        assert!(matches!(
            ParameterDescriptor::new(1.0, 1.0, 1.0, ParameterUnit::Plain, ParameterCurve::Linear),
            Err(ParameterError::InvalidRange { .. })
        ));
        assert!(matches!(
            ParameterDescriptor::new(
                f32::NAN,
                1.0,
                0.0,
                ParameterUnit::Plain,
                ParameterCurve::Linear
            ),
            Err(ParameterError::InvalidRange { .. })
        ));

        let corrected =
            ParameterDescriptor::new(0.0, 1.0, 9.0, ParameterUnit::Plain, ParameterCurve::Linear)
                .expect("valid range");
        assert_eq!(corrected.default(), 1.0);
    }

    #[test]
    fn built_in_parameter_keys_are_distinct() {
        // A shared key would make two different parameters the same address,
        // and automation on one would move the other.
        let keys = [
            ParameterKey::Gain,
            ParameterKey::EqLow,
            ParameterKey::EqMid,
            ParameterKey::EqHigh,
            ParameterKey::Filter,
            ParameterKey::Mix,
            ParameterKey::Crossfader,
        ];
        for (index, key) in keys.iter().enumerate() {
            assert!(!key.is_plugin());
            for (other, value) in keys.iter().enumerate() {
                assert!(
                    index == other || key.key() != value.key(),
                    "two parameters share the key {}",
                    key.key()
                );
            }
        }

        let plugin = ParameterKey::Plugin(PluginParameterId::new("cutoff").expect("valid"));
        assert!(plugin.is_plugin());
        assert_eq!(plugin.key(), "cutoff");
    }
}
