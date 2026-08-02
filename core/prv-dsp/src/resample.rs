//! Reading audio at a rate other than the one it was recorded at.
//!
//! # Three things a DJ means by "change the speed", and which piece does which
//!
//! | What the user wants | What it takes |
//! |---|---|
//! | Faster, and higher — a turntable | [`Resampler`] alone |
//! | Faster, same key — key lock | [`TimeStretch`](crate::TimeStretch) alone |
//! | Same speed, different key | [`PitchShift`] — both, in that order |
//!
//! Key lock needs no resampling at all, which surprises people: stretching
//! *is* the pitch-preserving operation, and adding a resampler to it would undo
//! the thing it just did. The composition only appears in the third row, where
//! the stretch buys back the duration the resampler takes away.
//!
//! # Why the kernel is rebuilt when the rate changes
//!
//! Reading faster than the material was written is decimation, and decimation
//! without a filter folds everything above the new Nyquist back down into the
//! audible band as inharmonic tones. A fixed kernel would either alias when the
//! fader is up or dull the top end when it is not.
//!
//! So the cutoff follows the rate, and [`Resampler::set_rate`] rebuilds when it
//! moves by more than a per cent. Rebuilding evaluates transcendental functions
//! and belongs on the control thread; the audio thread only ever reads the bank.
//! A per cent is small enough to be inaudible and large enough that a fader
//! sweep rebuilds a handful of times rather than once a block.

use prv_rt::AudioBuffer;

use crate::processor::PrepareConfig;
use crate::stretch::TimeStretch;

/// Taps in the interpolation kernel.
///
/// Thirty-three: sixteen samples of support either side. Longer than the
/// limiter's detector because this one is in the signal path — an estimate may
/// be approximate, a sample the listener hears may not.
const TAPS: usize = 33;

/// Half the support.
#[allow(
    clippy::integer_division,
    reason = "the tap count is odd by construction, so this is exact"
)]
const HALF: usize = TAPS / 2;

/// Fractional positions the kernel is precomputed at.
///
/// Two hundred and fifty-six, so the worst rounding of a read position is one
/// five-hundredth of a sample — about forty nanoseconds at 48 kHz, which is
/// below the level at which any modulation of it is audible.
const PHASES: usize = 256;

/// Input the resampler will hold.
const INPUT_CAPACITY: usize = 4096;

/// Output the resampler will hold before it stops producing.
const OUTPUT_CAPACITY: usize = 2048;

/// How far the cutoff may drift before the kernel is rebuilt.
const CUTOFF_TOLERANCE: f64 = 0.01;

/// Reads audio at a different rate than it was written at.
#[derive(Debug)]
pub struct Resampler {
    rate: f64,
    built_for_cutoff: f64,
    channels: usize,

    kernel: Vec<Vec<f32>>,
    input: Vec<Vec<f32>>,
    input_len: usize,
    position: f64,

    output: Vec<Vec<f32>>,
    output_len: usize,
}

impl Default for Resampler {
    fn default() -> Self {
        Self::new()
    }
}

impl Resampler {
    /// The slowest read rate.
    pub const MIN_RATE: f64 = 0.5;

    /// The fastest.
    pub const MAX_RATE: f64 = 2.0;

    /// A resampler that does nothing until it is prepared.
    #[must_use]
    pub fn new() -> Self {
        Self {
            rate: 1.0,
            built_for_cutoff: 0.0,
            channels: 0,
            kernel: Vec::new(),
            input: Vec::new(),
            input_len: 0,
            position: 0.0,
            output: Vec::new(),
            output_len: 0,
        }
    }

    /// Allocates everything the resampler will ever need.
    pub fn prepare(&mut self, config: &PrepareConfig) {
        let channels = config.channels.max(1);
        self.channels = channels;
        self.kernel = vec![vec![0.0; TAPS]; PHASES];
        self.input = vec![vec![0.0; INPUT_CAPACITY]; channels];
        self.output = vec![vec![0.0; OUTPUT_CAPACITY]; channels];
        self.built_for_cutoff = 0.0;
        self.rebuild_kernel();
        self.reset();
    }

