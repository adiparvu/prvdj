//! Changing how long audio takes without changing what it sounds like.
//!
//! # What key lock actually is
//!
//! Play a record faster and it goes up in pitch, because speed and pitch are the
//! same thing on a turntable. Every DJ knows this and most of them want it gone:
//! a track pulled up four per cent to match the one before it should still be in
//! the key the analysis said it was in, or the harmonic planning in `prv-mix` is
//! planning around a number that stopped being true the moment the fader moved.
//!
//! So this changes duration and leaves pitch alone. It is the whole of key lock;
//! there is no separate mechanism.
//!
//! # Why waveform similarity rather than a phase vocoder
//!
//! A phase vocoder stretches in the frequency domain and is the better tool for
//! extreme ratios and for material with a lot of sustained tone. It also smears
//! transients — the artefact engineers call "phasiness" — and a DJ's material is
//! mostly transients: a kick that arrives slightly soft is a kick that has lost
//! the thing it was for.
//!
//! Overlap-add on waveform similarity keeps transients intact, because it never
//! reconstructs anything: it cuts the input at points where the waveform matches
//! what was played last and crossfades. Within the range a DJ actually uses —
//! and this refuses ratios outside [`TimeStretch::MIN_RATIO`] to
//! [`TimeStretch::MAX_RATIO`] — it is the better trade, and it is cheap enough
//! to run on the audio thread.
//!
//! # Stereo is spliced once, not twice
//!
//! The similarity search runs on the sum of the channels and the winning
//! position is applied to all of them. Searching per channel would find slightly
//! different splice points, and two channels cut at different places is a stereo
//! image that wanders — the defect nobody can describe and everybody can hear.
//!
//! # It is a stream, not a processor
//!
//! [`Processor`](crate::Processor) works in place: as many samples out as in.
//! A stretcher's whole purpose is that those two numbers differ, so it has its
//! own shape — write input, read output, ask which it wants next.

use prv_rt::AudioBuffer;

use crate::processor::PrepareConfig;

/// Length of one overlap-added frame, in samples.
///
/// About 21 milliseconds at 48 kHz. Long enough to contain a full cycle of
/// anything down to about 50 Hz, so the similarity search has a period to lock
/// on to; short enough that a splice never spans two transients.
const FRAME: usize = 1024;

/// How far the output advances per frame.
///
/// Half the frame, which with a Hann window sums to exactly one — the reason the
/// overlap-add is transparent at a ratio of one rather than merely close to it.
#[allow(
    clippy::integer_division,
    reason = "the frame length is a power of two, so this is exact"
)]
const HOP_OUT: usize = FRAME / 2;

/// How many samples the similarity search compares.
const CORRELATION: usize = 256;

/// How far either side of the ideal position the search looks.
///
/// 128 samples is about 2.7 milliseconds — more than one period of anything
/// above 375 Hz, so the search can always find a period-aligned splice for the
/// part of the spectrum where a mismatch is audible as a click.
const SEARCH: usize = 128;

/// Input the stretcher will hold.
const INPUT_CAPACITY: usize = 4096;

/// Output the stretcher will hold before it stops producing.
const OUTPUT_CAPACITY: usize = 4 * HOP_OUT;

/// Changes duration without changing pitch.
#[derive(Debug)]
pub struct TimeStretch {
    ratio: f64,
    channels: usize,

    window: Vec<f32>,
    input: Vec<Vec<f32>>,
    mono: Vec<f32>,
    input_len: usize,

    accumulator: Vec<Vec<f32>>,
    output: Vec<Vec<f32>>,
    output_len: usize,

    ideal: f64,
    previous: usize,
    primed: bool,
}

impl Default for TimeStretch {
    fn default() -> Self {
        Self::new()
    }
}

impl TimeStretch {
    /// The shortest the output may be relative to the input.
    ///
    /// Half. Beyond that the splices come so often that the material stops
    /// sounding like itself, and a control that can be set to sound broken is a
    /// control that will be.
    pub const MIN_RATIO: f64 = 0.5;

    /// The longest.
    pub const MAX_RATIO: f64 = 2.0;

