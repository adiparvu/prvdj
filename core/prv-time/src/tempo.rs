use core::fmt;
use core::num::NonZeroU64;

use crate::error::TimeError;

/// Microseconds in one minute, used to convert between the internal
/// representation and beats per minute.
const MICROS_PER_MINUTE: f64 = 60_000_000.0;

/// The slowest tempo the engine accepts, in beats per minute.
const MIN_BPM: f64 = 20.0;

/// The fastest tempo the engine accepts, in beats per minute.
///
/// Deliberately far above any musical norm. Master Prompt #20 requires half-time
/// and double-time detection, and rejecting the doubled value of an already fast
/// track would discard a legitimate analysis result.
const MAX_BPM: f64 = 999.0;

/// A musical tempo, stored exactly as microseconds per beat.
///
/// # Why not beats per minute
///
/// Beats per minute is how musicians speak, but it is a poor internal
/// representation. Most musically ordinary tempi are not exactly representable
/// in binary floating point, so every conversion through beats per minute
/// introduces a small error. Repeated across a long session those errors
/// accumulate into audible misalignment.
///
/// Microseconds per beat is an integer. Every conversion between tempo, frames
/// and musical position therefore becomes exact integer arithmetic, and the
/// result is bit-identical on every platform — which is what ADR-0006 requires
/// of a planner whose output must be reproducible.
///
/// Beats per minute remains available at the edges, where a human reads or
/// types it.
///
/// A beat is a quarter note. Time signatures with other denominators are handled
/// by [`TimeSignature`](crate::TimeSignature), which converts bar lengths into
/// quarter-note terms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Tempo(NonZeroU64);

impl Tempo {
    /// 120 beats per minute — 500 000 microseconds per beat, exactly.
    pub const BPM_120: Self = Self::from_micros_per_beat_const(500_000);
    /// 128 beats per minute — the centre of gravity of house and techno.
    pub const BPM_128: Self = Self::from_micros_per_beat_const(468_750);
    /// 174 beats per minute — drum and bass.
    pub const BPM_174: Self = Self::from_micros_per_beat_const(344_827);

    /// Creates a tempo from beats per minute.
    ///
    /// The value is converted to microseconds per beat and rounded to the
    /// nearest microsecond, which is a resolution of better than one part in
    /// four hundred thousand at typical tempi — far finer than any audible
    /// difference, and exact from that point onward.
    ///
    /// # Errors
    ///
    /// Returns [`TimeError::TempoNotFinite`] if `bpm` is not finite, or
    /// [`TimeError::TempoOutOfRange`] if it lies outside 20 to 999 beats per
    /// minute.
    pub fn from_bpm(bpm: f64) -> Result<Self, TimeError> {
        if !bpm.is_finite() {
            return Err(TimeError::TempoNotFinite);
        }
        if !(MIN_BPM..=MAX_BPM).contains(&bpm) {
            return Err(TimeError::TempoOutOfRange { bpm });
        }
        let micros = (MICROS_PER_MINUTE / bpm).round();
        // `bpm` is bounded above by MAX_BPM and below by MIN_BPM, so `micros`
        // lies between roughly 60 060 and 3 000 000: comfortably inside u64.
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "range established by the bounds check above"
        )]
        let micros = micros as u64;
        NonZeroU64::new(micros)
            .map(Self)
            .ok_or(TimeError::TempoOutOfRange { bpm })
    }

    /// Creates a tempo directly from microseconds per beat.
    ///
    /// # Errors
    ///
    /// Returns [`TimeError::TempoOutOfRange`] if the value is zero or implies a
    /// tempo outside the supported range.
    pub fn from_micros_per_beat(micros: u64) -> Result<Self, TimeError> {
        let Some(micros) = NonZeroU64::new(micros) else {
            return Err(TimeError::TempoOutOfRange { bpm: f64::INFINITY });
        };
        let candidate = Self(micros);
        let bpm = candidate.bpm();
        if !(MIN_BPM..=MAX_BPM).contains(&bpm) {
            return Err(TimeError::TempoOutOfRange { bpm });
        }
        Ok(candidate)
    }

    /// Builds a compile-time constant from a known-good value.
    const fn from_micros_per_beat_const(micros: u64) -> Self {
        match NonZeroU64::new(micros) {
            Some(value) => Self(value),
            // Unreachable for the literals used above; avoids a panic path in a
            // `const fn`.
            None => Self(NonZeroU64::MIN),
        }
    }

    /// Returns the exact internal representation: microseconds per beat.
    #[must_use]
    pub const fn micros_per_beat(self) -> u64 {
        self.0.get()
    }

    /// Returns the tempo in beats per minute.
    ///
    /// Lossy, for display and for interfaces that speak in beats per minute.
    /// Internal arithmetic uses [`Self::micros_per_beat`].
    #[must_use]
    #[allow(
        clippy::cast_precision_loss,
        reason = "display-only conversion; internal arithmetic uses micros_per_beat"
    )]
    pub fn bpm(self) -> f64 {
        MICROS_PER_MINUTE / self.0.get() as f64
    }

    /// Returns this tempo halved, as produced by half-time detection.
    ///
    /// # Errors
    ///
    /// Returns [`TimeError::TempoOutOfRange`] if the halved tempo falls below
    /// the supported minimum.
    pub fn halved(self) -> Result<Self, TimeError> {
        Self::from_micros_per_beat(self.0.get().saturating_mul(2))
    }

    /// Returns this tempo doubled, as produced by double-time detection.
    ///
    /// # Errors
    ///
    /// Returns [`TimeError::TempoOutOfRange`] if the doubled tempo exceeds the
    /// supported maximum.
    pub fn doubled(self) -> Result<Self, TimeError> {
        // Integer division by a literal two: exact for even values, and the
        // half-microsecond lost for odd values is far below audible resolution.
        #[allow(
            clippy::integer_division,
            reason = "halving microseconds per beat; sub-microsecond remainder is inaudible"
        )]
        let micros = self.0.get() / 2;
        Self::from_micros_per_beat(micros)
    }
}