    /// Clears everything held without changing the rate.
    pub fn reset(&mut self) {
        for channel in &mut self.input {
            channel.fill(0.0);
        }
        for channel in &mut self.output {
            channel.fill(0.0);
        }
        self.input_len = 0;
        self.output_len = 0;
        // Start reading at the centre of the kernel's support: there is no
        // audio before the first sample, and reading from zero would convolve
        // the opening bar with silence that is not in the material.
        self.position = count_to_f64(HALF);
    }

    /// Sets how many input frames are consumed per output frame.
    ///
    /// Above one is faster and higher; below one is slower and lower. Clamped
    /// rather than refused, for the reason [`TimeStretch::set_ratio`] is.
    ///
    /// Rebuilds the kernel if the anti-aliasing cutoff has moved materially.
    /// Call from the control thread.
    pub fn set_rate(&mut self, rate: f64) {
        self.rate = if rate.is_finite() {
            rate.clamp(Self::MIN_RATE, Self::MAX_RATE)
        } else {
            1.0
        };
        if (self.required_cutoff() - self.built_for_cutoff).abs() > CUTOFF_TOLERANCE {
            self.rebuild_kernel();
        }
    }

    /// The rate in force.
    #[must_use]
    pub const fn rate(&self) -> f64 {
        self.rate
    }

    /// How many input frames the resampler can take right now.
    #[must_use]
    pub const fn writable(&self) -> usize {
        INPUT_CAPACITY - self.input_len
    }

    /// How many output frames are ready.
    #[must_use]
    pub const fn readable(&self) -> usize {
        self.output_len
    }

    /// Takes input, and produces whatever output it can.
    #[must_use]
    pub fn write(&mut self, buffer: &AudioBuffer, frames: usize) -> usize {
        let taken = frames.min(buffer.frames()).min(self.writable());
        if taken == 0 {
            return 0;
        }
        for channel in 0..self.channels {
            let Some(source) = buffer.channel(channel) else {
                continue;
            };
            let Some(destination) = self
                .input
                .get_mut(channel)
                .and_then(|held| held.get_mut(self.input_len..self.input_len + taken))
            else {
                continue;
            };
            for (slot, sample) in destination.iter_mut().zip(source.iter()) {
                *slot = *sample;
            }
        }
        self.input_len += taken;
        self.pump();
        taken
    }

    /// Copies finished output out.
    #[must_use]
    pub fn read(&mut self, buffer: &mut AudioBuffer, frames: usize) -> usize {
        let taken = frames.min(buffer.frames()).min(self.output_len);
        if taken == 0 {
            return 0;
        }
        for channel in 0..self.channels {
            // Borrowed rather than copied out: the first version called
            // `to_vec` here, which allocates — on the audio path, in the one
            // crate whose whole contract forbids it.
            let Some(source) = self.output.get(channel).and_then(|held| held.get(..taken)) else {
                continue;
            };
            let Some(destination) = buffer.channel_mut(channel) else {
                continue;
            };
            for (slot, sample) in destination.iter_mut().zip(source.iter()) {
                *slot = *sample;
            }
        }
        for channel in &mut self.output {
            channel.copy_within(taken.., 0);
        }
        self.output_len -= taken;
        self.pump();
        taken
    }

    /// The cutoff the current rate requires, as a fraction of Nyquist.
    fn required_cutoff(&self) -> f64 {
        // Reading slower than the material was written adds no new content above
        // Nyquist, so nothing needs removing; reading faster folds everything
        // above the new Nyquist down into the band.
        if self.rate > 1.0 {
            1.0 / self.rate
        } else {
            1.0
        }
    }

    /// Rebuilds the interpolation bank for the current cutoff.
    fn rebuild_kernel(&mut self) {
        let cutoff = self.required_cutoff();
        self.built_for_cutoff = cutoff;
        for (phase, taps) in self.kernel.iter_mut().enumerate() {
            let offset = count_to_f64(phase) / count_to_f64(PHASES);
            for (tap, coefficient) in taps.iter_mut().enumerate() {
                let distance = count_to_f64(tap) - count_to_f64(HALF) - offset;
                let value = cutoff * sinc(cutoff * distance) * blackman(tap);
                *coefficient = narrow(value);
            }
        }
    }