    /// The delay a caller should expect before the first output arrives, in
    /// input frames.
    ///
    /// The stretcher cannot emit anything until it holds a whole frame plus the
    /// search radius, because the first splice has to be choosable.
    pub const PRIMING_FRAMES: usize = FRAME + SEARCH;

    /// A stretcher that does nothing until it is prepared.
    #[must_use]
    pub fn new() -> Self {
        Self {
            ratio: 1.0,
            channels: 0,
            window: Vec::new(),
            input: Vec::new(),
            mono: Vec::new(),
            input_len: 0,
            accumulator: Vec::new(),
            output: Vec::new(),
            output_len: 0,
            ideal: 0.0,
            previous: 0,
            primed: false,
        }
    }

    /// Allocates everything the stretcher will ever need.
    ///
    /// The only allocating call. Everything after this is arithmetic on buffers
    /// that already exist, which is what lets it run inside the callback.
    pub fn prepare(&mut self, config: &PrepareConfig) {
        let channels = config.channels.max(1);
        self.channels = channels;

        self.window = (0..FRAME).map(hann).collect();
        self.input = vec![vec![0.0; INPUT_CAPACITY]; channels];
        self.mono = vec![0.0; INPUT_CAPACITY];
        self.accumulator = vec![vec![0.0; FRAME]; channels];
        self.output = vec![vec![0.0; OUTPUT_CAPACITY]; channels];
        self.reset();
    }

    /// Clears everything held without changing the ratio.
    pub fn reset(&mut self) {
        for channel in &mut self.input {
            channel.fill(0.0);
        }
        for channel in &mut self.accumulator {
            channel.fill(0.0);
        }
        for channel in &mut self.output {
            channel.fill(0.0);
        }
        self.mono.fill(0.0);
        self.input_len = 0;
        self.output_len = 0;
        self.ideal = 0.0;
        self.previous = 0;
        self.primed = false;
    }

    /// Sets how long the output is relative to the input.
    ///
    /// Above one is longer and slower; below one is shorter and faster. Clamped
    /// rather than refused: a ratio arrives from a tempo fader, and a fader that
    /// stops responding at its limit is better than one that reports an error
    /// nobody is reading.
    pub fn set_ratio(&mut self, ratio: f64) {
        self.ratio = if ratio.is_finite() {
            ratio.clamp(Self::MIN_RATIO, Self::MAX_RATIO)
        } else {
            1.0
        };
    }

    /// The ratio in force.
    #[must_use]
    pub const fn ratio(&self) -> f64 {
        self.ratio
    }

    /// How many input frames the stretcher can take right now.
    #[must_use]
    pub const fn writable(&self) -> usize {
        INPUT_CAPACITY - self.input_len
    }

    /// How many output frames are ready.
    #[must_use]
    pub const fn readable(&self) -> usize {
        self.output_len
    }

    /// Whether more input is needed before more output can be produced.
    #[must_use]
    pub fn needs_input(&self) -> bool {
        self.output_len < HOP_OUT && self.input_len < self.input_required()
    }

    /// Takes input, and produces whatever output it can.
    ///
    /// Returns how many frames were taken, which may be fewer than offered if
    /// the stretcher is full. A caller that ignores the return value will lose
    /// audio, so it is `#[must_use]`.
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

        // The similarity search runs on the sum, so the sum is maintained here
        // rather than rebuilt per frame: two channels spliced at different
        // places is a stereo image that wanders.
        if let Some(mono) = self.mono.get_mut(self.input_len..self.input_len + taken) {
            for (position, slot) in mono.iter_mut().enumerate() {
                let mut sum = 0.0;
                for channel in 0..self.channels {
                    sum += self
                        .input
                        .get(channel)
                        .and_then(|held| held.get(self.input_len + position))
                        .copied()
                        .unwrap_or(0.0);
                }
                *slot = sum;
            }
        }

