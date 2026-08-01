use core::fmt;
use core::num::NonZeroU32;

use crate::error::TimeError;

/// The lowest sample rate the engine accepts, in hertz.
const MIN_HZ: u32 = 8_000;

/// The highest sample rate the engine accepts, in hertz.
const MAX_HZ: u32 = 768_000;

/// A validated audio sample rate in hertz.
///
/// Wrapping the rate in a type rather than passing a bare integer removes an
/// entire class of defect: a sample rate of zero reaching a conversion would
/// divide by zero, and a rate silently confused with a frame count would produce
/// timing errors that are extremely hard to trace back to their origin.
///
/// The value is guaranteed non-zero and within the supported range, so every
/// conversion in this crate can rely on it without re-checking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SampleRate(NonZeroU32);

impl SampleRate {
    /// 44.1 kHz — compact disc rate, and the rate of most consumer music files.
    pub const HZ_44100: Self = Self::new_const(44_100);
    /// 48 kHz — the professional and broadcast standard, and the default for
    /// most audio interfaces.
    pub const HZ_48000: Self = Self::new_const(48_000);
    /// 88.2 kHz.
    pub const HZ_88200: Self = Self::new_const(88_200);
    /// 96 kHz.
    pub const HZ_96000: Self = Self::new_const(96_000);
    /// 176.4 kHz.
    pub const HZ_176400: Self = Self::new_const(176_400);
    /// 192 kHz — the highest rate named in Master Prompt #3A.
    pub const HZ_192000: Self = Self::new_const(192_000);

    /// Creates a sample rate, validating that it is supported.
    ///
    /// # Errors
    ///
    /// Returns [`TimeError::UnsupportedSampleRate`] if `hz` is zero or lies
    /// outside the supported range of 8 kHz to 768 kHz.
    pub const fn new(hz: u32) -> Result<Self, TimeError> {
        if hz < MIN_HZ || hz > MAX_HZ {
            return Err(TimeError::UnsupportedSampleRate { hz });
        }
        match NonZeroU32::new(hz) {
            Some(value) => Ok(Self(value)),
            // Unreachable: `hz >= MIN_HZ` already established it is non-zero.
            None => Err(TimeError::UnsupportedSampleRate { hz }),
        }
    }

    /// Builds one of the known-good constants at compile time.
    ///
    /// Only ever called with literals that are inside the valid range, so the
    /// fallback branch cannot be taken; it exists because `const fn` cannot
    /// panic under this crate's lint policy.
    const fn new_const(hz: u32) -> Self {
        match NonZeroU32::new(hz) {
            Some(value) => Self(value),
            // Unreachable for the literals used above. Falling back to a valid
            // rate keeps this function total without introducing a panic path.
            None => Self(NonZeroU32::MIN),
        }
    }

    /// Returns the rate in hertz.
    #[must_use]
    pub const fn hz(self) -> u32 {
        self.0.get()
    }

    /// Returns the rate in hertz as a 64-bit integer, for use in conversions.
    #[must_use]
    pub const fn hz_u64(self) -> u64 {
        self.0.get() as u64
    }
}

impl fmt::Display for SampleRate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} Hz", self.0.get())
    }
}

impl TryFrom<u32> for SampleRate {
    type Error = TimeError;

    fn try_from(hz: u32) -> Result<Self, Self::Error> {
        Self::new(hz)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_the_professional_rates() {
        for hz in [44_100, 48_000, 88_200, 96_000, 176_400, 192_000] {
            let rate = SampleRate::new(hz);
            assert!(rate.is_ok(), "{hz} Hz should be accepted");
            assert_eq!(rate.map(SampleRate::hz), Ok(hz));
        }
    }

    #[test]
    fn rejects_zero() {
        assert_eq!(
            SampleRate::new(0),
            Err(TimeError::UnsupportedSampleRate { hz: 0 })
        );
    }

    #[test]
    fn rejects_out_of_range() {
        assert!(SampleRate::new(MIN_HZ - 1).is_err());
        assert!(SampleRate::new(MAX_HZ + 1).is_err());
        assert!(SampleRate::new(MIN_HZ).is_ok());
        assert!(SampleRate::new(MAX_HZ).is_ok());
    }

    #[test]
    fn constants_match_their_names() {
        assert_eq!(SampleRate::HZ_44100.hz(), 44_100);
        assert_eq!(SampleRate::HZ_48000.hz(), 48_000);
        assert_eq!(SampleRate::HZ_88200.hz(), 88_200);
        assert_eq!(SampleRate::HZ_96000.hz(), 96_000);
        assert_eq!(SampleRate::HZ_176400.hz(), 176_400);
        assert_eq!(SampleRate::HZ_192000.hz(), 192_000);
    }
}