    /// Produces as many output frames as the input allows.
    fn pump(&mut self) {
        while self.output_len < OUTPUT_CAPACITY {
            let base = self.position.floor();
            let index = position_to_index(base);
            // The kernel reaches `HALF` samples either side of the read point.
            if index + HALF + 1 > self.input_len || index < HALF {
                break;
            }
            let fraction = self.position - base;
            let phase = phase_index(fraction);
            let Some(taps) = self.kernel.get(phase) else {
                break;
            };
            let start = index - HALF;

            for channel in 0..self.channels {
                let Some(window) = self
                    .input
                    .get(channel)
                    .and_then(|held| held.get(start..start + TAPS))
                else {
                    continue;
                };
                let mut sum = 0.0_f32;
                for (sample, coefficient) in window.iter().zip(taps.iter()) {
                    sum += sample * coefficient;
                }
                if let Some(slot) = self
                    .output
                    .get_mut(channel)
                    .and_then(|held| held.get_mut(self.output_len))
                {
                    *slot = sum;
                }
            }

            self.output_len += 1;
            self.position += self.rate;
            self.compact();
        }
    }

    /// Discards input the read position has passed.
    fn compact(&mut self) {
        let index = position_to_index(self.position.floor());
        let lowest = index.saturating_sub(HALF);
        if lowest == 0 {
            return;
        }
        for channel in &mut self.input {
            channel.copy_within(lowest.., 0);
        }
        self.input_len -= lowest.min(self.input_len);
        self.position -= count_to_f64(lowest);
    }
}

/// Widens a count to a real number.
///
/// Every count converted here is a tap index, a phase index or a sample offset
/// within a buffer of a few thousand — exact in `f64` by many orders of
/// magnitude, so the precision-loss warning describes a case that cannot arise.
#[allow(
    clippy::cast_precision_loss,
    reason = "counts here are bounded far below 2^53, where f64 is exact"
)]
#[inline]
fn count_to_f64(count: usize) -> f64 {
    count as f64
}

/// Narrows a non-negative position to an index.
///
/// The position is compacted below the buffer size on every produced sample, so
/// it never reaches a magnitude an index cannot hold.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the position is non-negative and compacted below the buffer size"
)]
#[inline]
fn position_to_index(position: f64) -> usize {
    if position <= 0.0 {
        0
    } else {
        position as usize
    }
}

/// Which precomputed phase a fractional position rounds to.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the fraction is in [0, 1) and PHASES is small"
)]
fn phase_index(fraction: f64) -> usize {
    let scaled = (fraction * count_to_f64(PHASES)) as usize;
    scaled.min(PHASES - 1)
}

/// The normalised sinc function.
fn sinc(distance: f64) -> f64 {
    if distance.abs() < 1e-12 {
        return 1.0;
    }
    let x = core::f64::consts::PI * distance;
    x.sin() / x
}

/// One point of a Blackman window over the kernel's support.
#[allow(
    clippy::cast_precision_loss,
    reason = "the tap index is a single-digit count"
)]
fn blackman(tap: usize) -> f64 {
    let position = count_to_f64(tap) / (count_to_f64(TAPS) - 1.0);
    let angle = core::f64::consts::TAU * position;
    0.42 - 0.5 * angle.cos() + 0.08 * (2.0 * angle).cos()
}

/// Narrows a designed coefficient to the precision the signal path works in.
#[allow(
    clippy::cast_possible_truncation,
    reason = "coefficients are bounded by one; this narrows precision, not range"
)]
fn narrow(value: f64) -> f32 {
    value as f32
}

/// Changes the key of a track without changing how long it takes.
///
/// Stretch by the pitch ratio, then read back at the same ratio: the stretch
/// makes it longer at the original pitch and the read makes it shorter and
/// higher, and the two durations cancel exactly. Composing them the other way
/// round would work too and would put the resampler's anti-aliasing filter
/// before the splices rather than after, where a splice can put energy back
/// above the cutoff the filter had just removed.
#[derive(Debug)]
pub struct PitchShift {
    stretch: TimeStretch,
    resampler: Resampler,
    bridge: Option<AudioBuffer>,
    semitones: f64,
}

impl Default for PitchShift {
    fn default() -> Self {
        Self::new()
    }
}

impl PitchShift {
    /// The largest shift offered, in semitones either way.
    ///
    /// Six. Beyond that the material stops sounding like itself, and a shift
    /// larger than a tritone is a different record rather than the same one in
    /// another key — which is a thing to choose in the library, not on a fader.
    pub const MAX_SEMITONES: f64 = 6.0;

