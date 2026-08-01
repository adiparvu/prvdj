//! The short-time Fourier transform, streamed rather than stored.
//!
//! # Why nothing is materialised
//!
//! The obvious shape for this module is a `Spectrogram` type holding every
//! frame, which every stage then reads. The arithmetic says no: ten minutes at
//! 44.1 kHz with a 2048-sample window and a 512-sample hop is about fifty-two
//! thousand frames of 1025 bins. Even at single precision that is 210 megabytes
//! for one track, and Module Specification #001 sizes the library at a hundred
//! thousand of them.
//!
//! So the transform pushes frames to a consumer and keeps none. Every stage
//! that needs the spectrum reduces it as it arrives — flux to one number per
//! frame, chroma to twelve, band energy to a handful — and only those reductions
//! are stored. The whole analysis of a track then costs a few hundred kilobytes
//! rather than a few hundred megabytes, and a library-wide re-analysis becomes
//! something that can run in the background rather than something that must be
//! scheduled around memory.
//!
//! It also means each stage sees each frame exactly once, in order, which is
//! what allows analysis to run against a decoder that streams rather than
//! requiring the whole track decoded into memory first.
//!
//! # Not on the audio thread
//!
//! Nothing here obeys ADR-0002. Analysis allocates at construction, runs for
//! seconds, and is explicitly an off-thread activity. The realtime contract
//! applies to the signal path in `prv-dsp`; conflating the two would force
//! analysis into a shape that serves neither.

use prv_time::SampleRate;

use crate::error::AnalysisError;
use crate::fft::{Complex, RealFft};
use crate::num::{count_to_f64, ratio};
use crate::window::{Window, WindowShape};

/// The analysis window used for rhythm, in samples.
///
/// About 23 milliseconds at 44.1 kHz. Rhythm analysis is a trade against the
/// uncertainty principle in the direction opposite to tonal analysis, and the
/// reason is a systematic error rather than a preference.
///
/// Spectral flux fires when a transient *enters* the analysis window, not when
/// the window is centred on it, so every onset is detected early by up to half
/// a window. That error is a constant offset applied to the whole beat grid,
/// which is the one kind of timing error a listener notices immediately: the
/// grid sits ahead of the drums. Halving the window halves the offset. At this
/// size it is a few milliseconds, below the threshold where a DJ would reach
/// for the nudge control.
pub const RHYTHM_WINDOW_SIZE: usize = 1024;

/// The hop used for rhythm, in samples.
///
/// A quarter of the window: 75 per cent overlap, giving a novelty value every
/// 5.8 milliseconds at 44.1 kHz. The hop bounds how precisely any onset can be
/// placed, and a beat grid quantised to 12 milliseconds is visibly off the
/// transients when the waveform is zoomed in, which Module Specification #003
/// makes an ordinary thing for a user to do.
pub const RHYTHM_HOP_SIZE: usize = 256;

/// The analysis window used for tonal work, in samples.
///
/// About 93 milliseconds at 44.1 kHz, resolving to 11 Hz. Key detection needs
/// to separate adjacent semitones, and in the octave below middle C those are
/// less than 15 Hz apart; a shorter window merges them and the chroma of a bass
/// line becomes a smear across three pitch classes. The cost — poor time
/// resolution — does not matter, because a key is a property of a passage
/// rather than of an instant.
pub const TONAL_WINDOW_SIZE: usize = 4096;

/// The hop used for tonal work, in samples.
pub const TONAL_HOP_SIZE: usize = 1024;

/// One analysed frame, borrowed for the duration of the callback.
///
/// Borrowed rather than owned so that streaming allocates nothing per frame.
/// A consumer that needs to keep something copies out the reduction it cares
/// about, which is the point of the design.
#[derive(Debug, Clone, Copy)]
pub struct SpectrumFrame<'a> {
    index: usize,
    centre_sample: usize,
    window_size: usize,
    sample_rate: SampleRate,
    magnitude: &'a [f64],
}

