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

use prv_project::wire;

use crate::status::Status;

/// What a merge did.
///
/// Four counts rather than one, because they mean four different things to a
/// host: work arrived, work was already here, work disagrees, and work could not
/// be read.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SyncReport {
    /// Operations that were new and have been applied.
    pub applied: u64,
    /// Operations already present. Expected, not exceptional.
    pub already_present: u64,
    /// Pairs that disagree and need a person to decide.
    pub conflicts: u64,
    /// Operations this build could not interpret.
    ///
    /// # What this count promises, and what it does not
    ///
    /// A message containing them can be passed on byte for byte — that is what
    /// [`prv_project::wire`] guarantees and what makes a device running an older
    /// build a relay rather than a hole in a fleet.
    ///
    /// What is *not* built is store-and-forward: they are not written into the
    /// log, so a device that merges a message and later derives a new one from
    /// its own log will not re-emit them. Doing that properly means the log
    /// holding operations it cannot fold, and it means being careful about the
    /// version vector — a device that recorded them as seen would be telling
    /// peers it holds work it cannot produce, which is worse than not holding
    /// it.
    ///
    /// So this count exists to be shown to a person. A non-zero value means
    /// "some of this project was made with a newer version of the app", which is
    /// true, actionable, and the honest thing to say.
    pub carried: u64,
}

/// The device an engine is until a host says otherwise.
///
/// Correct for one machine and wrong for a fleet, which is why
/// [`Engine::set_device`] exists and why a host that synchronises must call it.
const DEFAULT_DEVICE: DeviceId = DeviceId::new(1);

/// The first placement identity a device may allocate, exclusive.
///
/// The high half of the number names the device; the low half counts. See
/// [`Engine::allocate_placements`] for why placements are numbered this way and
/// what the arrangement does and does not promise.
fn placement_base(device: DeviceId) -> u64 {
    u64::from(fingerprint(device)) << 32
}

/// A 32-bit fingerprint of a device identity.
///
/// The finalising mix from `splitmix64`, folded in half. A host is free to
/// declare device identities that differ only in their low bits — a sequence, a
/// row identifier, whatever its storage handed it — and taking a fingerprint
/// rather than a slice means such identities still land far apart.
fn fingerprint(device: DeviceId) -> u32 {
    let mut z = device.get();
    z ^= z >> 30;
    z = z.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z ^= z >> 27;
    z = z.wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^= z >> 31;
    #[allow(
        clippy::cast_possible_truncation,
        reason = "folding a mixed 64-bit value into 32 bits is the whole intent; \
                  both halves have contributed to every bit kept"
    )]
    let folded = ((z >> 32) ^ z) as u32;
    folded
}

