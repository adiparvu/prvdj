//! Analysis windows.
//!
//! A window is not decoration. Cutting a finite block out of a continuous
//! signal is itself a multiplication by a rectangle, and a rectangle's spectrum
//! has sidelobes that fall off at only 6 dB per octave. Left uncorrected, a
//! loud bass note leaks across the whole spectrum and every stage downstream —
//! flux, chroma, spectral balance — reads that leakage as content.

use crate::error::AnalysisError;
use crate::num::count_to_f64;

use core::f64::consts::TAU;

/// The window shapes this crate uses.
///
/// The list is short on purpose. Each shape is here because a specific stage
/// needs its particular trade-off between mainlobe width — how well two nearby
/// partials are told apart — and sidelobe level — how much a loud partial
/// contaminates a quiet one. Offering a catalogue would invite choosing by
/// name rather than by requirement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum WindowShape {
    /// Hann: sidelobes at −31 dB falling at 18 dB per octave.
    ///
    /// The default for onset and flux work. Its fast sidelobe roll-off means a
    /// kick drum stops contaminating the mid band within a few bins, which is
    /// what keeps spectral flux measuring an actual event rather than the
    /// smear of the previous one.
    Hann,

    /// Blackman-Harris: sidelobes at −92 dB.
    ///
    /// Used for tonal work. The mainlobe is twice as wide as Hann's, which
    /// costs frequency resolution, but chroma sums energy across a whole
    /// semitone anyway, so the width is free while the extra 60 dB of sidelobe
    /// rejection is not: it is the difference between a bass fundamental
    /// leaking into the chroma of every other pitch class and it not.
    BlackmanHarris,
}

impl WindowShape {
    /// Fills `window` with this shape.
    ///
    /// The periodic rather than symmetric form is used: sample `n` of `N` is
    /// evaluated at `n/N`, not `n/(N-1)`. For spectral analysis with
    /// overlapping frames the periodic form is the correct one — it makes the
    /// window sum to a constant under 50 per cent overlap, so a steady signal
    /// produces a steady analysis instead of one that ripples at the frame
    /// rate.
    ///
    /// # Errors
    ///
    /// Returns [`AnalysisError::SizeTooSmall`] for a window shorter than two
    /// samples, which has no meaningful shape.
    pub fn fill(self, window: &mut [f64]) -> Result<(), AnalysisError> {
        let length = window.len();
        if length < 2 {
            return Err(AnalysisError::SizeTooSmall {
                size: length,
                minimum: 2,
            });
        }

        let denominator = count_to_f64(length);
        for (index, slot) in window.iter_mut().enumerate() {
            let phase = TAU * count_to_f64(index) / denominator;
            *slot = match self {
                Self::Hann => 0.5 * (1.0 - phase.cos()),
                Self::BlackmanHarris => {
                    // Four-term coefficients, minimum sidelobe form.
                    0.358_75 - 0.488_29 * phase.cos() + 0.141_28 * (2.0 * phase).cos()
                        - 0.011_68 * (3.0 * phase).cos()
                }
            };
        }

        Ok(())
    }
}

/// A precomputed analysis window.
///
/// Computed once per plan. Recomputing the shape per frame would call four
/// cosines per sample per frame, which for a ten-minute track is a few hundred
/// million transcendental evaluations spent reproducing a constant.
#[derive(Debug, Clone)]
pub struct Window {
    shape: WindowShape,
    values: Vec<f64>,
    coherent_gain: f64,
}

impl Window {
    /// Builds a window of the given shape and length.
    ///
    /// # Errors
    ///
    /// Returns [`AnalysisError::SizeTooSmall`] for a length below two.
    pub fn new(shape: WindowShape, length: usize) -> Result<Self, AnalysisError> {
        let mut values = vec![0.0; length];
        shape.fill(&mut values)?;
        let sum: f64 = values.iter().sum();
        let coherent_gain = sum / count_to_f64(length);
        Ok(Self {
            shape,
            values,
            coherent_gain,
        })
    }

    /// The shape this window was built from.
    #[must_use]
    pub const fn shape(&self) -> WindowShape {
        self.shape
    }

    /// The window values.
    #[must_use]
    pub fn values(&self) -> &[f64] {
        &self.values
    }

    /// The window length.
    #[must_use]
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// Whether the window is empty, which construction prevents.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// The coherent gain: the mean window value.
    ///
    /// A sinusoid at a bin centre comes out of the transform scaled by this
    /// factor times half the window length. Any stage that reports an absolute
    /// level — rather than comparing one frame with another — must divide it
    /// out, or a Hann-windowed measurement reads 6 dB low and a
    /// Blackman-Harris one reads 9 dB low, purely because of the window.
    #[must_use]
    pub const fn coherent_gain(&self) -> f64 {
        self.coherent_gain
    }

