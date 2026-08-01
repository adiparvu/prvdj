use core::f64::consts::TAU;

use prv_time::SampleRate;

/// State below this magnitude is flushed to zero.
///
/// Filter state decaying toward silence passes through the denormal range, where
/// arithmetic can slow by an order of magnitude on some processors. On the audio
/// thread that is a dropout, caused by nothing more than a track fading out.
/// Flushing ends the decay at zero instead.
const DENORMAL_FLOOR: f64 = 1e-30;

/// The quality factor of a second-order Butterworth section.
///
/// Two of these in cascade make a fourth-order Linkwitz-Riley, which is the
/// crossover the equaliser is built from.
pub(crate) const BUTTERWORTH_Q: f64 = core::f64::consts::FRAC_1_SQRT_2;

/// Second-order filter coefficients, already normalised by `a0`.
///
/// Normalising once at design time removes a division from the inner loop.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BiquadCoefficients {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
}

impl BiquadCoefficients {
    /// Coefficients that pass the signal through unchanged.
    pub const PASSTHROUGH: Self = Self {
        b0: 1.0,
        b1: 0.0,
        b2: 0.0,
        a1: 0.0,
        a2: 0.0,
    };

    /// Designs a low-pass section.
    ///
    /// Uses the bilinear transform with frequency pre-warping, so the corner
    /// lands where it was asked for rather than where the transform moved it.
    #[must_use]
    pub fn low_pass(cutoff_hz: f64, q: f64, rate: SampleRate) -> Self {
        let (sin_w0, cos_w0, alpha) = Self::intermediates(cutoff_hz, q, rate);
        let _ = sin_w0;
        let a0 = 1.0 + alpha;
        Self::normalise(
            (1.0 - cos_w0) * 0.5,
            1.0 - cos_w0,
            (1.0 - cos_w0) * 0.5,
            a0,
            -2.0 * cos_w0,
            1.0 - alpha,
        )
    }

    /// Designs a high-pass section.
    #[must_use]
    pub fn high_pass(cutoff_hz: f64, q: f64, rate: SampleRate) -> Self {
        let (_, cos_w0, alpha) = Self::intermediates(cutoff_hz, q, rate);
        let a0 = 1.0 + alpha;
        Self::normalise(
            (1.0 + cos_w0) * 0.5,
            -(1.0 + cos_w0),
            (1.0 + cos_w0) * 0.5,
            a0,
            -2.0 * cos_w0,
            1.0 - alpha,
        )
    }

    /// Designs an all-pass section.
    ///
    /// Passes every frequency at unit magnitude while shifting phase. Used to
    /// phase-align the low band of the equaliser with the two bands that went
    /// through the second crossover, so that the three sum back to a flat
    /// magnitude response. Without it the equaliser would have a dip at the
    /// upper crossover with every band at unity — audible as a hollow midrange
    /// on a channel nobody has touched.
    #[must_use]
    pub fn all_pass(centre_hz: f64, q: f64, rate: SampleRate) -> Self {
        let (_, cos_w0, alpha) = Self::intermediates(centre_hz, q, rate);
        let a0 = 1.0 + alpha;
        Self::normalise(
            1.0 - alpha,
            -2.0 * cos_w0,
            1.0 + alpha,
            a0,
            -2.0 * cos_w0,
            1.0 - alpha,
        )
    }

    /// Shared trigonometry, with the cutoff clamped inside the representable band.
    ///
    /// A cutoff at or above Nyquist has no meaning and would produce a filter
    /// that is unstable or silent; clamping keeps the design total. The lower
    /// bound keeps a nonsensical or corrupted value from producing a pole
    /// exactly on the unit circle.
    fn intermediates(frequency_hz: f64, q: f64, rate: SampleRate) -> (f64, f64, f64) {
        let nyquist = f64::from(rate.hz()) * 0.5;
        let frequency = frequency_hz.clamp(1.0, nyquist * 0.999);
        let q = if q.is_finite() && q > 0.01 { q } else { 0.5 };
        let w0 = TAU * frequency / f64::from(rate.hz());
        let sin_w0 = w0.sin();
        let cos_w0 = w0.cos();
        let alpha = sin_w0 / (2.0 * q);
        (sin_w0, cos_w0, alpha)
    }

    /// Divides through by `a0`.
    fn normalise(b0: f64, b1: f64, b2: f64, a0: f64, a1: f64, a2: f64) -> Self {
        if a0.abs() < f64::EPSILON {
            return Self::PASSTHROUGH;
        }
        let inverse = 1.0 / a0;
        Self {
            b0: b0 * inverse,
            b1: b1 * inverse,
            b2: b2 * inverse,
            a1: a1 * inverse,
            a2: a2 * inverse,
        }
    }
}