/// Copies bytes into a caller-owned buffer, or reports the size needed.
///
/// The convention every buffer-returning call at this boundary uses: a caller
/// with nowhere to put the answer still learns how much room to make.
fn copy_out(bytes: &[u8], into: &mut [u8]) -> Result<usize, Status> {
    let needed = bytes.len();
    if into.len() < needed {
        return Ok(needed);
    }
    into.get_mut(..needed)
        .ok_or(Status::BufferTooSmall)?
        .copy_from_slice(bytes);
    Ok(needed)
}

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
    /// The message [`Engine::sync_prepare`] built, waiting to be copied out.
    ///
    /// Held so that sizing and sending do not each encode the project. Kept
    /// after it is taken rather than cleared, so a host whose first buffer was
    /// too small can simply ask again.
    outbound: Vec<u8>,
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
            outbound: Vec::new(),
            device: DEFAULT_DEVICE,
            channels,
            block_frames: block,
            sample_rate,
            next_placement: placement_base(DEFAULT_DEVICE).saturating_add(1),
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
        let placement = PlacementId::new(self.allocate_placements(1)?);

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
        self.allocate_placements(used.try_into().unwrap_or(u64::MAX))?;

        for operation in self.log.author_all(self.device, timestamp_micros, payloads) {
            self.log.append(operation).map_err(|_| Status::Refused)?;
        }

        self.refold()
    }

    /// Declares which device this is.
    ///
    /// # Why the host has to say
    ///
    /// An operation's identity is a device plus a number that device allocates
    /// itself, which is what lets two machines edit the same project offline
    /// without colliding. The device part has to be stable for the life of an
    /// installation and distinct between installations — and both of those are
    /// facts about the machine, which ADR-0001 puts outside the core.
    ///
    /// A host that never calls this gets a default, which is correct for a
    /// single machine and wrong for a fleet. It is not silently wrong: two
    /// devices sharing an identity produce operations with the same name and
    /// different contents, and
    /// [`ConflictKind::SameNameDifferentWork`](prv_project::ConflictKind)
    /// reports exactly that rather than letting one overwrite the other.
    ///
    /// # Errors
    ///
    /// [`Status::InvalidArgument`] for zero, which is what an uninitialised
    /// value looks like, and [`Status::InvalidState`] once the project holds
    /// operations — an identity that changed underneath a log would leave the
    /// numbering of what came before meaning something else.
    pub fn set_device(&mut self, device: u64) -> Result<(), Status> {
        if device == 0 {
            return Err(Status::InvalidArgument);
        }
        if !self.log.is_empty() {
            return Err(Status::InvalidState);
        }
        self.device = DeviceId::new(device);
        self.next_placement = placement_base(self.device).saturating_add(1);
        Ok(())
    }

    /// Reserves a run of placement identities and returns the first.
    ///
    /// # Why this is not simply a counter
    ///
    /// It was, and every engine started at one. Two machines editing the same
    /// project offline therefore both named their first clip `placement:1`,
    /// and merging produced two `PlaceTrack` operations claiming the same
    /// placement. The log reported those as conflicts rather than losing
    /// anything — that part was working — but a user would have met a conflict
    /// for every clip either of them had added, which is the same as having no
    /// conflict detection at all.
    ///
    /// The lesson was already in the codebase.
    /// [`OperationId`](prv_project::OperationId) is a device plus a number that
    /// device allocates itself, precisely so two offline machines cannot
    /// collide. Placement identities now inherit the same idea: the high half
    /// of the number names the device, the low half counts.
    ///
    /// # What that promises, and what it does not
    ///
    /// The device half is a 32-bit fingerprint of the identity the host
    /// declared, so two devices collide only if their fingerprints do. Sixty-
    /// four bits of exactness would need both halves whole, and a placement
    /// identity is a `uint64_t` at the boundary — widening it is a change to a
    /// call that already exists, which the ABI's major version forbids.
    ///
    /// The residual risk is small and bounded by the devices sharing *one*
    /// project: a handful, not a population. And it fails the way the old
    /// scheme failed rather than a new way — as a reported conflict.
    ///
    /// # Errors
    ///
    /// [`Status::Refused`] when a device has allocated every identity in its
    /// half of the space. Refusing is the point: spilling into the next
    /// device's range would reintroduce exactly the collision this prevents,
    /// silently, after four billion clips.
    fn allocate_placements(&mut self, count: u64) -> Result<u64, Status> {
        let base = placement_base(self.device);
        let ceiling = base.saturating_add(u64::from(u32::MAX));
        let first = self.next_placement;
        let last = first.checked_add(count).ok_or(Status::Refused)?;
        if first < base || last > ceiling {
            return Err(Status::Refused);
        }
        self.next_placement = last;
        Ok(first)
    }

    /// What this project has already seen, for a peer to answer.
    ///
    /// The first half of a synchronisation. The core never opens a socket —
    /// ADR-0001 — so it produces bytes and the host carries them.
    ///
    /// Writes nothing and returns the size required when `into` is too small, so
    /// a host can ask with an empty slice and allocate exactly once.
    ///
    /// # Errors
    ///
    /// [`Status::Refused`] if the vector names more devices than the format can
    /// count, which needs four billion of them.
    pub fn sync_state(&self, into: &mut [u8]) -> Result<usize, Status> {
        let bytes = wire::encode_vector(self.log.version_vector()).map_err(|_| Status::Refused)?;
        copy_out(&bytes, into)
    }

    /// Prepares the operations a peer has not seen, and reports their size.
    ///
    /// Held rather than returned, so that a host learns the exact size before it
    /// allocates and the message is encoded once rather than once per attempt.
    /// [`Engine::sync_outbound`] then copies it out, and may be called again if
    /// the first buffer was too small.
    ///
    /// # Errors
    ///
    /// [`Status::InvalidArgument`] if `peer_state` is not a version vector this
    /// build can read, and [`Status::Refused`] if the project will not fit a
    /// single message.
    pub fn sync_prepare(&mut self, peer_state: &[u8]) -> Result<usize, Status> {
        let peer = wire::decode_vector(peer_state).map_err(|_| Status::InvalidArgument)?;
        let operations = self.log.operations_since(&peer);
        self.outbound = wire::encode(&operations).map_err(|_| Status::Refused)?;
        Ok(self.outbound.len())
    }

    /// Copies out what [`Engine::sync_prepare`] built.
    ///
    /// # Errors
    ///
    /// None beyond reporting the size required, which the caller sees as
    /// [`Status::BufferTooSmall`] at the boundary.
    pub fn sync_outbound(&self, into: &mut [u8]) -> Result<usize, Status> {
        copy_out(&self.outbound, into)
    }

    /// Merges what a peer sent.
    ///
    /// Operations already present are skipped rather than refused: overlap is
    /// the normal case, because a device sends whatever the other side might not
    /// have. Operations this build cannot interpret are counted and not applied;
    /// see [`SyncReport::carried`] for what that means and what it does not.
    ///
    /// # Errors
    ///
    /// [`Status::InvalidArgument`] if the bytes are not a message this build
    /// could have been sent, and [`Status::Refused`] if the log will not accept
    /// an operation.
    pub fn sync_merge(&mut self, bytes: &[u8]) -> Result<SyncReport, Status> {
        let message = wire::decode(bytes).map_err(|_| Status::InvalidArgument)?;
        let carried = message.unrecognised_count();
        let report = self
            .log
            .merge(&message.to_operations())
            .map_err(|_| Status::Refused)?;
        self.refold()?;
        Ok(SyncReport {
            applied: report.applied.try_into().unwrap_or(u64::MAX),
            already_present: report.already_present.try_into().unwrap_or(u64::MAX),
            conflicts: report.conflicts.len().try_into().unwrap_or(u64::MAX),
            carried: carried.try_into().unwrap_or(u64::MAX),
        })
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

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        reason = "a test that cannot build its own fixture should fail loudly"
    )]

    use super::*;

    fn engine(device: u64) -> Engine {
        let mut engine = Engine::new(48_000, 2, 512).expect("an engine");
        engine.set_device(device).expect("an identity");
        engine
    }

    #[test]
    fn two_devices_never_name_the_same_placement() {
        // The defect this scheme replaced: every engine counted from one, so
        // two machines editing offline both called their first clip
        // `placement:1`. Merging then produced two operations claiming the same
        // placement — reported as a conflict rather than lost, but a conflict
        // for every clip either of them had added.
        let mut here = engine(1);
        let mut there = engine(2);

        let mine: Vec<u64> = (0..16)
            .map(|index| {
                here.place_track(1, index * 4_096, 4_096, 0, 0, 1_000)
                    .expect("places")
            })
            .collect();
        let theirs: Vec<u64> = (0..16)
            .map(|index| {
                there
                    .place_track(1, index * 4_096, 4_096, 0, 0, 1_000)
                    .expect("places")
            })
            .collect();

        for identity in &mine {
            assert!(
                !theirs.contains(identity),
                "two devices both named a placement {identity}"
            );
        }
    }

    #[test]
    fn a_device_that_has_used_its_whole_range_is_refused_rather_than_spilling() {
        // Spilling into the next device's half of the space would reintroduce
        // the collision silently, four billion clips in, which is the worst
        // possible time to find out.
        let mut engine = engine(1);
        let base = placement_base(engine.device);

        engine.next_placement = base.saturating_add(u64::from(u32::MAX));
        assert_eq!(
            engine.place_track(1, 0, 4_096, 0, 0, 1_000).err(),
            Some(Status::Refused)
        );

        // And one short of the end still works, so the bound is where it says.
        engine.next_placement = base.saturating_add(u64::from(u32::MAX)).saturating_sub(1);
        assert!(engine.place_track(1, 0, 4_096, 0, 0, 1_000).is_ok());
    }

    #[test]
    fn declaring_an_identity_renumbers_before_anything_is_named() {
        // The order matters: a host that set its identity after placing a clip
        // would have one clip in the default device's range and the rest in its
        // own, which is the collision again for exactly one clip.
        let mut engine = Engine::new(48_000, 2, 512).expect("an engine");
        engine.set_device(9).expect("an identity");
        let placement = engine
            .place_track(1, 0, 4_096, 0, 0, 1_000)
            .expect("places");

        assert!(placement > placement_base(DeviceId::new(9)));
        assert!(placement <= placement_base(DeviceId::new(9)) + u64::from(u32::MAX));
        assert_eq!(engine.set_device(10).err(), Some(Status::InvalidState));
    }

    #[test]
    fn nearby_device_identities_land_far_apart() {
        // A host is free to hand out identities that differ only in their low
        // bits. Slicing the number rather than mixing it would put those
        // devices side by side, so the first would reach the second's range
        // after a single clip.
        let bases: Vec<u64> = (1..=8)
            .map(|id| placement_base(DeviceId::new(id)))
            .collect();
        for (index, base) in bases.iter().enumerate() {
            for (other, candidate) in bases.iter().enumerate() {
                assert!(
                    index == other || base != candidate,
                    "two nearby identities share a range"
                );
            }
        }
    }

    #[test]
    fn both_sides_of_an_exchange_end_up_with_everything() {
        let mut here = engine(1);
        let mut there = engine(2);
        for index in 0..3 {
            here.place_track(1, index * 4_096, 4_096, 0, 0, 1_000)
                .expect("places");
        }
        for index in 0..2 {
            there
                .place_track(1, index * 4_096, 4_096, 0, 0, 1_000)
                .expect("places");
        }

        let exchange = |from: &mut Engine, to: &Engine| -> Vec<u8> {
            let mut state = vec![0_u8; 512];
            let needed = to.sync_state(&mut state).expect("state");
            let size = from.sync_prepare(&state[..needed]).expect("prepares");
            let mut message = vec![0_u8; size];
            from.sync_outbound(&mut message).expect("copies");
            message
        };

        let outbound = exchange(&mut here, &there);
        let inbound = exchange(&mut there, &here);

        let arriving = there.sync_merge(&outbound).expect("merges");
        let returning = here.sync_merge(&inbound).expect("merges");
        assert_eq!(arriving.conflicts, 0, "an exchange conflicted");
        assert_eq!(returning.conflicts, 0, "an exchange conflicted");
        assert_eq!(here.placement_count(), 5);
        assert_eq!(there.placement_count(), 5);
    }

    #[test]
    fn a_project_synchronises_with_itself_without_conflict() {
        // The whole path, in the crate that owns the boundary: state out,
        // message back, merge, and the two projects agree.
        let mut here = engine(1);
        let mut there = engine(2);
        for index in 0..4 {
            here.place_track(1, index * 4_096, 4_096, 0, 0, 1_000)
                .expect("places");
        }

        let mut state = vec![0_u8; 256];
        let needed = there.sync_state(&mut state).expect("state");
        assert!(needed <= state.len());

        let size = here.sync_prepare(&state[..needed]).expect("prepares");
        let mut message = vec![0_u8; size];
        assert_eq!(here.sync_outbound(&mut message).expect("copies"), size);

        let report = there.sync_merge(&message).expect("merges");
        assert_eq!(report.applied, 4);
        assert_eq!(report.conflicts, 0);
        assert_eq!(report.carried, 0);
        assert_eq!(there.placement_count(), here.placement_count());
        assert_eq!(there.duration(), here.duration());
    }
}