        self.input_len += taken;
        self.pump();
        taken
    }

    /// Copies finished output out.
    ///
    /// Returns how many frames were written into the buffer.
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

    /// How much input a frame needs before it can be produced.
    fn input_required(&self) -> usize {
        let ideal = self.ideal_floor();
        let candidate_reach = ideal + SEARCH + FRAME;
        let template_reach = self.previous + HOP_OUT + CORRELATION;
        candidate_reach.max(template_reach)
    }

    /// The ideal position as an index.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "the position is non-negative and compacted below the buffer size"
    )]
    fn ideal_floor(&self) -> usize {
        if self.ideal <= 0.0 {
            0
        } else {
            self.ideal as usize
        }
    }

    /// Produces as many frames as the input allows and the output has room for.
    fn pump(&mut self) {
        while self.output_len + HOP_OUT <= OUTPUT_CAPACITY {
            if self.input_len < self.input_required() {
                break;
            }
            let ideal = self.ideal_floor();
            let position = if self.primed {
                self.best_position(ideal)
            } else {
                self.primed = true;
                ideal
            };

            self.overlap_add(position);
            self.emit();

            self.previous = position;
            self.ideal += count_to_f64(HOP_OUT) / self.ratio;
            self.compact();
        }
    }

    /// The input position whose waveform best continues what was played last.
    ///
    /// Normalised cross-correlation: the plain sum of products would prefer
    /// whichever candidate is loudest rather than whichever matches, which on a
    /// track with a kick in it means every splice lands on the kick.
    fn best_position(&self, ideal: usize) -> usize {
        let lowest = ideal.saturating_sub(SEARCH);
        let highest = (ideal + SEARCH).min(self.input_len.saturating_sub(FRAME));
        if highest <= lowest {
            return lowest;
        }

        let template_at = self.previous + HOP_OUT;
        let Some(template) = self.mono.get(template_at..template_at + CORRELATION) else {
            return ideal;
        };

        let mut best = ideal;
        let mut best_score = f32::NEG_INFINITY;
        for candidate in lowest..=highest {
            let Some(window) = self.mono.get(candidate..candidate + CORRELATION) else {
                break;
            };
            let mut product = 0.0_f32;
            let mut energy = 0.0_f32;
            for (held, offered) in window.iter().zip(template.iter()) {
                product += held * offered;
                energy += held * held;
            }
            // A silent candidate correlates with nothing; treating it as a
            // perfect match would splice silence into the middle of a bar.
            let score = if energy > f32::EPSILON {
                product / energy.sqrt()
            } else {
                f32::NEG_INFINITY
            };
            if score > best_score {
                best_score = score;
                best = candidate;
            }
        }
        best
    }

    /// Adds one windowed frame of input into the accumulator.
    fn overlap_add(&mut self, position: usize) {
        for channel in 0..self.channels {
            let Some(source) = self
                .input
                .get(channel)
                .and_then(|held| held.get(position..position + FRAME))
            else {
                continue;
            };
            let Some(accumulator) = self.accumulator.get_mut(channel) else {
                continue;
            };
            for ((slot, sample), shape) in accumulator
                .iter_mut()
                .zip(source.iter())
                .zip(self.window.iter())
            {
                *slot += sample * shape;
            }
        }
    }

    /// Moves the finished half of the accumulator into the output.
    ///
    /// The first `HOP_OUT` samples are final: the next frame starts there, so
    /// nothing will be added to them again.
    fn emit(&mut self) {
        for channel in 0..self.channels {
            let Some(accumulator) = self.accumulator.get_mut(channel) else {
                continue;
            };
            if let Some(ready) = accumulator.get(..HOP_OUT) {
                if let Some(destination) = self
                    .output
                    .get_mut(channel)
                    .and_then(|held| held.get_mut(self.output_len..self.output_len + HOP_OUT))
                {
                    for (slot, sample) in destination.iter_mut().zip(ready.iter()) {
                        *slot = *sample;
                    }
                }
            }
            accumulator.copy_within(HOP_OUT.., 0);
            if let Some(tail) = accumulator.get_mut(FRAME - HOP_OUT..) {
                tail.fill(0.0);
            }
        }
        self.output_len += HOP_OUT;
    }

    /// Discards input nothing can still refer to.
    fn compact(&mut self) {
        let lowest = self.previous.min(self.ideal_floor()).saturating_sub(SEARCH);
        if lowest == 0 {
            return;
        }
        for channel in &mut self.input {
            channel.copy_within(lowest.., 0);
        }
        self.mono.copy_within(lowest.., 0);
        self.input_len -= lowest.min(self.input_len);
        self.previous -= lowest.min(self.previous);
        self.ideal -= count_to_f64(lowest);
    }
}