impl SpectrumFrame<'_> {
    /// The frame's ordinal position, counting from zero.
    #[must_use]
    pub const fn index(&self) -> usize {
        self.index
    }

    /// The sample offset at the centre of this frame's window.
    ///
    /// The centre, not the start, is what an event detected in this frame
    /// should be attributed to. A window is a weighted average over its whole
    /// span, and a Hann window's weight is concentrated at its middle;
    /// timestamping at the start would place every onset half a window early,
    /// which at the default settings is 23 milliseconds — plainly visible as a
    /// beat grid sitting ahead of the transients.
    ///
    /// Because the analysis is centred, frame `i` is centred at exactly
    /// `i × hop`, so this is an index into the curve rather than a correction
    /// to it.
    #[must_use]
    pub const fn centre_sample(&self) -> usize {
        self.centre_sample
    }

    /// The magnitude spectrum, from direct current to Nyquist inclusive.
    #[must_use]
    pub const fn magnitude(&self) -> &[f64] {
        self.magnitude
    }

    /// The number of bins.
    #[must_use]
    pub const fn bins(&self) -> usize {
        self.magnitude.len()
    }

    /// The centre frequency of a bin, in hertz.
    #[must_use]
    pub fn bin_frequency(&self, bin: usize) -> f64 {
        f64::from(self.sample_rate.hz()) * ratio(bin, self.window_size)
    }

    /// The bin whose centre is nearest a frequency.
    ///
    /// Saturates at Nyquist rather than wrapping, so a caller asking for a
    /// frequency above the representable range gets the top bin instead of a
    /// silently aliased one.
    #[must_use]
    pub fn bin_for_frequency(&self, hertz: f64) -> usize {
        if !hertz.is_finite() || hertz <= 0.0 {
            return 0;
        }
        let per_bin = f64::from(self.sample_rate.hz()) / count_to_f64(self.window_size);
        if per_bin <= 0.0 {
            return 0;
        }
        let index = crate::num::round_to_count(hertz / per_bin);
        index.min(self.bins().saturating_sub(1))
    }

    /// The sample rate the frame was analysed at.
    #[must_use]
    pub const fn sample_rate(&self) -> SampleRate {
        self.sample_rate
    }
}

/// A configured short-time Fourier transform.
///
/// Holds every buffer the analysis needs, sized once at construction. Running
/// it over a track allocates nothing.
#[derive(Debug)]
pub struct Stft {
    window: Window,
    fft: RealFft,
    hop: usize,
    block: Vec<f64>,
    spectrum: Vec<Complex>,
    magnitude: Vec<f64>,
}

impl Stft {
    /// Builds the transform used for onsets, tempo and beats.
    ///
    /// There is no general-purpose constructor because there is no
    /// general-purpose setting. Time resolution and frequency resolution trade
    /// against each other, and the two jobs this crate does want opposite ends
    /// of that trade. Naming the constructors after the jobs means the choice
    /// is made by requirement rather than by whoever last edited a default.
    ///
    /// # Errors
    ///
    /// Propagates any error from [`Stft::with_settings`], which these settings
    /// cannot trigger.
    pub fn for_rhythm() -> Result<Self, AnalysisError> {
        Self::with_settings(WindowShape::Hann, RHYTHM_WINDOW_SIZE, RHYTHM_HOP_SIZE)
    }

    /// Builds the transform used for chroma, key and spectral balance.
    ///
    /// # Errors
    ///
    /// Propagates any error from [`Stft::with_settings`], which these settings
    /// cannot trigger.
    pub fn for_tone() -> Result<Self, AnalysisError> {
        Self::with_settings(
            WindowShape::BlackmanHarris,
            TONAL_WINDOW_SIZE,
            TONAL_HOP_SIZE,
        )
    }

    /// Builds a transform with an explicit window size and hop.
    ///
    /// # Errors
    ///
    /// Returns [`AnalysisError::SizeNotPowerOfTwo`] or
    /// [`AnalysisError::SizeTooSmall`] for an unusable window, and
    /// [`AnalysisError::HopOutOfRange`] for a hop of zero or one larger than
    /// the window.
    pub fn with_settings(
        shape: WindowShape,
        window_size: usize,
        hop: usize,
    ) -> Result<Self, AnalysisError> {
        if hop == 0 || hop > window_size {
            return Err(AnalysisError::HopOutOfRange {
                hop,
                window: window_size,
            });
        }
        let fft = RealFft::new(window_size)?;
        let window = Window::new(shape, window_size)?;
        let bins = fft.bins();
        Ok(Self {
            window,
            fft,
            hop,
            block: vec![0.0; window_size],
            spectrum: vec![Complex::ZERO; bins],
            magnitude: vec![0.0; bins],
        })
    }

    /// The analysis window size, in samples.
    #[must_use]
    pub fn window_size(&self) -> usize {
        self.window.len()
    }