    /// A shifter that does nothing until it is prepared.
    #[must_use]
    pub fn new() -> Self {
        Self {
            stretch: TimeStretch::new(),
            resampler: Resampler::new(),
            bridge: None,
            semitones: 0.0,
        }
    }

    /// Allocates everything the shifter will ever need.
    ///
    /// # Errors
    ///
    /// Returns the buffer error if the hand-off buffer cannot be allocated,
    /// which means the configuration asked for a shape the engine refuses.
    pub fn prepare(&mut self, config: &PrepareConfig) -> Result<(), prv_rt::BufferError> {
        self.stretch.prepare(config);
        self.resampler.prepare(config);
        self.bridge = Some(AudioBuffer::new(
            config.channels.max(1),
            config.max_block_frames.max(1) as usize,
        )?);
        self.set_semitones(self.semitones);
        Ok(())
    }

    /// Sets the shift, in semitones.
    ///
    /// Clamped to [`Self::MAX_SEMITONES`] either way.
    pub fn set_semitones(&mut self, semitones: f64) {
        let bounded = if semitones.is_finite() {
            semitones.clamp(-Self::MAX_SEMITONES, Self::MAX_SEMITONES)
        } else {
            0.0
        };
        self.semitones = bounded;
        let ratio = 2.0_f64.powf(bounded / 12.0);
        self.stretch.set_ratio(ratio);
        self.resampler.set_rate(ratio);
    }

    /// The shift in force, in semitones.
    #[must_use]
    pub const fn semitones(&self) -> f64 {
        self.semitones
    }

    /// Clears everything held.
    pub fn reset(&mut self) {
        self.stretch.reset();
        self.resampler.reset();
    }

    /// How many input frames the shifter can take right now.
    #[must_use]
    pub const fn writable(&self) -> usize {
        self.stretch.writable()
    }

    /// How many output frames are ready.
    #[must_use]
    pub const fn readable(&self) -> usize {
        self.resampler.readable()
    }

    /// Takes input, and moves whatever it can along the chain.
    #[must_use]
    pub fn write(&mut self, buffer: &AudioBuffer, frames: usize) -> usize {
        let taken = self.stretch.write(buffer, frames);
        self.drain();
        taken
    }

    /// Copies finished output out.
    #[must_use]
    pub fn read(&mut self, buffer: &mut AudioBuffer, frames: usize) -> usize {
        self.drain();
        self.resampler.read(buffer, frames)
    }