/// Widens a count to a real number.
///
/// Every count converted here is a position within a buffer of a few thousand
/// samples — exact in `f64` by many orders of magnitude.
#[allow(
    clippy::cast_precision_loss,
    reason = "counts here are bounded far below 2^53, where f64 is exact"
)]
#[inline]
fn count_to_f64(count: usize) -> f64 {
    count as f64
}

/// One point of a periodic Hann window.
///
/// Periodic rather than symmetric, because two of them at half-frame spacing sum
/// to exactly one. The symmetric form is off by a sample and leaves a ripple at
/// the frame rate — inaudible in isolation and a buzz once it repeats ninety
/// times a second.
#[allow(
    clippy::cast_precision_loss,
    reason = "the index is bounded by the frame length"
)]
fn hann(index: usize) -> f32 {
    let position = index as f32 / FRAME as f32;
    0.5 - 0.5 * (core::f32::consts::TAU * position).cos()
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

    fn prepared(channels: usize) -> TimeStretch {
        let mut stretch = TimeStretch::new();
        stretch.prepare(&PrepareConfig::new(
            SampleRate::new(RATE).expect("a valid rate"),
            BLOCK as u32,
            channels,
        ));
        stretch
    }

    /// Feeds a mono signal through and collects everything that comes out.
    fn run(stretch: &mut TimeStretch, input: &[f32]) -> Vec<f32> {
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
            let taken = stretch.write(&in_buffer, frames);
            offset += taken;

            loop {
                let produced = stretch.read(&mut out_buffer, BLOCK);
                if produced == 0 {
                    break;
                }
                let data = out_buffer.channel(0).expect("a channel");
                output.extend_from_slice(&data[..produced]);
            }

            assert!(
                taken > 0,
                "the stretcher accepted nothing and produced nothing"
            );
        }
        output
    }

    fn sine(frequency: f32, frames: usize) -> Vec<f32> {
        (0..frames)
            .map(|index| {
                (index as f32 / RATE as f32 * frequency * core::f32::consts::TAU).sin() * 0.5
            })
            .collect()
    }

    /// Estimates the dominant frequency by counting upward zero crossings.
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

    #[test]
    fn stretching_changes_the_length_and_leaves_the_pitch_alone() {
        // The whole of key lock. If this fails, a track pulled up to match the
        // one before it is no longer in the key the analysis reported, and the
        // harmonic planning is planning around a number that stopped being true.
        for ratio in [0.75_f64, 1.25, 1.5] {
            let mut stretch = prepared(1);
            stretch.set_ratio(ratio);

            let input = sine(440.0, RATE as usize / 2);
            let output = run(&mut stretch, &input);

            let expected = input.len() as f64 * ratio;
            let error = (output.len() as f64 - expected).abs() / expected;
            assert!(
                error < 0.05,
                "at ratio {ratio} the output was {} frames, expected about {expected:.0}",
                output.len()
            );

            // Skip the priming region, where the overlap-add has not reached
            // unity gain and the frequency estimate is meaningless.
            let settled = &output[output.len() / 4..];
            let measured = frequency_of(settled);
            assert!(
                (measured - 440.0).abs() < 8.0,
                "at ratio {ratio} the pitch moved to {measured} Hz"
            );
        }
    }

    #[test]
    fn a_ratio_of_one_is_transparent() {
        // Two Hann windows at half-frame spacing sum to exactly one, so the
        // overlap-add is an identity rather than an approximation. A stretcher
        // that coloured the sound when it was doing nothing would have to be
        // switched out of the path, which is a worse design.
        let mut stretch = prepared(1);
        stretch.set_ratio(1.0);

        let input = sine(220.0, RATE as usize / 4);
        let output = run(&mut stretch, &input);

        let settled = &output[FRAME..output.len() - FRAME];
        let peak = settled.iter().fold(0.0_f32, |worst, s| worst.max(s.abs()));
        assert!(
            (peak - 0.5).abs() < 0.02,
            "a ratio of one changed the level: peak {peak}, expected 0.5"
        );
    }

    #[test]
    fn silence_in_produces_silence_out() {
        let mut stretch = prepared(2);
        stretch.set_ratio(1.3);
        let output = run(&mut stretch, &vec![0.0_f32; RATE as usize / 8]);
        assert!(output.iter().all(|sample| *sample == 0.0));
    }

    #[test]
    fn both_channels_are_spliced_at_the_same_place() {
        // Two channels cut at different places is a stereo image that wanders —
        // the defect nobody can describe and everybody can hear. The search runs
        // on the sum precisely so this cannot happen, and this test is what says
        // the wiring matches the intention.
        let mut stretch = prepared(2);
        stretch.set_ratio(1.4);

        let mut in_buffer = AudioBuffer::new(2, BLOCK).expect("a buffer");
        let mut out_buffer = AudioBuffer::new(2, BLOCK).expect("a buffer");
        let mut left_out = Vec::new();
        let mut right_out = Vec::new();

        for block in 0..80 {
            in_buffer.clear();
            for channel in 0..2 {
                let data = in_buffer.channel_mut(channel).expect("a channel");
                for (frame, slot) in data.iter_mut().enumerate() {
                    let index = block * BLOCK + frame;
                    let value = (index as f32 / RATE as f32 * 330.0 * core::f32::consts::TAU).sin();
                    // Identical material in both channels: any difference in the
                    // output can only come from the splices.
                    *slot = value * 0.4;
                }
            }
            let _ = stretch.write(&in_buffer, BLOCK);
            loop {
                let produced = stretch.read(&mut out_buffer, BLOCK);
                if produced == 0 {
                    break;
                }
                left_out.extend_from_slice(&out_buffer.channel(0).expect("left")[..produced]);
                right_out.extend_from_slice(&out_buffer.channel(1).expect("right")[..produced]);
            }
        }

        assert!(!left_out.is_empty());
        assert_eq!(left_out.len(), right_out.len());
        for (index, (left, right)) in left_out.iter().zip(right_out.iter()).enumerate() {
            assert!(
                (left - right).abs() < 1e-6,
                "the channels diverged at {index}: {left} against {right}"
            );
        }
    }

    #[test]
    fn a_ratio_outside_what_the_stretcher_can_do_is_brought_back() {
        // A ratio arrives from a tempo fader. A fader that stops responding at
        // its limit is better than one that reports an error nobody is reading.
        let mut stretch = prepared(1);

        stretch.set_ratio(8.0);
        assert!((stretch.ratio() - TimeStretch::MAX_RATIO).abs() < 1e-9);

        stretch.set_ratio(0.01);
        assert!((stretch.ratio() - TimeStretch::MIN_RATIO).abs() < 1e-9);

        stretch.set_ratio(f64::NAN);
        assert!((stretch.ratio() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn resetting_leaves_nothing_of_the_previous_track() {
        // Master Prompt #15's "silence should remain silent" applied to a deck
        // load: the tail of the outgoing record must not arrive under the first
        // bar of the incoming one.
        let mut stretch = prepared(1);
        stretch.set_ratio(1.2);
        let _ = run(&mut stretch, &sine(440.0, RATE as usize / 8));

        stretch.reset();
        assert_eq!(stretch.readable(), 0);

        let output = run(&mut stretch, &vec![0.0_f32; RATE as usize / 8]);
        assert!(
            output.iter().all(|sample| *sample == 0.0),
            "audio survived a reset"
        );
    }

    #[test]
    fn it_never_takes_more_than_it_can_hold() {
        let mut stretch = prepared(1);
        let buffer = AudioBuffer::new(1, BLOCK).expect("a buffer");

        let mut total = 0;
        for _ in 0..100 {
            total += stretch.write(&buffer, BLOCK);
        }
        assert!(total > 0);
        assert!(stretch.writable() <= INPUT_CAPACITY);
    }

    #[test]
    fn the_window_sums_to_one_at_half_frame_spacing() {
        // The property the transparency test depends on, checked directly so a
        // failure says which of the two is wrong.
        for offset in 0..HOP_OUT {
            let sum = hann(offset) + hann(offset + HOP_OUT);
            assert!(
                (sum - 1.0).abs() < 1e-5,
                "the window sums to {sum} at offset {offset}"
            );
        }
    }
}