/// A second-order filter section.
///
/// Transposed direct form II: fewer state variables than direct form I and
/// better numerical behaviour than direct form II, which is why it is the
/// standard choice for fixed-coefficient audio filtering.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Biquad {
    coefficients: BiquadCoefficients,
    s1: f64,
    s2: f64,
}

impl Biquad {
    /// Creates a section that passes the signal through unchanged.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            coefficients: BiquadCoefficients::PASSTHROUGH,
            s1: 0.0,
            s2: 0.0,
        }
    }

    /// Replaces the coefficients, leaving the state intact.
    ///
    /// Keeping the state is deliberate: clearing it on every coefficient change
    /// would produce a click each time a filter knob moved. Coefficients are
    /// changed at block boundaries so that the discontinuity is bounded.
    pub fn set_coefficients(&mut self, coefficients: BiquadCoefficients) {
        self.coefficients = coefficients;
    }

    /// The current coefficients.
    #[must_use]
    pub const fn coefficients(&self) -> BiquadCoefficients {
        self.coefficients
    }

    /// Clears the state.
    pub fn reset(&mut self) {
        self.s1 = 0.0;
        self.s2 = 0.0;
    }

    /// Processes one sample.
    ///
    /// Allocation-free, branch-light and panic-free.
    #[must_use]
    pub fn process(&mut self, input: f64) -> f64 {
        let coefficients = self.coefficients;
        let output = coefficients.b0 * input + self.s1;
        self.s1 = coefficients.b1 * input - coefficients.a1 * output + self.s2;
        self.s2 = coefficients.b2 * input - coefficients.a2 * output;

        if self.s1.abs() < DENORMAL_FLOOR {
            self.s1 = 0.0;
        }
        if self.s2.abs() < DENORMAL_FLOOR {
            self.s2 = 0.0;
        }

        output
    }
}