    /// The hop between successive frames, in samples.
    #[must_use]
    pub const fn hop(&self) -> usize {
        self.hop
    }

    /// The number of spectrum bins each frame carries.
    #[must_use]
    pub const fn bins(&self) -> usize {
        self.fft.bins()
    }

    /// The number of frames a signal of the given length produces.
    ///
    /// Exposed so that consumers can size their curves once rather than
    /// growing a vector while the transform runs.
    #[must_use]
    pub fn frame_count(&self, sample_count: usize) -> usize {
        if sample_count == 0 {
            return 0;
        }
        let mut frames = 0_usize;
        let mut centre = 0_usize;
        while centre < sample_count {
            frames += 1;
            centre += self.hop;
        }
        frames
    }

    /// The rate at which frames are produced, in frames per second.
    #[must_use]
    pub fn frame_rate(&self, sample_rate: SampleRate) -> f64 {
        f64::from(sample_rate.hz()) / count_to_f64(self.hop)
    }

    /// Runs the transform over `samples`, calling `sink` once per frame.
    ///
    /// `samples` is mono. Downmixing is the caller's job because the correct
    /// downmix depends on why the analysis is being run: rhythm analysis wants
    /// the sum, which reinforces centre-panned drums, and stereo-field analysis
    /// wants the difference. Deciding here would quietly impose one of them on
    /// every stage.
    ///
    /// # Centred analysis
    ///
    /// Frame `i` is centred at sample `i × hop`, with the signal treated as
    /// preceded and followed by silence. The alternative — starting the first
    /// window at sample zero — loses every event in the first and last half
    /// window, which for a track that opens on a downbeat means losing the
    /// downbeat. Treating the surrounding silence as silence is also simply
    /// true: a track does begin from nothing.
    ///
    /// An empty signal produces no frames and no error. That is not a failure
    /// to report; the stage above knows the minimum duration it needs and says
    /// so with [`AnalysisError::NotEnoughAudio`].
    ///
    /// # Errors
    ///
    /// Propagates transform errors, which the internal buffer sizing makes
    /// unreachable.
    pub fn analyse<F>(
        &mut self,
        samples: &[f32],
        sample_rate: SampleRate,
        mut sink: F,
    ) -> Result<usize, AnalysisError>
    where
        F: FnMut(&SpectrumFrame<'_>),
    {
        let window_size = self.window.len();
        if samples.is_empty() {
            return Ok(0);
        }
        let half = window_size >> 1;

        let mut index = 0_usize;
        let mut centre = 0_usize;
        while centre < samples.len() {
            // The window spans `centre - half` to `centre + half`. Positions
            // outside the signal read as silence rather than being clamped to
            // the edge sample, because clamping would invent a sustained tone
            // at the boundary and the flux detector would report it as an event.
            for (offset, slot) in self.block.iter_mut().enumerate() {
                let position = centre + offset;
                *slot = if position < half {
                    0.0
                } else {
                    samples
                        .get(position - half)
                        .map_or(0.0, |&sample| f64::from(sample))
                };
            }
            self.window.apply(&mut self.block)?;
            self.fft.forward(&self.block, &mut self.spectrum)?;
            for (slot, value) in self.magnitude.iter_mut().zip(self.spectrum.iter()) {
                *slot = value.magnitude();
            }

            let frame = SpectrumFrame {
                index,
                centre_sample: centre,
                window_size,
                sample_rate,
                magnitude: &self.magnitude,
            };
            sink(&frame);

            index += 1;
            centre += self.hop;
        }

        Ok(index)
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

    use crate::testing::tone;

    #[test]
    fn frame_count_agrees_with_what_analysis_produces() {
        // The two are computed by different code and used for different
        // purposes — one to size a buffer, one to fill it. A disagreement would
        // show up as a curve with a trailing zero that the tempo estimator
        // reads as a silent bar.
        let mut stft = Stft::with_settings(WindowShape::Hann, 64, 16).expect("valid");
        for length in [0_usize, 1, 15, 16, 17, 63, 64, 65, 80, 200, 1000] {
            let signal = vec![0.0_f32; length];
            let mut produced = 0_usize;
            let returned = stft
                .analyse(&signal, SampleRate::HZ_44100, |_| produced += 1)
                .expect("valid");
            assert_eq!(produced, returned, "length {length}");
            assert_eq!(
                produced,
                stft.frame_count(length),
                "predicted count disagrees at length {length}"
            );
        }
    }

    #[test]
    fn a_tone_appears_at_its_own_frequency() {
        let rate = SampleRate::HZ_44100;
        let mut stft = Stft::for_rhythm().expect("valid");
        let signal = tone(441.0, 8192, 1.0, rate);

        let mut peak_bin = 0_usize;
        let mut peak_value = 0.0_f64;
        let mut frames = 0_usize;
        stft.analyse(&signal, rate, |frame| {
            frames += 1;
            if frame.index() != 4 {
                return;
            }
            for (bin, &value) in frame.magnitude().iter().enumerate() {
                if value > peak_value {
                    peak_value = value;
                    peak_bin = bin;
                }
            }
        })
        .expect("valid");

        assert!(frames > 4);
        // 441 Hz at 44.1 kHz with a 1024-point window is bin 10.24, so the peak
        // is bin 10.
        assert_eq!(peak_bin, 10, "peak landed in bin {peak_bin}");
    }

    #[test]
    fn bin_frequency_and_bin_for_frequency_are_inverses() {
        let rate = SampleRate::HZ_48000;
        let mut stft = Stft::with_settings(WindowShape::Hann, 1024, 512).expect("valid");
        let signal = vec![0.0_f32; 2048];

        let mut checked = false;
        stft.analyse(&signal, rate, |frame| {
            if checked {
                return;
            }
            checked = true;
            for bin in [0_usize, 1, 17, 200, 512] {
                let hertz = frame.bin_frequency(bin);
                assert_eq!(
                    frame.bin_for_frequency(hertz),
                    bin,
                    "bin {bin} at {hertz} Hz"
                );
            }
            // Above Nyquist the answer saturates rather than aliasing.
            assert_eq!(frame.bin_for_frequency(1e9), frame.bins() - 1);
            assert_eq!(frame.bin_for_frequency(-5.0), 0);
            assert_eq!(frame.bin_for_frequency(f64::NAN), 0);
        })
        .expect("valid");
        assert!(checked, "no frame was produced");
    }

    #[test]
    fn frames_are_centred_on_multiples_of_the_hop() {
        // The consequence of centred analysis, and the reason a novelty curve
        // index maps to a sample position by multiplication alone. An
        // implementation that started the first window at sample zero would put
        // every frame half a window late and every detected onset with it.
        let rate = SampleRate::HZ_44100;
        let mut stft = Stft::with_settings(WindowShape::Hann, 64, 16).expect("valid");
        let signal = vec![0.0_f32; 256];

        let mut seen = Vec::new();
        stft.analyse(&signal, rate, |frame| seen.push(frame.centre_sample()))
            .expect("valid");

        assert_eq!(seen.first().copied(), Some(0));
        assert_eq!(seen.get(1).copied(), Some(16));
        assert_eq!(seen.get(2).copied(), Some(32));
    }

    #[test]
    fn an_event_at_the_very_start_is_visible_in_the_first_frame() {
        // What centring buys. A track that opens on a downbeat has its loudest
        // moment at sample zero; an analysis starting its first window there
        // would attribute that moment to half a window later, and a track
        // shorter than one window would produce no frames at all.
        let rate = SampleRate::HZ_44100;
        let mut stft = Stft::with_settings(WindowShape::Hann, 64, 16).expect("valid");
        let mut signal = vec![0.0_f32; 64];
        if let Some(slot) = signal.first_mut() {
            *slot = 1.0;
        }

        let mut energies = Vec::new();
        stft.analyse(&signal, rate, |frame| {
            energies.push(frame.magnitude().iter().sum::<f64>());
        })
        .expect("valid");

        let first = energies.first().copied().unwrap_or(0.0);
        assert!(first > 0.0, "the impulse at sample zero produced no energy");
        assert!(
            energies.iter().skip(4).all(|&value| value < first),
            "the impulse was not attributed to the frame centred on it"
        );
    }

    #[test]
    fn a_hop_larger_than_the_window_is_rejected() {
        assert_eq!(
            Stft::with_settings(WindowShape::Hann, 512, 1024).err(),
            Some(AnalysisError::HopOutOfRange {
                hop: 1024,
                window: 512
            })
        );
        assert_eq!(
            Stft::with_settings(WindowShape::Hann, 512, 0).err(),
            Some(AnalysisError::HopOutOfRange {
                hop: 0,
                window: 512
            })
        );
    }
}