impl fmt::Display for Tempo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:.2} BPM", self.bpm())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_hundred_twenty_bpm_is_exactly_half_a_second_per_beat() {
        let tempo = Tempo::from_bpm(120.0);
        assert_eq!(tempo.map(Tempo::micros_per_beat), Ok(500_000));
        assert_eq!(Tempo::BPM_120.micros_per_beat(), 500_000);
    }

    #[test]
    fn bpm_round_trips_within_display_resolution() {
        for bpm in [
            20.0_f64, 60.0, 90.0, 120.0, 124.0, 128.0, 140.0, 174.0, 200.0, 999.0,
        ] {
            let tempo = Tempo::from_bpm(bpm);
            assert!(tempo.is_ok(), "{bpm} BPM should be accepted");
            if let Ok(tempo) = tempo {
                assert!(
                    (tempo.bpm() - bpm).abs() < 0.001,
                    "{bpm} BPM round-tripped to {}",
                    tempo.bpm()
                );
            }
        }
    }

    #[test]
    fn rejects_non_finite_and_out_of_range() {
        assert_eq!(Tempo::from_bpm(f64::NAN), Err(TimeError::TempoNotFinite));
        assert_eq!(
            Tempo::from_bpm(f64::INFINITY),
            Err(TimeError::TempoNotFinite)
        );
        assert!(matches!(
            Tempo::from_bpm(0.0),
            Err(TimeError::TempoOutOfRange { .. })
        ));
        assert!(matches!(
            Tempo::from_bpm(-120.0),
            Err(TimeError::TempoOutOfRange { .. })
        ));
        assert!(matches!(
            Tempo::from_bpm(1000.0),
            Err(TimeError::TempoOutOfRange { .. })
        ));
    }

    #[test]
    fn zero_micros_per_beat_is_rejected() {
        assert!(matches!(
            Tempo::from_micros_per_beat(0),
            Err(TimeError::TempoOutOfRange { .. })
        ));
    }

    #[test]
    fn half_and_double_time_are_exact_for_even_values() {
        let tempo = Tempo::BPM_120;
        let doubled = tempo.doubled();
        let halved = tempo.halved();
        assert_eq!(doubled.map(Tempo::micros_per_beat), Ok(250_000));
        assert_eq!(halved.map(Tempo::micros_per_beat), Ok(1_000_000));
    }

    #[test]
    fn doubling_past_the_maximum_is_rejected() {
        let fast = Tempo::from_bpm(900.0);
        assert!(fast.is_ok());
        if let Ok(fast) = fast {
            assert!(matches!(
                fast.doubled(),
                Err(TimeError::TempoOutOfRange { .. })
            ));
        }
    }

    #[test]
    fn halving_past_the_minimum_is_rejected() {
        let slow = Tempo::from_bpm(25.0);
        assert!(slow.is_ok());
        if let Ok(slow) = slow {
            assert!(matches!(
                slow.halved(),
                Err(TimeError::TempoOutOfRange { .. })
            ));
        }
    }
}
