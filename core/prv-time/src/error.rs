use core::fmt;

/// Errors produced when constructing musical time values from untrusted input.
///
/// Every variant names the offending value so that diagnostics can explain the
/// problem to the user rather than reporting a bare failure. Master Prompt #10
/// requires structured errors with a user-facing explanation; this type is the
/// domain half of that contract.
// `Eq` is deliberately not derived: one variant carries an `f64`, and floating
// point has no total equality. `PartialEq` is enough for the comparisons this
// type is used in, and claiming `Eq` here would be a lie about the value's
// semantics.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum TimeError {
    /// A sample rate of zero, or one outside the supported range.
    ///
    /// The supported range spans the rates named in Master Prompt #3A, from
    /// 8 kHz to 768 kHz, which comfortably covers 44.1, 48, 96 and 192 kHz plus
    /// headroom for future hardware.
    UnsupportedSampleRate {
        /// The rejected value, in hertz.
        hz: u32,
    },

    /// A tempo outside the range the engine will accept.
    ///
    /// The bounds are deliberately wide: half-time and double-time detection
    /// (Master Prompt #20) legitimately produces values far from the musical
    /// mainstream, and rejecting them here would discard useful analysis.
    TempoOutOfRange {
        /// The rejected value, in beats per minute.
        bpm: f64,
    },

    /// A tempo that is not a finite number.
    TempoNotFinite,

    /// A time signature numerator of zero.
    InvalidSignatureNumerator {
        /// The rejected numerator.
        numerator: u8,
    },

    /// A time signature denominator that is not a power of two between 1 and 32.
    ///
    /// Non-power-of-two denominators have no unambiguous note value, and the
    /// bound of 32 keeps every bar length an exact whole number of ticks.
    InvalidSignatureDenominator {
        /// The rejected denominator.
        denominator: u8,
    },

    /// A conversion whose result does not fit the target representation.
    ///
    /// In practice this requires positions far beyond any real session; it is
    /// surfaced rather than silently saturated because silent saturation in
    /// time arithmetic produces misalignment that is very hard to diagnose.
    ConversionOverflow,
}

impl fmt::Display for TimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedSampleRate { hz } => {
                write!(f, "unsupported sample rate: {hz} Hz")
            }
            Self::TempoOutOfRange { bpm } => {
                write!(f, "tempo out of range: {bpm} BPM")
            }
            Self::TempoNotFinite => f.write_str("tempo is not a finite number"),
            Self::InvalidSignatureNumerator { numerator } => {
                write!(f, "invalid time signature numerator: {numerator}")
            }
            Self::InvalidSignatureDenominator { denominator } => {
                write!(
                    f,
                    "invalid time signature denominator: {denominator} (must be a power of two, 1 to 32)"
                )
            }
            Self::ConversionOverflow => f.write_str("time conversion overflowed"),
        }
    }
}

impl core::error::Error for TimeError {}
