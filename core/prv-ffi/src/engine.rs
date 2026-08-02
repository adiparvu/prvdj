//! The object a host holds, and everything it can be asked.
//!
//! # One handle, not twelve
//!
//! The architecture overview requires calls across the language boundary to be
//! *coarse-grained*. The obvious alternative — a handle per subsystem, so a host
//! juggles a transport, a project, a renderer and a clock — moves the job of
//! keeping them consistent to the side of the boundary least able to do it, in a
//! language with no access to the invariants that say what consistent means.
//!
//! So there is one handle. Inside it the subsystems are exactly the crates they
//! always were; outside it there is one thing to create, one thing to destroy,
//! and no way to hold a transport that belongs to a project that has been
//! dropped.
//!
//! # Why the source is a callback rather than a buffer
//!
//! [`prv_render::Source`] is the port through which the renderer asks for audio,
//! and only the platform can open a file. Across C that is a function pointer
//! and an opaque context, registered once and called from the audio thread.
//!
//! The alternative — the host pushes audio in and the core pulls from a queue —
//! sounds safer and is worse. The renderer reads *the frames it needs, in the
//! order it needs them*, and a set is not always played forwards; a push model
//! forces the host to predict seeks it cannot see, and the first thing it gets
//! wrong is the scratch after a jump.

use prv_project::{DeviceId, OperationPayload, PlacementId, ProjectState, TrackRef};
use prv_render::{RenderReport, Renderer, Source};
use prv_rt::AudioBuffer;
use prv_time::{Frames, SampleRate, Tempo, TimeSignature};
use prv_transport::{Transport, TransportEvent};

use crate::status::Status;

/// How many channels the engine will accept.
///
/// Two is what the product mixes in and the limit exists so a mistaken argument
/// cannot ask for an allocation measured in gigabytes. Raising it is a minor ABI
/// change and costs nothing but a decision.
pub const MAX_CHANNELS: u32 = 8;

/// The largest block the engine will prepare for.
///
/// A host asking for more than this has almost certainly passed a byte count
/// where a frame count belongs, which is a mistake worth failing on rather than
/// honouring.
pub const MAX_BLOCK_FRAMES: u32 = 65_536;

/// Reads audio for one track.
///
/// Called from the audio thread, so it must obey the same contract the rest of
/// that thread does: no allocation, no locking, no file access that can block.
/// A host that cannot satisfy a read from memory it already holds returns fewer
/// frames than asked for, and the renderer records the shortfall rather than
/// stalling.
///
/// `planar` points at `channels * capacity` floats, channel-major: channel 0's
/// frames first, then channel 1's. Write `frames` frames beginning at
/// `destination` within each channel. Return how many were actually written.
pub type ReadAudio = unsafe extern "C" fn(
    user_data: *mut core::ffi::c_void,
    track: u64,
    source_offset: i64,
    planar: *mut f32,
    channels: u32,
    capacity: u32,
    destination: u32,
    frames: u32,
) -> u32;

/// The host's audio source, as it crosses the boundary.
///
/// `user_data` is opaque to the core and is handed back untouched. The core
/// never dereferences it, never copies what it points at, and never frees it;
/// its lifetime is entirely the host's business, and the host must keep it alive
/// until the engine is destroyed or another source replaces it.
struct HostSource {
    read: Option<ReadAudio>,
    user_data: *mut core::ffi::c_void,
}

impl HostSource {
    const fn none() -> Self {
        Self {
            read: None,
            user_data: core::ptr::null_mut(),
        }
    }
}

