//! The discrete Fourier transform, written here rather than depended upon.
//!
//! # Why this is not a dependency
//!
//! ADR-0001 keeps the portable core free of runtime dependencies. That is not
//! austerity for its own sake: the core is the layer that must compile
//! unchanged for macOS, iOS, Android and WebAssembly, must produce
//! bit-identical results on all of them so that a preview and an export agree,
//! and must remain auditable under the security review of Master Prompt #26. A
//! transform is a few hundred lines of well-understood mathematics; a
//! dependency is a supply chain.
//!
//! The requirement that makes writing it the *cheaper* option is determinism.
//! Several excellent transform libraries dispatch on runtime CPU features, so
//! the same input can produce results differing in the last bits between an
//! Apple Silicon Mac and an x86 build machine. For a spectrum that difference
//! is inaudible; for a *decision derived from* the spectrum — this is the
//! downbeat, this is the key — it can change the answer, and ADR-0006 requires
//! the same inputs to produce the same set.
//!
//! # Correctness
//!
//! The transform is verified against a directly evaluated discrete Fourier
//! transform, which is the definition rather than another implementation of the
//! same shortcut. Testing a fast transform against another fast transform can
//! agree on a shared misunderstanding; testing it against the sum it is an
//! optimisation of cannot.

use core::f64::consts::TAU;

use crate::error::AnalysisError;
use crate::num::count_to_f64;

/// The smallest transform size the crate accepts.
const MINIMUM_SIZE: usize = 4;

/// A complex number, in the form the transform needs.
///
/// Deliberately minimal. Only the operations a radix-2 butterfly performs are
/// defined, so there is no arithmetic here whose numerical behaviour is not
/// exercised by the transform's own tests.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Complex {
    /// The real part.
    pub re: f64,
    /// The imaginary part.
    pub im: f64,
}

impl Complex {
    /// The additive identity.
    pub const ZERO: Self = Self { re: 0.0, im: 0.0 };

    /// Creates a complex number from its parts.
    #[must_use]
    pub const fn new(re: f64, im: f64) -> Self {
        Self { re, im }
    }

    /// Returns the complex conjugate.
    #[must_use]
    pub const fn conjugate(self) -> Self {
        Self {
            re: self.re,
            im: -self.im,
        }
    }

    /// Returns the magnitude.
    ///
    /// Uses [`f64::hypot`] rather than the square root of the sum of squares.
    /// The naive form overflows for large inputs and loses precision for small
    /// ones; neither arises at audio amplitudes, but the correct form costs
    /// nothing here and removes a footgun for anyone who reuses this type.
    #[must_use]
    pub fn magnitude(self) -> f64 {
        self.re.hypot(self.im)
    }

    /// Returns the squared magnitude, which needs no square root.
    #[must_use]
    pub fn magnitude_squared(self) -> f64 {
        self.re.mul_add(self.re, self.im * self.im)
    }

    /// Returns the phase angle in radians.
    #[must_use]
    pub fn phase(self) -> f64 {
        self.im.atan2(self.re)
    }

    #[must_use]
    const fn add(self, other: Self) -> Self {
        Self {
            re: self.re + other.re,
            im: self.im + other.im,
        }
    }

    #[must_use]
    const fn sub(self, other: Self) -> Self {
        Self {
            re: self.re - other.re,
            im: self.im - other.im,
        }
    }

    #[must_use]
    fn mul(self, other: Self) -> Self {
        Self {
            re: self.re.mul_add(other.re, -(self.im * other.im)),
            im: self.re.mul_add(other.im, self.im * other.re),
        }
    }

    #[must_use]
    const fn scale(self, factor: f64) -> Self {
        Self {
            re: self.re * factor,
            im: self.im * factor,
        }
    }

    /// Multiplies by `-i`, which the real-input untangling needs and which is
    /// exact — a swap and a negation, with no rounding at all.
    #[must_use]
    const fn mul_neg_i(self) -> Self {
        Self {
            re: self.im,
            im: -self.re,
        }
    }
}