    /// Multiplies `block` by the window, in place.
    ///
    /// # Errors
    ///
    /// Returns [`AnalysisError::BufferLength`] if `block` is not the window
    /// length.
    pub fn apply(&self, block: &mut [f64]) -> Result<(), AnalysisError> {
        if block.len() != self.values.len() {
            return Err(AnalysisError::BufferLength {
                expected: self.values.len(),
                actual: block.len(),
            });
        }
        for (sample, &weight) in block.iter_mut().zip(self.values.iter()) {
            *sample *= weight;
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

    #[test]
    fn a_periodic_hann_window_sums_to_a_constant_under_half_overlap() {
        // The property that makes overlapping analysis stable. If the
        // overlapped windows did not sum flat, a steady tone would produce a
        // novelty curve that pulsed at the frame rate, and the tempo estimator
        // would happily lock onto that pulse and report the hop rate as the
        // tempo. This test exists because that failure looks entirely
        // plausible in a plot.
        let size = 64_usize;
        let hop = size >> 1;
        let window = Window::new(WindowShape::Hann, size).expect("valid length");

        for offset in 0..hop {
            let first = window.values().get(offset).copied().expect("in range");
            let second = window
                .values()
                .get(offset + hop)
                .copied()
                .expect("in range");
            assert!(
                (first + second - 1.0).abs() < 1e-12,
                "overlap at {offset} sums to {}",
                first + second
            );
        }
    }

    #[test]
    fn window_endpoints_are_zero_or_near_it() {
        let hann = Window::new(WindowShape::Hann, 32).expect("valid");
        assert!(hann.values().first().copied().unwrap_or(1.0).abs() < 1e-15);

        let bh = Window::new(WindowShape::BlackmanHarris, 32).expect("valid");
        // The four-term coefficients sum to a small residue rather than exactly
        // zero; anything under a thousandth is inaudible as leakage.
        assert!(bh.values().first().copied().unwrap_or(1.0).abs() < 1e-3);
    }

    #[test]
    fn blackman_harris_rejects_far_sidelobes_far_better_than_hann() {
        // The reason two shapes exist, measured rather than quoted from a
        // textbook, so that a coefficient typo is caught here instead of
        // surfacing as a key detector that hedges.
        //
        // The measurement must be zero-padded. Transforming a window at its own
        // length samples its spectrum exactly at the zeros between the
        // sidelobes: a Hann window is a sum of three complex exponentials, so
        // its own-length transform is *exactly* three non-zero bins and nothing
        // else. That reads as −300 dB of sidelobes for both shapes and would
        // have let any coefficients at all pass. Padding sixteenfold samples
        // between the zeros, which is where the sidelobes actually are.
        use crate::fft::{Complex, RealFft};

        const WINDOW: usize = 256;
        const PADDING: usize = 16;
        let size = WINDOW * PADDING;
        let mut fft = RealFft::new(size).expect("valid size");

        // Beyond the mainlobe of the wider of the two shapes. Blackman-Harris
        // has a mainlobe four window-bins wide either side; measuring from six
        // leaves it out of the comparison entirely.
        let first_sidelobe_bin = 6 * PADDING;

        let mut peak_sidelobe = |shape: WindowShape| -> f64 {
            let window = Window::new(shape, WINDOW).expect("valid");
            let mut block = vec![0.0_f64; size];
            for (slot, &value) in block.iter_mut().zip(window.values().iter()) {
                *slot = value;
            }
            let mut spectrum = vec![Complex::ZERO; fft.bins()];
            fft.forward(&block, &mut spectrum).expect("valid");

            let peak = spectrum
                .first()
                .copied()
                .expect("non-empty")
                .magnitude()
                .max(f64::MIN_POSITIVE);
            spectrum
                .iter()
                .skip(first_sidelobe_bin)
                .map(|value| 20.0 * (value.magnitude() / peak).max(1e-30).log10())
                .fold(f64::NEG_INFINITY, f64::max)
        };

        let hann = peak_sidelobe(WindowShape::Hann);
        let blackman = peak_sidelobe(WindowShape::BlackmanHarris);

        assert!(hann < -40.0, "Hann sidelobes measured at {hann} dB");
        assert!(
            blackman < -85.0,
            "Blackman-Harris sidelobes measured at {blackman} dB"
        );
        assert!(
            blackman < hann - 30.0,
            "the tonal window must be far quieter outside its mainlobe: {blackman} dB against \
             Hann's {hann} dB"
        );
    }

    #[test]
    fn coherent_gain_matches_the_known_values() {
        let hann = Window::new(WindowShape::Hann, 1024).expect("valid");
        assert!((hann.coherent_gain() - 0.5).abs() < 1e-12);

        let bh = Window::new(WindowShape::BlackmanHarris, 1024).expect("valid");
        assert!((bh.coherent_gain() - 0.358_75).abs() < 1e-6);
    }

    #[test]
    fn a_window_of_one_sample_is_rejected() {
        assert_eq!(
            Window::new(WindowShape::Hann, 1).err(),
            Some(AnalysisError::SizeTooSmall {
                size: 1,
                minimum: 2
            })
        );
    }
}