impl Source for HostSource {
    fn read(
        &mut self,
        track: TrackRef,
        offset: Frames,
        into: &mut AudioBuffer,
        destination: usize,
        frames: usize,
    ) -> usize {
        let Some(read) = self.read else {
            // No source registered yet. Silence is the honest answer, and the
            // renderer already reports a short read as an incomplete placement.
            return 0;
        };

        let channels = u32::try_from(into.channels()).unwrap_or(0);
        let capacity = u32::try_from(into.frames()).unwrap_or(0);
        let destination = u32::try_from(destination).unwrap_or(u32::MAX);
        let wanted = u32::try_from(frames).unwrap_or(0);
        if channels == 0 || capacity == 0 || wanted == 0 || destination >= capacity {
            return 0;
        }

        let planar = into.as_mut_slice().as_mut_ptr();

        // SAFETY: `planar` points at exactly `channels * capacity` floats, which
        // is what `AudioBuffer` guarantees for its own storage and what the
        // arguments describe. The callback is the host's, registered through
        // `prv_engine_set_source`, whose documented contract is that it writes
        // only within those bounds. Calling it is the one thing this crate
        // cannot verify, and it is the reason the boundary is audited.
        let got = unsafe {
            read(
                self.user_data,
                track.get(),
                offset.get(),
                planar,
                channels,
                capacity,
                destination,
                wanted,
            )
        };

        // A host that reports writing more than it was asked for has a defect.
        // Believing it would tell the renderer that frames it never wrote are
        // audio, so the claim is clamped rather than trusted.
        usize::try_from(got.min(wanted)).unwrap_or(0)
    }
}

/// Everything a host holds.
///
/// Not `#[repr(C)]`: the host never sees inside it. Keeping the layout Rust's
/// own is what lets the fields change without touching the ABI, which is the
/// entire point of an opaque handle.
pub struct Engine {
    transport: Transport,
    log: prv_project::OperationLog,
    state: ProjectState,
    renderer: Renderer,
    /// The engine's own output block.
    ///
    /// The renderer needs an [`AudioBuffer`], and an `AudioBuffer` owns its
    /// storage — it cannot be made to borrow a host's memory. So the engine
    /// renders into its own and copies out. The copy is a `memcpy` of one
    /// block: no allocation, no locking, and bounded by the block size, which
    /// is what the realtime contract actually asks for.
    output: AudioBuffer,
    source: HostSource,
    report: RenderReport,
    device: DeviceId,
    channels: usize,
    block_frames: usize,
    sample_rate: SampleRate,
    next_placement: u64,
    /// Set once `prepare` has run, because rendering before it would have no
    /// scratch buffer and would silently produce nothing.
    prepared: bool,
}

impl Engine {
    /// Builds an engine at a rate and block size.
    ///
    /// # Errors
    ///
    /// [`Status::InvalidArgument`] if the shape is one the engine refuses, which
    /// is checked here rather than at the entry point so the bounds live beside
    /// the thing they bound.
    pub fn new(sample_rate: u32, channels: u32, max_block_frames: u32) -> Result<Self, Status> {
        let Ok(sample_rate) = SampleRate::new(sample_rate) else {
            return Err(Status::InvalidArgument);
        };
        if channels == 0 || channels > MAX_CHANNELS {
            return Err(Status::InvalidArgument);
        }
        if max_block_frames == 0 || max_block_frames > MAX_BLOCK_FRAMES {
            return Err(Status::InvalidArgument);
        }
        let channels = usize::try_from(channels).unwrap_or(2);
        let block = usize::try_from(max_block_frames).unwrap_or(1024);

        let state = ProjectState::new();
        let mut renderer = Renderer::new();
        if renderer.prepare(&state, channels, block).is_err() {
            return Err(Status::InvalidArgument);
        }
        let Ok(output) = AudioBuffer::new(channels, block) else {
            return Err(Status::InvalidArgument);
        };

        Ok(Self {
            transport: Transport::new(sample_rate, Tempo::BPM_120, TimeSignature::FOUR_FOUR),
            log: prv_project::OperationLog::new(),
            state,
            renderer,
            output,
            source: HostSource::none(),
            report: RenderReport::new(),
            device: DeviceId::new(1),
            channels,
            block_frames: block,
            sample_rate,
            next_placement: 1,
            prepared: true,
        })
    }