/// A planned complex-to-complex fast Fourier transform of a fixed size.
///
/// The twiddle factors are computed once, at construction, off the analysis
/// hot path. Recomputing them inside the butterfly loop — which several
/// textbook implementations do — costs a transcendental function per butterfly
/// and, worse, accumulates a different rounding pattern depending on the order
/// the compiler chooses to evaluate them in.
#[derive(Debug, Clone)]
pub struct Fft {
    size: usize,
    /// `twiddles[k] = exp(-2πik/size)` for `k` in `0..size/2`.
    twiddles: Vec<Complex>,
}

impl Fft {
    /// Plans a transform of the given size.
    ///
    /// # Errors
    ///
    /// Returns [`AnalysisError::SizeNotPowerOfTwo`] or
    /// [`AnalysisError::SizeTooSmall`] if the size is unusable.
    pub fn new(size: usize) -> Result<Self, AnalysisError> {
        if size < MINIMUM_SIZE {
            return Err(AnalysisError::SizeTooSmall {
                size,
                minimum: MINIMUM_SIZE,
            });
        }
        if !size.is_power_of_two() {
            return Err(AnalysisError::SizeNotPowerOfTwo { size });
        }

        let half = size >> 1;
        let mut twiddles = Vec::with_capacity(half);
        let denominator = count_to_f64(size);
        for k in 0..half {
            // Computed from the index rather than by rotating a running value.
            // A running rotation drifts: after a thousand multiplications the
            // accumulated error is visible in the spectrum as a raised noise
            // floor, and it differs between platforms.
            let angle = -TAU * count_to_f64(k) / denominator;
            twiddles.push(Complex::new(angle.cos(), angle.sin()));
        }

        Ok(Self { size, twiddles })
    }

    /// The transform size.
    #[must_use]
    pub const fn size(&self) -> usize {
        self.size
    }

    /// Transforms `data` in place.
    ///
    /// # Errors
    ///
    /// Returns [`AnalysisError::BufferLength`] if `data` is not exactly
    /// [`Fft::size`] elements long.
    pub fn forward(&self, data: &mut [Complex]) -> Result<(), AnalysisError> {
        if data.len() != self.size {
            return Err(AnalysisError::BufferLength {
                expected: self.size,
                actual: data.len(),
            });
        }

        bit_reverse_permute(data);

        // `half` is the size of each half-block being combined; `stride` is the
        // step through the twiddle table that keeps the angle correct as blocks
        // grow. Both are powers of two and are shifted rather than divided,
        // which is exact and keeps the workspace's ban on integer division
        // meaningful — the ban exists to catch discarded remainders, and a
        // shift by construction has none.
        let mut half = 1_usize;
        let mut stride = self.size >> 1;
        while half < self.size {
            for block in data.chunks_exact_mut(half << 1) {
                let (lower, upper) = block.split_at_mut(half);
                for (k, (low, high)) in lower.iter_mut().zip(upper.iter_mut()).enumerate() {
                    let Some(&twiddle) = self.twiddles.get(k * stride) else {
                        // Unreachable: `k < half` and `half * stride == size/2`.
                        // Returning rather than asserting keeps the transform
                        // panic-free, which matters because a future caller may
                        // run it from a latency-sensitive context.
                        return Err(AnalysisError::BufferLength {
                            expected: self.size,
                            actual: data.len(),
                        });
                    };
                    let product = twiddle.mul(*high);
                    *high = low.sub(product);
                    *low = low.add(product);
                }
            }
            half <<= 1;
            stride >>= 1;
        }

        Ok(())
    }
}

/// Reorders a buffer into bit-reversed index order, in place.
fn bit_reverse_permute(data: &mut [Complex]) {
    let size = data.len();
    let mut target = 0_usize;
    for source in 1..size {
        let mut bit = size >> 1;
        while target & bit != 0 {
            target ^= bit;
            bit >>= 1;
        }
        target |= bit;
        if source < target {
            data.swap(source, target);
        }
    }
}

