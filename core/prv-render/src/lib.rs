//! Turning a project into audio.
//!
//! # The core cannot read a file, so the caller hands it one
//!
//! ADR-0001 keeps input and output outside the core, and a renderer's whole job
//! is to read audio. The resolution is [`Source`]: a port the platform layer
//! implements, which the renderer asks for frames from a track at an offset. The
//! decisions — which placement contributes, from where in its media, at what
//! level — are all here and all testable, against a source that generates a tone
//! rather than opens a file.
//!
//! # Two properties matter more than the rest
//!
//! **The result does not depend on the block size.** Rendering a project in
//! blocks of 64 and in blocks of 1024 must produce identical samples. If it does
//! not, an export does not match what the user monitored, and the difference
//! appears only once — in the file they have already sent to somebody. It is the
//! defect this module is arranged around: every position is computed from the
//! project's own timeline rather than accumulated across calls.
//!
//! **A source that could not be read is reported, not silently silent.** A
//! missing file renders as silence either way; the difference is whether anybody
//! is told. Master Prompt #3C requires an export to know whether everything it
//! needed was there, and `prv-export`'s manifest is where that ends up — but it
//! can only record what the renderer noticed.
//!
//! # What this does not do
//!
//! No effects, no plugins, no master chain. A placement contributes its audio at
//! its automated level and nothing else. Effects belong to a graph whose nodes
//! are plugins, and the plugin runtime is outside the core; wiring `prv-dsp`'s
//! chain in here before that exists would be guessing at the shape of something
//! that does not yet have one.

use prv_project::{ParameterKey, ParameterOwner, PlacementId, ProjectState, TrackRef};
use prv_rt::AudioBuffer;
use prv_time::Frames;
use prv_timeline::Timeline;

/// Where the renderer gets audio from.
///
/// Implemented by the platform layer, which is the only thing that can open a
/// file. The renderer never learns what a track *is*.
pub trait Source {
    /// Reads frames of a track into a buffer, beginning `offset` frames into it.
    ///
    /// Writes into the first `frames` positions of `into`, starting at
    /// `destination`. Returns how many frames were written: fewer than asked for
    /// means the media ended or could not be read, and the renderer treats the
    /// difference as silence and records that it happened.
    ///
    /// The implementation must not assume the offsets it is given are
    /// increasing. A renderer seeks, and a set is not always played forwards.
    fn read(
        &mut self,
        track: TrackRef,
        offset: Frames,
        into: &mut AudioBuffer,
        destination: usize,
        frames: usize,
    ) -> usize;
}

/// What happened while rendering one block.
///
/// Accumulated across a whole render and handed to `prv-export`, which turns it
/// into the part of a manifest that says whether everything the mix needed was
/// there.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RenderReport {
    incomplete: Vec<PlacementId>,
    frames_rendered: u64,
}

impl RenderReport {
    /// Nothing rendered yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The placements whose media could not be read in full.
    ///
    /// In placement order, each named once however many blocks it failed in: a
    /// report that listed a missing file four thousand times would be one nobody
    /// reads.
    #[must_use]
    pub fn incomplete(&self) -> &[PlacementId] {
        &self.incomplete
    }

    /// How many frames have been rendered.
    #[must_use]
    pub const fn frames_rendered(&self) -> u64 {
        self.frames_rendered
    }

    /// Whether every placement gave everything it was asked for.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.incomplete.is_empty()
    }

    /// Records that a placement fell short.
    fn note_incomplete(&mut self, placement: PlacementId) {
        if let Err(index) = self.incomplete.binary_search(&placement) {
            self.incomplete.insert(index, placement);
        }
    }
}

/// The parameter a placement's level is automated on.
///
/// `Gain`, which is what `prv-mix::render` writes when it builds a transition.
///
/// Named here rather than invented per call so that the renderer and whatever
/// draws the automation lane are addressing the same thing. A level that a user
/// can see and the renderer cannot find would be the worst kind of silence.
#[must_use]
pub fn level_of(placement: PlacementId) -> Option<prv_project::ParameterAddress> {
    prv_project::ParameterAddress::new(ParameterOwner::Placement(placement), ParameterKey::Gain)
        .ok()
}

/// Widens a count to a frame position.
///
/// Every count converted here is a block length or an offset within one, so it
/// is far below the range a signed frame position holds. Saturating rather than
/// wrapping means a nonsensical input produces a clamped position rather than a
/// negative one, which would read as audio before the start of the set.
#[inline]
fn count_to_frames(count: usize) -> i64 {
    i64::try_from(count).unwrap_or(i64::MAX)
}