impl Default for Biquad {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    // Measurement helpers convert between sample indices and floating-point
    // phase throughout. The values involved are block offsets and frequencies,
    // orders of magnitude below any precision limit, and a conversion error
    // here would show up immediately as a failed magnitude assertion.
    #![allow(
        clippy::cast_possible_truncation,
        clippy::cast_precision_loss,
        clippy::cast_sign_loss,
        reason = "test measurement helpers; magnitudes are asserted, so a bad conversion fails loudly"
    )]
    #![allow(
        clippy::float_cmp,
        reason = "state that has been flushed must be exactly zero, not nearly"
    )]

    use super::*;

    /// Measures the magnitude response at one frequency by driving the filter
    /// with a sine and comparing output energy to input energy.
    ///
    /// Measured rather than derived from the transfer function on purpose: this
    /// exercises the code that will actually run, including its state handling
    /// and its denormal flushing, rather than re-deriving the mathematics the
    /// implementation was written from.
    ///
    /// Energy rather than peak amplitude, because a sampled sine near Nyquist
    /// rarely has a sample at its crest — at 16 kHz and a 48 kHz rate there are
    /// three samples per cycle, and the largest of them is 0.866 however
    /// undistorted the signal is. A peak measurement would report that as 1.4 dB
    /// of attenuation that does not exist. Root-mean-square is independent of
    /// where the samples land.
    fn magnitude_at(filter: &mut Biquad, frequency_hz: f64, rate: SampleRate) -> f64 {
        let rate_hz = f64::from(rate.hz());
        let settle = (rate_hz * 0.5) as usize;
        let measure = (rate_hz * 0.5) as usize;
        let step = TAU * frequency_hz / rate_hz;

        for index in 0..settle {
            let _ = filter.process((index as f64 * step).sin());
        }

        let mut output_energy = 0.0_f64;
        let mut input_energy = 0.0_f64;
        for index in 0..measure {
            let input = ((settle + index) as f64 * step).sin();
            let output = filter.process(input);
            input_energy += input * input;
            output_energy += output * output;
        }
        // The ratio is taken against the energy actually presented, not against
        // the theoretical value for a sine. A measurement window rarely spans a
        // whole number of cycles, and at 50 Hz the difference is over one per
        // cent — enough to fail a flatness assertion that is actually correct.
        if input_energy <= 0.0 {
            return 0.0;
        }
        (output_energy / input_energy).sqrt()
    }

    #[test]
    fn passthrough_coefficients_leave_the_signal_alone() {
        let mut filter = Biquad::new();
        for value in [0.0, 0.5, -0.25, 1.0] {
            assert!((filter.process(value) - value).abs() < 1e-12);
        }
    }

    #[test]
    fn a_low_pass_passes_below_and_stops_above() {
        let rate = SampleRate::HZ_48000;
        let mut filter = Biquad::new();
        filter.set_coefficients(BiquadCoefficients::low_pass(1_000.0, BUTTERWORTH_Q, rate));

        let low = magnitude_at(&mut filter, 100.0, rate);
        filter.reset();
        let corner = magnitude_at(&mut filter, 1_000.0, rate);
        filter.reset();
        let high = magnitude_at(&mut filter, 10_000.0, rate);

        assert!(low > 0.98, "the passband should be flat, got {low}");
        // A Butterworth corner is 3 dB down: a magnitude of about 0.707.
        assert!(
            (corner - 0.707).abs() < 0.02,
            "the corner should be 3 dB down, got {corner}"
        );
        assert!(high < 0.02, "the stopband should be rejected, got {high}");
    }

    #[test]
    fn a_high_pass_is_the_mirror_of_a_low_pass() {
        let rate = SampleRate::HZ_48000;
        let mut filter = Biquad::new();
        filter.set_coefficients(BiquadCoefficients::high_pass(1_000.0, BUTTERWORTH_Q, rate));

        let low = magnitude_at(&mut filter, 100.0, rate);
        filter.reset();
        let corner = magnitude_at(&mut filter, 1_000.0, rate);
        filter.reset();
        let high = magnitude_at(&mut filter, 10_000.0, rate);

        assert!(low < 0.02, "the stopband should be rejected, got {low}");
        assert!(
            (corner - 0.707).abs() < 0.02,
            "the corner should be 3 dB down, got {corner}"
        );
        assert!(high > 0.98, "the passband should be flat, got {high}");
    }

    #[test]
    fn an_all_pass_preserves_magnitude_at_every_frequency() {
        // The property the equaliser's phase compensation depends on.
        let rate = SampleRate::HZ_48000;
        for frequency in [50.0, 200.0, 1_000.0, 2_000.0, 8_000.0, 16_000.0] {
            let mut filter = Biquad::new();
            filter.set_coefficients(BiquadCoefficients::all_pass(2_000.0, BUTTERWORTH_Q, rate));
            let magnitude = magnitude_at(&mut filter, frequency, rate);
            assert!(
                (magnitude - 1.0).abs() < 0.02,
                "all-pass magnitude at {frequency} Hz was {magnitude}, expected 1"
            );
        }
    }

    #[test]
    fn resetting_clears_the_tail() {
        let rate = SampleRate::HZ_48000;
        let mut filter = Biquad::new();
        filter.set_coefficients(BiquadCoefficients::low_pass(500.0, BUTTERWORTH_Q, rate));

        for _ in 0..1_000 {
            let _ = filter.process(1.0);
        }
        filter.reset();
        // With the state cleared, the first sample of silence must produce
        // silence. Master Prompt #15: silence should remain silent.
        assert_eq!(filter.process(0.0), 0.0);
    }

    #[test]
    fn state_decays_to_exactly_zero_rather_than_into_denormals() {
        let rate = SampleRate::HZ_48000;
        let mut filter = Biquad::new();
        filter.set_coefficients(BiquadCoefficients::low_pass(1_000.0, BUTTERWORTH_Q, rate));

        let _ = filter.process(1.0);
        for _ in 0..100_000 {
            let _ = filter.process(0.0);
        }
        assert_eq!(filter.s1, 0.0, "state must flush rather than grind");
        assert_eq!(filter.s2, 0.0);
    }

    #[test]
    fn a_cutoff_beyond_nyquist_is_clamped_rather_than_producing_nonsense() {
        let rate = SampleRate::HZ_48000;
        let mut filter = Biquad::new();
        filter.set_coefficients(BiquadCoefficients::low_pass(100_000.0, BUTTERWORTH_Q, rate));
        // Whatever it does, it must remain finite and stable.
        for index in 0..10_000_i32 {
            let output = filter.process((f64::from(index) * 0.1).sin());
            assert!(output.is_finite(), "filter became unstable");
        }
    }

    #[test]
    fn a_degenerate_q_is_replaced_rather_than_dividing_by_zero() {
        let rate = SampleRate::HZ_48000;
        let mut filter = Biquad::new();
        filter.set_coefficients(BiquadCoefficients::low_pass(1_000.0, 0.0, rate));
        for _ in 0..1_000 {
            assert!(filter.process(0.5).is_finite());
        }
        filter.set_coefficients(BiquadCoefficients::low_pass(1_000.0, f64::NAN, rate));
        filter.reset();
        for _ in 0..1_000 {
            assert!(filter.process(0.5).is_finite());
        }
    }
}