/// A transform specialised for real input.
///
/// A real signal of length `n` has a conjugate-symmetric spectrum, so half of
/// what a complex transform computes is redundant. Packing the even and odd
/// samples into a complex sequence of length `n/2`, transforming that, and
/// untangling the result costs a little under half as much time and half as
/// much memory as transforming the real signal directly.
///
/// The saving is not a micro-optimisation. A ten-minute track analysed with a
/// 2048-sample window and a 512-sample hop runs roughly fifty thousand
/// transforms, and the analysis of a freshly imported library runs that for
/// every track. Halving it is the difference between an import that finishes
/// while the user makes coffee and one that does not.
#[derive(Debug, Clone)]
pub struct RealFft {
    /// The complex transform of half the real length.
    inner: Fft,
    /// `untwiddle[k] = exp(-2πik/n)` for `k` in `0..=n/4`, the rotation the
    /// untangling applies. Only a quarter of the circle is stored because the
    /// untangling walks `k` from `0` to `n/4` and mirrors the rest.
    untwiddle: Vec<Complex>,
    /// Scratch for the packed half-length sequence.
    packed: Vec<Complex>,
    /// The real input length.
    size: usize,
}

impl RealFft {
    /// Plans a real-input transform of the given size.
    ///
    /// # Errors
    ///
    /// Returns [`AnalysisError::SizeNotPowerOfTwo`] or
    /// [`AnalysisError::SizeTooSmall`] if the size is unusable. The minimum is
    /// twice that of the complex transform, because half of it is what runs.
    pub fn new(size: usize) -> Result<Self, AnalysisError> {
        if size < MINIMUM_SIZE << 1 {
            return Err(AnalysisError::SizeTooSmall {
                size,
                minimum: MINIMUM_SIZE << 1,
            });
        }
        if !size.is_power_of_two() {
            return Err(AnalysisError::SizeNotPowerOfTwo { size });
        }

        let half = size >> 1;
        let inner = Fft::new(half)?;

        let quarter = half >> 1;
        let mut untwiddle = Vec::with_capacity(quarter + 1);
        let denominator = count_to_f64(size);
        for k in 0..=quarter {
            let angle = -TAU * count_to_f64(k) / denominator;
            untwiddle.push(Complex::new(angle.cos(), angle.sin()));
        }

        Ok(Self {
            inner,
            untwiddle,
            packed: vec![Complex::ZERO; half],
            size,
        })
    }

    /// The real input length.
    #[must_use]
    pub const fn size(&self) -> usize {
        self.size
    }

    /// The number of spectrum bins produced, which is `size / 2 + 1`.
    #[must_use]
    pub const fn bins(&self) -> usize {
        (self.size >> 1) + 1
    }

    /// Transforms `input` into `spectrum`.
    ///
    /// `spectrum` receives [`RealFft::bins`] values covering direct current up
    /// to and including Nyquist. The caller supplies the output buffer so that
    /// analysing a whole track allocates once rather than once per frame.
    ///
    /// # Errors
    ///
    /// Returns [`AnalysisError::BufferLength`] if either buffer is the wrong
    /// length.
    pub fn forward(
        &mut self,
        input: &[f64],
        spectrum: &mut [Complex],
    ) -> Result<(), AnalysisError> {
        if input.len() != self.size {
            return Err(AnalysisError::BufferLength {
                expected: self.size,
                actual: input.len(),
            });
        }
        if spectrum.len() != self.bins() {
            return Err(AnalysisError::BufferLength {
                expected: self.bins(),
                actual: spectrum.len(),
            });
        }

        // Pack: z[k] = x[2k] + i·x[2k+1].
        for (slot, pair) in self.packed.iter_mut().zip(input.chunks_exact(2)) {
            let (Some(&even), Some(&odd)) = (pair.first(), pair.get(1)) else {
                // Unreachable for a `chunks_exact(2)` chunk; handled rather than
                // asserted to keep the transform panic-free.
                return Err(AnalysisError::BufferLength {
                    expected: self.size,
                    actual: input.len(),
                });
            };
            *slot = Complex::new(even, odd);
        }

        self.inner.forward(&mut self.packed)?;

        self.untangle(spectrum)
    }