    /// Moves everything the stretcher has produced into the resampler.
    fn drain(&mut self) {
        let Some(bridge) = self.bridge.as_mut() else {
            return;
        };
        loop {
            let room = self.resampler.writable().min(bridge.frames());
            if room == 0 {
                break;
            }
            let produced = self.stretch.read(bridge, room);
            if produced == 0 {
                break;
            }
            let accepted = self.resampler.write(bridge, produced);
            if accepted < produced {
                // Cannot happen: `room` was taken from the resampler's own
                // capacity a moment ago and nothing else writes to it. Breaking
                // rather than looping keeps a future change from spinning here.
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::integer_division,
        reason = "test fixtures index their own buffers and build signals from indices"
    )]

    use super::*;
    use prv_time::SampleRate;

    const RATE: u32 = 48_000;
    const BLOCK: usize = 256;

    fn config(channels: usize) -> PrepareConfig {
        PrepareConfig::new(
            SampleRate::new(RATE).expect("a valid rate"),
            BLOCK as u32,
            channels,
        )
    }

    fn sine(frequency: f32, frames: usize) -> Vec<f32> {
        (0..frames)
            .map(|index| {
                (index as f32 / RATE as f32 * frequency * core::f32::consts::TAU).sin() * 0.5
            })
            .collect()
    }

    fn frequency_of(signal: &[f32]) -> f32 {
        let mut crossings = 0_u32;
        let mut first = None;
        let mut last = 0;
        for index in 1..signal.len() {
            if signal[index - 1] <= 0.0 && signal[index] > 0.0 {
                if first.is_none() {
                    first = Some(index);
                }
                last = index;
                crossings += 1;
            }
        }
        let Some(first) = first else { return 0.0 };
        if crossings < 2 || last <= first {
            return 0.0;
        }
        (crossings - 1) as f32 * RATE as f32 / (last - first) as f32
    }

    fn through_resampler(rate: f64, input: &[f32]) -> Vec<f32> {
        let mut resampler = Resampler::new();
        resampler.prepare(&config(1));
        resampler.set_rate(rate);

        let mut in_buffer = AudioBuffer::new(1, BLOCK).expect("a buffer");
        let mut out_buffer = AudioBuffer::new(1, BLOCK).expect("a buffer");
        let mut output = Vec::new();
        let mut offset = 0;

        while offset < input.len() {
            let frames = BLOCK.min(input.len() - offset);
            in_buffer.clear();
            let data = in_buffer.channel_mut(0).expect("a channel");
            for (slot, sample) in data.iter_mut().zip(input[offset..offset + frames].iter()) {
                *slot = *sample;
            }
            let taken = resampler.write(&in_buffer, frames);
            offset += taken;
            loop {
                let produced = resampler.read(&mut out_buffer, BLOCK);
                if produced == 0 {
                    break;
                }
                output.extend_from_slice(&out_buffer.channel(0).expect("a channel")[..produced]);
            }
            assert!(
                taken > 0,
                "the resampler accepted nothing and produced nothing"
            );
        }
        output
    }

    #[test]
    fn reading_faster_raises_the_pitch_and_shortens_the_material() {
        // The turntable behaviour, which is a feature and not a defect: it is
        // what a DJ gets with key lock off.
        let input = sine(440.0, RATE as usize / 2);

        let faster = through_resampler(1.25, &input);
        let measured = frequency_of(&faster[faster.len() / 4..]);
        assert!(
            (measured - 550.0).abs() < 8.0,
            "reading at 1.25 gave {measured} Hz, expected 550"
        );
        let expected = input.len() as f64 / 1.25;
        assert!(
            (faster.len() as f64 - expected).abs() / expected < 0.05,
            "reading at 1.25 gave {} frames, expected about {expected:.0}",
            faster.len()
        );
    }

    #[test]
    fn reading_slower_lowers_the_pitch_and_lengthens_the_material() {
        let input = sine(440.0, RATE as usize / 2);
        let slower = through_resampler(0.8, &input);

        let measured = frequency_of(&slower[slower.len() / 4..]);
        assert!(
            (measured - 352.0).abs() < 8.0,
            "reading at 0.8 gave {measured} Hz, expected 352"
        );
        let expected = input.len() as f64 / 0.8;
        assert!(
            (slower.len() as f64 - expected).abs() / expected < 0.05,
            "reading at 0.8 gave {} frames, expected about {expected:.0}",
            slower.len()
        );
    }

    #[test]
    fn a_rate_of_one_passes_the_material_through() {
        // Not bit-identical — the kernel is a filter and a filter has a
        // response — but the level and the pitch must be untouched, because a
        // deck at zero pitch is the reference everything else is judged
        // against.
        let input = sine(1000.0, RATE as usize / 4);
        let output = through_resampler(1.0, &input);

        let settled = &output[TAPS * 4..output.len() - TAPS * 4];
        let peak = settled.iter().fold(0.0_f32, |worst, s| worst.max(s.abs()));
        assert!(
            (peak - 0.5).abs() < 0.03,
            "a rate of one changed the level to {peak}"
        );
        let measured = frequency_of(settled);
        assert!(
            (measured - 1000.0).abs() < 10.0,
            "a rate of one moved the pitch to {measured} Hz"
        );
    }

    #[test]
    fn speeding_up_removes_what_would_otherwise_fold_back() {
        // Decimation without a filter folds everything above the new Nyquist
        // down into the audible band as inharmonic tones. A tone above the new
        // Nyquist must come out quiet, not come out somewhere else.
        //
        // At a rate of 2 the new Nyquist is 12 kHz; 18 kHz would alias to 6 kHz.
        let input = sine(18_000.0, RATE as usize / 4);
        let output = through_resampler(2.0, &input);

        let settled = &output[TAPS * 4..output.len() - TAPS * 4];
        let peak = settled.iter().fold(0.0_f32, |worst, s| worst.max(s.abs()));
        assert!(
            peak < 0.1,
            "a tone above the new Nyquist survived at {peak}; it has folded back"
        );
    }

    #[test]
    fn the_kernel_is_rebuilt_only_when_the_cutoff_moves() {
        // Rebuilding evaluates transcendental functions. Doing it per block
        // would put that cost on a fader movement; never doing it would alias.
        let mut resampler = Resampler::new();
        resampler.prepare(&config(1));

        // Below one, nothing needs removing, so every rate shares one kernel.
        resampler.set_rate(0.9);
        let at_nine = resampler.built_for_cutoff;
        resampler.set_rate(0.6);
        assert!((resampler.built_for_cutoff - at_nine).abs() < 1e-12);

        // Above one, the cutoff follows the rate.
        resampler.set_rate(2.0);
        assert!((resampler.built_for_cutoff - 0.5).abs() < 1e-12);
    }

    #[test]
    fn a_rate_outside_what_the_resampler_can_do_is_brought_back() {
        let mut resampler = Resampler::new();
        resampler.prepare(&config(1));

        resampler.set_rate(9.0);
        assert!((resampler.rate() - Resampler::MAX_RATE).abs() < 1e-9);
        resampler.set_rate(0.01);
        assert!((resampler.rate() - Resampler::MIN_RATE).abs() < 1e-9);
        resampler.set_rate(f64::INFINITY);
        assert!((resampler.rate() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn silence_in_produces_silence_out() {
        let output = through_resampler(1.3, &vec![0.0_f32; RATE as usize / 8]);
        assert!(output.iter().all(|sample| *sample == 0.0));
    }

    #[test]
    fn shifting_the_key_leaves_the_length_alone() {
        // The composition this module exists to get right: the stretch buys
        // back exactly the duration the resampler takes away.
        let mut shift = PitchShift::new();
        shift.prepare(&config(1)).expect("a valid configuration");
        shift.set_semitones(4.0);

        let input = sine(440.0, RATE as usize / 2);
        let mut in_buffer = AudioBuffer::new(1, BLOCK).expect("a buffer");
        let mut out_buffer = AudioBuffer::new(1, BLOCK).expect("a buffer");
        let mut output = Vec::new();
        let mut offset = 0;

        while offset < input.len() {
            let frames = BLOCK.min(input.len() - offset);
            in_buffer.clear();
            let data = in_buffer.channel_mut(0).expect("a channel");
            for (slot, sample) in data.iter_mut().zip(input[offset..offset + frames].iter()) {
                *slot = *sample;
            }
            let taken = shift.write(&in_buffer, frames);
            offset += taken;
            loop {
                let produced = shift.read(&mut out_buffer, BLOCK);
                if produced == 0 {
                    break;
                }
                output.extend_from_slice(&out_buffer.channel(0).expect("a channel")[..produced]);
            }
            assert!(
                taken > 0,
                "the shifter accepted nothing and produced nothing"
            );
        }

        let expected = input.len() as f64;
        assert!(
            (output.len() as f64 - expected).abs() / expected < 0.08,
            "a four-semitone shift changed the length: {} frames against {expected:.0}",
            output.len()
        );

        // Four semitones is a ratio of 2^(4/12) = 1.2599, so 440 Hz becomes
        // about 554 Hz — the C sharp above the A.
        let measured = frequency_of(&output[output.len() / 4..]);
        assert!(
            (measured - 554.0).abs() < 12.0,
            "a four-semitone shift gave {measured} Hz, expected about 554"
        );
    }

    #[test]
    fn no_shift_is_the_identity_of_the_pair() {
        let mut shift = PitchShift::new();
        shift.prepare(&config(1)).expect("a valid configuration");
        assert!((shift.semitones() - 0.0).abs() < 1e-12);
        shift.set_semitones(0.0);

        // Both halves must be at unity, or "no shift" would be two operations
        // that happen to cancel rather than two that do nothing.
        assert!((shift.stretch.ratio() - 1.0).abs() < 1e-12);
        assert!((shift.resampler.rate() - 1.0).abs() < 1e-12);
    }

    #[test]
    fn a_shift_larger_than_a_tritone_is_brought_back() {
        // Beyond that the material stops sounding like itself, and a shift that
        // large is a different record rather than the same one in another key.
        let mut shift = PitchShift::new();
        shift.prepare(&config(1)).expect("a valid configuration");

        shift.set_semitones(24.0);
        assert!((shift.semitones() - PitchShift::MAX_SEMITONES).abs() < 1e-9);
        shift.set_semitones(-24.0);
        assert!((shift.semitones() + PitchShift::MAX_SEMITONES).abs() < 1e-9);
        shift.set_semitones(f64::NAN);
        assert!((shift.semitones() - 0.0).abs() < 1e-9);
    }
}