    /// Registers where audio comes from.
    pub fn set_source(&mut self, read: Option<ReadAudio>, user_data: *mut core::ffi::c_void) {
        self.source = HostSource { read, user_data };
    }

    /// Applies a transport event.
    ///
    /// # Errors
    ///
    /// [`Status::InvalidState`] when the transition is not one the machine
    /// defines. Returned rather than ignored, because a refused transition is
    /// either a defect or a race and swallowing it hides both.
    pub fn transport(&mut self, event: TransportEvent) -> Result<(), Status> {
        self.transport
            .apply(event)
            .map(|_| ())
            .map_err(|_| Status::InvalidState)
    }

    /// Moves the playhead.
    pub fn seek(&mut self, position: i64) {
        self.transport.seek(Frames::new(position));
    }

    /// The transport's current position in frames.
    #[must_use]
    pub fn position(&self) -> i64 {
        self.transport.snapshot().position.get()
    }

    /// The transport's playback state, as its ABI code.
    #[must_use]
    pub fn playback_state(&self) -> i32 {
        crate::mapping::playback_code(self.transport.state())
    }

    /// Whether audio is being rendered.
    #[must_use]
    pub fn is_playing(&self) -> bool {
        self.transport.state().is_rendering()
    }

    /// Appends a placement and refolds the project.
    ///
    /// # Errors
    ///
    /// [`Status::Refused`] if the log would not accept the operation, which
    /// happens when an identity has already been used — a retried delivery, or a
    /// host that reused a number.
    pub fn place_track(
        &mut self,
        track: u64,
        position: i64,
        length: i64,
        source_offset: i64,
        lane: u32,
        timestamp_micros: i64,
    ) -> Result<u64, Status> {
        if length <= 0 {
            return Err(Status::InvalidArgument);
        }
        let placement = PlacementId::new(self.next_placement);

        let mut payloads = vec![OperationPayload::PlaceTrack {
            placement,
            track: TrackRef::new(track),
            position: Frames::new(position),
            length: Frames::new(length),
            lane,
        }];
        if source_offset != 0 {
            payloads.push(OperationPayload::SetPlacementSource {
                placement,
                source_offset: Frames::new(source_offset),
            });
        }

        for operation in self.log.author_all(self.device, timestamp_micros, payloads) {
            self.log.append(operation).map_err(|_| Status::Refused)?;
        }

        self.next_placement = self.next_placement.saturating_add(1);
        self.refold()?;
        Ok(placement.get())
    }

    /// How long the project is, in frames.
    #[must_use]
    pub fn duration(&self) -> i64 {
        self.state.duration().get()
    }

    /// How many placements the project holds.
    #[must_use]
    pub fn placement_count(&self) -> u64 {
        self.state.placements.len().try_into().unwrap_or(u64::MAX)
    }

    /// Applies a planned set to this project.
    ///
    /// # A generated mix is an ordinary edit
    ///
    /// The plan becomes operations on the log — placements, automation, tempo
    /// changes — which is what `prv-mix::render` produces and what a hand-made
    /// edit produces. Master Prompt #3B requires the user to be able to edit
    /// everything the system decides, and this is what makes that true by
    /// construction: after this call there is nothing in the document that says
    /// which placements a person made and which the planner did.
    ///
    /// # Errors
    ///
    /// [`Status::InvalidState`] when nothing has been planned, and
    /// [`Status::Refused`] when the log will not accept an operation.
    pub fn apply_plan(
        &mut self,
        planner: &crate::planning::Planner,
        timestamp_micros: i64,
    ) -> Result<(), Status> {
        let payloads = planner.operations(self.sample_rate, self.next_placement)?;
        if payloads.is_empty() {
            return Err(Status::InvalidState);
        }

        // The planner allocated placement identities starting from this
        // engine's next one, so the counter advances past every identity it
        // used. Counting the placements rather than the payloads is deliberate:
        // a render emits automation and tempo changes too, and treating those
        // as identities would leave gaps that look like deleted clips.
        let used = payloads
            .iter()
            .filter(|payload| matches!(payload, OperationPayload::PlaceTrack { .. }))
            .count();

        for operation in self.log.author_all(self.device, timestamp_micros, payloads) {
            self.log.append(operation).map_err(|_| Status::Refused)?;
        }
        self.next_placement = self
            .next_placement
            .saturating_add(used.try_into().unwrap_or(0));

        self.refold()
    }