    /// Recovers the spectrum of the real signal from the packed transform.
    ///
    /// With `Z` the transform of the packed sequence and `m = n/2`,
    ///
    /// ```text
    /// Ze[k] = (Z[k] + conj(Z[m-k])) / 2          the transform of the even samples
    /// Zo[k] = (Z[k] - conj(Z[m-k])) · (-i) / 2   the transform of the odd samples
    /// X[k]  = Ze[k] + exp(-2πik/n) · Zo[k]
    /// ```
    ///
    /// The two halves are filled together, from the outside in, because each
    /// pair `(k, m-k)` is derived from the same two packed values.
    fn untangle(&self, spectrum: &mut [Complex]) -> Result<(), AnalysisError> {
        let half = self.size >> 1;
        let lengths = AnalysisError::BufferLength {
            expected: self.bins(),
            actual: spectrum.len(),
        };

        // Direct current and Nyquist are real and come straight from the packed
        // transform's first bin, which carries the sum and difference of the
        // even and odd sums.
        let Some(&first) = self.packed.first() else {
            return Err(lengths);
        };
        let Some(dc) = spectrum.first_mut() else {
            return Err(lengths);
        };
        *dc = Complex::new(first.re + first.im, 0.0);
        let Some(nyquist) = spectrum.get_mut(half) else {
            return Err(lengths);
        };
        *nyquist = Complex::new(first.re - first.im, 0.0);

        for k in 1..=(half >> 1) {
            let mirror = half - k;
            let (Some(&low), Some(&high)) = (self.packed.get(k), self.packed.get(mirror)) else {
                return Err(lengths);
            };
            let Some(&rotation) = self.untwiddle.get(k) else {
                return Err(lengths);
            };

            let conjugate = high.conjugate();
            let even = low.add(conjugate).scale(0.5);
            let odd = low.sub(conjugate).mul_neg_i().scale(0.5);
            let rotated = rotation.mul(odd);

            let forward_bin = even.add(rotated);
            // The mirrored bin follows from the same pair without a second
            // rotation: X[m-k] = conj(Ze[k] - exp(-2πik/n)·Zo[k]).
            let mirrored_bin = even.sub(rotated).conjugate();

            let Some(slot) = spectrum.get_mut(k) else {
                return Err(lengths);
            };
            *slot = forward_bin;
            let Some(slot) = spectrum.get_mut(mirror) else {
                return Err(lengths);
            };
            *slot = mirrored_bin;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::float_cmp,
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a test that cannot build its own fixture should fail loudly, and the signal \
                  generators here work in exact, bounded quantities"
    )]

    use super::*;

    /// The transform, evaluated directly from its definition.
    ///
    /// Deliberately the slow sum: this is the specification the fast transform
    /// is an optimisation of, so agreement with it is evidence, whereas
    /// agreement with another fast transform would only show that two
    /// implementations share an idea.
    fn naive_dft(input: &[f64]) -> Vec<Complex> {
        let n = input.len();
        let mut output = Vec::with_capacity(n);
        for k in 0..n {
            let mut sum = Complex::ZERO;
            for (index, &sample) in input.iter().enumerate() {
                let angle = -TAU * count_to_f64(k) * count_to_f64(index) / count_to_f64(n);
                sum = sum.add(Complex::new(sample * angle.cos(), sample * angle.sin()));
            }
            output.push(sum);
        }
        output
    }