/// Renders a project.
#[derive(Debug)]
pub struct Renderer {
    scratch: Option<AudioBuffer>,
    channels: usize,
    automation: Timeline,
}

impl Default for Renderer {
    fn default() -> Self {
        Self::new()
    }
}

impl Renderer {
    /// A renderer that does nothing until it is prepared.
    #[must_use]
    pub fn new() -> Self {
        Self {
            scratch: None,
            channels: 0,
            automation: Timeline::new(),
        }
    }

    /// Allocates the working buffer and reads the project's automation.
    ///
    /// # Why the automation is read here rather than per block
    ///
    /// Evaluating a curve is a binary search; rebuilding the curves from the
    /// operation log is a fold over the whole history. Doing the second once per
    /// block would make a render quadratic in the length of the project — a cost
    /// nobody notices on a test fixture and everybody notices on a four-hour
    /// set. Call this again when the project changes.
    ///
    /// # Errors
    ///
    /// Returns the buffer error if the shape is one the engine refuses.
    pub fn prepare(
        &mut self,
        state: &ProjectState,
        channels: usize,
        max_block_frames: usize,
    ) -> Result<(), prv_rt::BufferError> {
        let channels = channels.max(1);
        self.channels = channels;
        self.scratch = Some(AudioBuffer::new(channels, max_block_frames.max(1))?);
        let (timeline, _) = Timeline::from_project(state);
        self.automation = timeline;
        Ok(())
    }