    /// Rebuilds the materialised state and the renderer's automation.
    fn refold(&mut self) -> Result<(), Status> {
        self.state = self.log.state();
        // The scratch buffer's shape has not changed, so this cannot fail for a
        // reason the caller can act on — but it is checked rather than assumed,
        // because "cannot fail" is a claim with a poor record.
        self.renderer
            .prepare(&self.state, self.channels, self.block_frames)
            .map_err(|_| Status::InvalidArgument)
    }

    /// Renders one block into a caller-owned planar slice.
    ///
    /// # The realtime contract, restated at the boundary
    ///
    /// This is the only call in the crate that runs on the audio thread. It
    /// allocates nothing, locks nothing and waits for nothing; every buffer it
    /// touches was allocated by `new`, which runs on another thread.
    ///
    /// # Errors
    ///
    /// [`Status::InvalidState`] if the engine was never prepared, and
    /// [`Status::InvalidArgument`] if the block is larger than the one the
    /// engine was built for, or has a different channel count. Both are refused
    /// rather than clamped: a host that asks for 512 frames and receives 256
    /// would play the difference as whatever was in its buffer before.
    pub fn render_into(
        &mut self,
        samples: &mut [f32],
        channels: usize,
        frames: usize,
    ) -> Result<(), Status> {
        if !self.prepared {
            return Err(Status::InvalidState);
        }
        if channels != self.channels || frames > self.block_frames {
            return Err(Status::InvalidArgument);
        }
        let needed = channels
            .checked_mul(frames)
            .ok_or(Status::InvalidArgument)?;
        if samples.len() < needed {
            return Err(Status::BufferTooSmall);
        }

        let position = Frames::new(self.transport.snapshot().position.get());
        self.renderer.render(
            &self.state,
            position,
            frames,
            &mut self.output,
            &mut self.source,
            &mut self.report,
        );

        // Channel-major, one channel at a time. The engine's buffer is sized to
        // the maximum block, so its rows are `block_frames` apart while the
        // host's are `frames` apart — copying the whole slab would interleave
        // the wrong silence into every channel after the first.
        for channel in 0..channels {
            let Some(rendered) = self.output.channel(channel) else {
                return Err(Status::InvalidState);
            };
            let from: &[f32] = rendered.get(..frames).ok_or(Status::InvalidState)?;
            let start = channel.checked_mul(frames).ok_or(Status::InvalidArgument)?;
            let end = start.checked_add(frames).ok_or(Status::InvalidArgument)?;
            let into = samples.get_mut(start..end).ok_or(Status::BufferTooSmall)?;
            into.copy_from_slice(from);
        }

        if self.transport.state().is_rendering() {
            self.transport.advance(u32::try_from(frames).unwrap_or(0));
        }
        Ok(())
    }

    /// Whether every placement the last render touched was read in full.
    #[must_use]
    pub fn render_was_complete(&self) -> bool {
        self.report.is_complete()
    }
}

impl core::fmt::Debug for Engine {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        // Written by hand because `HostSource` holds a raw pointer belonging to
        // the host, and printing a foreign address in a log is both useless and
        // the kind of thing that ends up in a crash report.
        f.debug_struct("Engine")
            .field("position", &self.transport.snapshot().position)
            .field("state", &self.transport.state())
            .field("placements", &self.state.placements.len())
            .field("has_source", &self.source.read.is_some())
            .finish_non_exhaustive()
    }
}