    /// Compares two spectrum values against a tolerance scaled by the size of
    /// the transform.
    ///
    /// A fixed absolute tolerance would be the wrong test. Both sides of this
    /// comparison sum `n` products, so both accumulate rounding error, and for
    /// a sum of terms with mixed signs that error grows about as the square
    /// root of `n`. The reference is in fact the *less* accurate of the two:
    /// the direct sum adds `n` terms in one accumulator, while the fast
    /// transform's tree of butterflies has depth `log n`. A tolerance that
    /// ignored this would be measuring the transform size rather than its
    /// correctness — and it would tighten as the test got more demanding, which
    /// is precisely backwards.
    ///
    /// The bound is therefore relative to the largest value the spectrum can
    /// hold and grows with the square root of the size. A genuinely wrong
    /// transform misses it by many orders of magnitude, so nothing is given
    /// away.
    fn assert_close(actual: Complex, expected: Complex, scale: f64, size: usize, context: &str) {
        let tolerance = scale * f64::EPSILON * 8.0 * count_to_f64(size).sqrt();
        let difference = (actual.re - expected.re)
            .abs()
            .max((actual.im - expected.im).abs());
        assert!(
            difference <= tolerance,
            "{context}: got {actual:?}, expected {expected:?}, difference {difference} \
             exceeds {tolerance}"
        );
    }

    fn deterministic_signal(length: usize) -> Vec<f64> {
        // A fixed pseudo-random sequence: deterministic so a failure is
        // reproducible, broadband so every bin is exercised.
        let mut noise = crate::testing::Noise::new(0x2545_F491_4F6C_DD1D);
        (0..length).map(|_| noise.next_bipolar()).collect()
    }

    /// The largest magnitude the transform of this signal can produce, which is
    /// the sum of the absolute sample values.
    fn spectrum_scale(signal: &[f64]) -> f64 {
        signal.iter().map(|value| value.abs()).sum()
    }

    #[test]
    fn complex_transform_matches_the_definition() {
        for &size in &[4_usize, 8, 16, 64, 256] {
            let signal = deterministic_signal(size);
            let expected = naive_dft(&signal);

            let fft = Fft::new(size).expect("power-of-two size is valid");
            let mut data: Vec<Complex> = signal.iter().map(|&s| Complex::new(s, 0.0)).collect();
            fft.forward(&mut data).expect("length matches the plan");

            let scale = spectrum_scale(&signal);
            for (bin, (actual, wanted)) in data.iter().zip(expected.iter()).enumerate() {
                assert_close(
                    *actual,
                    *wanted,
                    scale,
                    size,
                    &format!("size {size} bin {bin}"),
                );
            }
        }
    }

    #[test]
    fn real_transform_matches_the_definition() {
        for &size in &[8_usize, 16, 64, 512, 2048] {
            let signal = deterministic_signal(size);
            let expected = naive_dft(&signal);

            let mut fft = RealFft::new(size).expect("power-of-two size is valid");
            let mut spectrum = vec![Complex::ZERO; fft.bins()];
            fft.forward(&signal, &mut spectrum)
                .expect("lengths match the plan");

            let scale = spectrum_scale(&signal);
            for (bin, actual) in spectrum.iter().enumerate() {
                let wanted = expected.get(bin).copied().expect("bins are a prefix");
                assert_close(
                    *actual,
                    wanted,
                    scale,
                    size,
                    &format!("size {size} bin {bin}"),
                );
            }
        }
    }