    /// Renders one block of the project, beginning at `position`.
    ///
    /// The buffer is cleared first: a renderer that summed into whatever the
    /// caller left behind would produce a different mix depending on what was
    /// played before it, which is the same class of defect as depending on the
    /// block size.
    pub fn render(
        &mut self,
        state: &ProjectState,
        position: Frames,
        frames: usize,
        output: &mut AudioBuffer,
        source: &mut dyn Source,
        report: &mut RenderReport,
    ) {
        let frames = frames.min(output.frames());
        output.clear();
        if frames == 0 {
            return;
        }

        let block_start = position.get();
        let block_end = block_start.saturating_add(count_to_frames(frames));

        for (id, placement) in &state.placements {
            let start = placement.position.get();
            let end = start.saturating_add(placement.length.get());
            if end <= block_start || start >= block_end {
                continue;
            }

            // The overlap between this placement and this block, in project
            // frames. Computed from the project's own numbers every time rather
            // than carried between calls: a position accumulated across blocks
            // is a position that depends on how the blocks were cut.
            let from = start.max(block_start);
            let to = end.min(block_end);
            if to <= from {
                continue;
            }
            let count = usize::try_from(to - from).unwrap_or(0);
            let destination = usize::try_from(from - block_start).unwrap_or(0);
            let into_source = placement.source_offset.get().saturating_add(from - start);

            let Some(scratch) = self.scratch.as_mut() else {
                return;
            };
            scratch.clear();
            let got = source.read(
                placement.track,
                Frames::new(into_source),
                scratch,
                0,
                count.min(scratch.frames()),
            );
            if got < count {
                report.note_incomplete(*id);
            }

            // Read before the scratch buffer is borrowed again below. The level
            // comes from the automation the renderer prepared, not from the
            // state, so it is a lookup rather than a fold.
            let lane = level_of(*id).and_then(|address| self.automation.automation_for(&address));

            // Evaluated per sample rather than per block. A level held constant
            // across a block is a staircase, and a staircase in a gain is a
            // click at the block rate — the artefact `prv-rt`'s smoother exists
            // to remove from live controls, and it must not come back in here.
            let scratch = &*scratch;
            for offset in 0..got {
                let at = Frames::new(from.saturating_add(count_to_frames(offset)));
                // One before the first point and after the last, and one where
                // there is no curve at all: an automation lane says how a level
                // *changes*, and the absence of one is not an instruction to be
                // silent.
                let level = lane.and_then(|curve| curve.value_at(at)).unwrap_or(1.0);
                for channel in 0..self.channels.min(output.channels()) {
                    let sample = scratch
                        .channel(channel)
                        .and_then(|data| data.get(offset))
                        .copied()
                        .unwrap_or(0.0);
                    if let Some(slot) = output
                        .channel_mut(channel)
                        .and_then(|data| data.get_mut(destination + offset))
                    {
                        *slot += sample * level;
                    }
                }
            }
        }

        report.frames_rendered = report
            .frames_rendered
            .saturating_add(u64::try_from(frames).unwrap_or(0));
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
        clippy::float_cmp,
        clippy::cast_possible_wrap,
        reason = "test fixtures index their own buffers and build signals from indices"
    )]

    use super::*;
    use prv_project::OperationPayload;

    /// A source that returns a ramp whose value is its absolute offset.
    ///
    /// Every sample says where in the media it came from, so a test can read the
    /// output and see exactly which part of which track was placed where. A sine
    /// would be prettier and would hide an offset error of a whole period.
    struct Ramp {
        available: i64,
    }

    impl Source for Ramp {
        fn read(
            &mut self,
            _track: TrackRef,
            offset: Frames,
            into: &mut AudioBuffer,
            destination: usize,
            frames: usize,
        ) -> usize {
            let start = offset.get();
            let possible = (self.available - start).max(0);
            let count = frames.min(usize::try_from(possible).unwrap_or(0));
            for channel in 0..into.channels() {
                let Some(data) = into
                    .channel_mut(channel)
                    .and_then(|slice| slice.get_mut(destination..destination + count))
                else {
                    continue;
                };
                for (index, slot) in data.iter_mut().enumerate() {
                    *slot = (start + index as i64) as f32;
                }
            }
            count
        }
    }

    fn project(placements: &[(u64, i64, i64, i64)]) -> ProjectState {
        let mut state = ProjectState::new();
        for (id, position, length, source_offset) in placements {
            state.apply(&OperationPayload::PlaceTrack {
                placement: PlacementId::new(*id),
                track: TrackRef::new(*id),
                position: Frames::new(*position),
                length: Frames::new(*length),
                lane: 0,
            });
            if *source_offset != 0 {
                state.apply(&OperationPayload::SetPlacementSource {
                    placement: PlacementId::new(*id),
                    source_offset: Frames::new(*source_offset),
                });
            }
        }
        state
    }

    /// Renders a whole project in blocks of a given size.
    fn render_all(state: &ProjectState, total: usize, block: usize) -> (Vec<f32>, RenderReport) {
        let mut renderer = Renderer::new();
        renderer.prepare(state, 1, block).expect("a valid shape");
        let mut output = AudioBuffer::new(1, block).expect("a buffer");
        let mut source = Ramp {
            available: 1_000_000,
        };
        let mut report = RenderReport::new();
        let mut collected = Vec::with_capacity(total);

        let mut position = 0_i64;
        while (position as usize) < total {
            let frames = block.min(total - position as usize);
            renderer.render(
                state,
                Frames::new(position),
                frames,
                &mut output,
                &mut source,
                &mut report,
            );
            collected.extend_from_slice(&output.channel(0).expect("a channel")[..frames]);
            position += frames as i64;
        }
        (collected, report)
    }

    #[test]
    fn the_result_does_not_depend_on_the_block_size() {
        // The defect this module is arranged around: if it did, an export would
        // not match what the user monitored, and the difference would appear
        // only in the file they had already sent to somebody.
        let state = project(&[(1, 0, 500, 0), (2, 300, 700, 1_000)]);

        let (reference, _) = render_all(&state, 1200, 1024);
        for block in [1_usize, 7, 64, 128, 333, 1024] {
            let (rendered, _) = render_all(&state, 1200, block);
            assert_eq!(
                rendered.len(),
                reference.len(),
                "block size {block} changed the length"
            );
            for (index, (a, b)) in reference.iter().zip(rendered.iter()).enumerate() {
                assert_eq!(
                    a, b,
                    "block size {block} changed sample {index}: {a} against {b}"
                );
            }
        }
    }

    #[test]
    fn a_placement_contributes_only_where_it_sits() {
        let state = project(&[(1, 100, 50, 0)]);
        let (rendered, _) = render_all(&state, 300, 64);

        // Compared against the exact value rather than against "not silence":
        // the ramp's first sample *is* zero, and a test that read that as
        // silence would be testing the fixture rather than the renderer.
        for (index, sample) in rendered.iter().enumerate() {
            let expected = if (100..150).contains(&index) {
                (index - 100) as f32
            } else {
                0.0
            };
            assert_eq!(
                *sample, expected,
                "sample {index} is {sample}, expected {expected}"
            );
        }
    }

    #[test]
    fn the_source_offset_says_which_part_of_the_media_is_heard() {
        // The ramp's value is its absolute offset in the media, so the rendered
        // samples say exactly which part was placed where. This is the property
        // Sprint 13 added the offset for, checked from the other end.
        let state = project(&[(1, 100, 50, 96_000)]);
        let (rendered, _) = render_all(&state, 200, 64);

        assert_eq!(rendered[100], 96_000.0);
        assert_eq!(rendered[101], 96_001.0);
        assert_eq!(rendered[149], 96_049.0);
        assert_eq!(rendered[150], 0.0);
    }

    #[test]
    fn overlapping_placements_sum() {
        // Two records playing together is the whole product. If they did not
        // sum, every transition would be a cut.
        let state = project(&[(1, 0, 100, 0), (2, 50, 100, 0)]);
        let (rendered, _) = render_all(&state, 200, 32);

        // Where only the first plays, the sample is its own ramp value.
        assert_eq!(rendered[10], 10.0);
        // Where both play, the samples add.
        assert_eq!(rendered[60], 60.0 + 10.0);
        // Where only the second plays.
        assert_eq!(rendered[120], 70.0);
    }

    #[test]
    fn a_source_that_falls_short_is_reported_rather_than_silently_silent() {
        // A missing file renders as silence either way; the difference is
        // whether anybody is told. `prv-export`'s manifest can only record what
        // the renderer noticed.
        let state = project(&[(1, 0, 400, 0)]);

        let mut renderer = Renderer::new();
        renderer.prepare(&state, 1, 128).expect("a valid shape");
        let mut output = AudioBuffer::new(1, 128).expect("a buffer");
        // The media runs out after 200 frames.
        let mut source = Ramp { available: 200 };
        let mut report = RenderReport::new();

        for block in 0..4_i64 {
            renderer.render(
                &state,
                Frames::new(block * 128),
                128,
                &mut output,
                &mut source,
                &mut report,
            );
        }

        assert!(!report.is_complete());
        assert_eq!(
            report.incomplete(),
            &[PlacementId::new(1)],
            "a short read was not reported, or was reported once per block"
        );
        assert_eq!(report.frames_rendered(), 512);
    }

    #[test]
    fn a_complete_render_says_so() {
        let state = project(&[(1, 0, 400, 0)]);
        let (_, report) = render_all(&state, 512, 128);
        assert!(report.is_complete());
        assert!(report.incomplete().is_empty());
    }

    #[test]
    fn the_buffer_is_cleared_before_anything_is_summed_into_it() {
        // A renderer that summed into whatever the caller left behind would
        // produce a different mix depending on what was played before it — the
        // same class of defect as depending on the block size.
        let state = project(&[(1, 0, 10, 0)]);
        let mut renderer = Renderer::new();
        renderer.prepare(&state, 1, 64).expect("a valid shape");

        let mut output = AudioBuffer::new(1, 64).expect("a buffer");
        if let Some(data) = output.channel_mut(0) {
            data.fill(999.0);
        }
        let mut source = Ramp { available: 1_000 };
        let mut report = RenderReport::new();
        renderer.render(
            &state,
            Frames::new(0),
            64,
            &mut output,
            &mut source,
            &mut report,
        );

        let produced = output.channel(0).expect("a channel");
        assert_eq!(produced[0], 0.0, "the first sample of the placement");
        assert_eq!(produced[9], 9.0);
        assert_eq!(produced[10], 0.0, "the caller's leftovers survived");
    }

    #[test]
    fn an_empty_project_renders_silence_and_says_nothing_is_missing() {
        let state = ProjectState::new();
        let (rendered, report) = render_all(&state, 256, 64);
        assert!(rendered.iter().all(|sample| *sample == 0.0));
        assert!(report.is_complete());
    }

    #[test]
    fn rendering_the_same_block_twice_gives_the_same_samples() {
        // Determinism, checked directly. A renderer that carried state between
        // calls would pass every test above and fail this one.
        let state = project(&[(1, 0, 500, 1_000), (2, 200, 500, 0)]);
        let mut renderer = Renderer::new();
        renderer.prepare(&state, 1, 128).expect("a valid shape");
        let mut source = Ramp {
            available: 1_000_000,
        };
        let mut report = RenderReport::new();

        let mut first = AudioBuffer::new(1, 128).expect("a buffer");
        renderer.render(
            &state,
            Frames::new(256),
            128,
            &mut first,
            &mut source,
            &mut report,
        );
        let first: Vec<f32> = first.channel(0).expect("a channel").to_vec();

        let mut second = AudioBuffer::new(1, 128).expect("a buffer");
        renderer.render(
            &state,
            Frames::new(256),
            128,
            &mut second,
            &mut source,
            &mut report,
        );
        let second = second.channel(0).expect("a channel");

        assert_eq!(first.as_slice(), second);
    }
}