    #[test]
    fn a_pure_tone_lands_in_exactly_one_bin() {
        // The property that matters for every consumer of this file: a
        // sinusoid at a bin centre must not leak into its neighbours. Leakage
        // here would appear downstream as a smeared chroma and a key detector
        // that hedges between neighbouring pitches.
        let size = 1024_usize;
        let bin = 40_usize;
        let signal: Vec<f64> = (0..size)
            .map(|n| (TAU * count_to_f64(bin) * count_to_f64(n) / count_to_f64(size)).cos())
            .collect();

        let mut fft = RealFft::new(size).expect("valid size");
        let mut spectrum = vec![Complex::ZERO; fft.bins()];
        fft.forward(&signal, &mut spectrum).expect("valid lengths");

        let peak = spectrum
            .get(bin)
            .copied()
            .expect("bin is inside the spectrum")
            .magnitude();
        assert!(
            (peak - count_to_f64(size) * 0.5).abs() < 1e-6,
            "a full-scale cosine at a bin centre has magnitude n/2, got {peak}"
        );

        for (index, value) in spectrum.iter().enumerate() {
            if index != bin {
                assert!(
                    value.magnitude() < 1e-8,
                    "bin {index} should be empty, has {}",
                    value.magnitude()
                );
            }
        }
    }

    #[test]
    fn direct_current_and_nyquist_are_real() {
        let size = 64_usize;
        let signal: Vec<f64> = (0..size)
            .map(|n| if n % 2 == 0 { 1.0 } else { -1.0 })
            .collect();

        let mut fft = RealFft::new(size).expect("valid size");
        let mut spectrum = vec![Complex::ZERO; fft.bins()];
        fft.forward(&signal, &mut spectrum).expect("valid lengths");

        let dc = spectrum.first().copied().expect("bins are non-empty");
        let nyquist = spectrum.last().copied().expect("bins are non-empty");
        assert!(dc.im.abs() < 1e-12 && dc.re.abs() < 1e-12);
        assert!(nyquist.im.abs() < 1e-12);
        assert!((nyquist.re - count_to_f64(size)).abs() < 1e-9);
    }

    #[test]
    fn invalid_sizes_are_rejected_rather_than_rounded() {
        assert_eq!(
            Fft::new(3).err(),
            Some(AnalysisError::SizeTooSmall {
                size: 3,
                minimum: MINIMUM_SIZE
            })
        );
        assert_eq!(
            Fft::new(48).err(),
            Some(AnalysisError::SizeNotPowerOfTwo { size: 48 })
        );
        assert_eq!(
            RealFft::new(1000).err(),
            Some(AnalysisError::SizeNotPowerOfTwo { size: 1000 })
        );
    }

    #[test]
    fn a_mismatched_buffer_is_an_error_not_a_partial_transform() {
        let fft = Fft::new(8).expect("valid size");
        let mut data = vec![Complex::ZERO; 7];
        assert_eq!(
            fft.forward(&mut data).err(),
            Some(AnalysisError::BufferLength {
                expected: 8,
                actual: 7
            })
        );
    }

    #[test]
    fn linearity_holds_across_the_real_transform() {
        // Linearity is the property every downstream stage silently relies on:
        // spectral flux compares frames, and chroma sums bins. If the transform
        // were not linear those operations would be meaningless.
        let size = 256_usize;
        let first = deterministic_signal(size);
        let second: Vec<f64> = deterministic_signal(size).iter().map(|s| s * 0.3).collect();
        let sum: Vec<f64> = first
            .iter()
            .zip(second.iter())
            .map(|(a, b)| a + 2.0 * b)
            .collect();

        let mut fft = RealFft::new(size).expect("valid size");
        let mut spectrum_first = vec![Complex::ZERO; fft.bins()];
        let mut spectrum_second = vec![Complex::ZERO; fft.bins()];
        let mut spectrum_sum = vec![Complex::ZERO; fft.bins()];
        fft.forward(&first, &mut spectrum_first).expect("valid");
        fft.forward(&second, &mut spectrum_second).expect("valid");
        fft.forward(&sum, &mut spectrum_sum).expect("valid");

        let scale = spectrum_scale(&sum);
        for (index, combined) in spectrum_sum.iter().enumerate() {
            let a = spectrum_first.get(index).copied().expect("same length");
            let b = spectrum_second.get(index).copied().expect("same length");
            assert_close(*combined, a.add(b.scale(2.0)), scale, size, "linearity");
        }
    }
}
