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
use prv_project::VersionVector;

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
    /// Operations this build could not interpret, and kept anyway.
    ///
    /// They are written into the log, so they survive being closed and reopened
    /// and are sent on to the next peer byte for byte. A device running an older
    /// build is a relay rather than a hole in a fleet, and stays one.
    ///
    /// What they cannot do is take part in the project: they are not folded into
    /// a timeline and they cannot be checked for conflicts, because this build
    /// does not know what they touch. That decision moves to whichever build
    /// understands both sides.
    ///
    /// A non-zero value means "part of this project was made with a newer
    /// version of the app" — true, actionable, and worth saying. It stops being
    /// true after an upgrade and a call to
    /// [`Engine::promote_carried`].
    pub carried: u64,
}

/// One clip on the timeline, as it crosses the boundary.
///
/// Six numbers rather than a handle: a placement is a value, it is small, and a
/// host redrawing a timeline wants a great many of them without a call each.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Placement {
    /// Its identity, stable for as long as it exists.
    pub id: u64,
    /// The library track it plays.
    pub track: u64,
    /// Where it starts, in frames.
    pub position: i64,
    /// How long it plays for.
    pub length: i64,
    /// Which lane it sits on.
    pub lane: u32,
    /// How far into its media it begins.
    pub source_offset: i64,
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

    /// Moves a clip, as an operation on the log.
    ///
    /// # Why every edit goes through the log
    ///
    /// ADR-0003 makes the project *be* its log, so an edit that changed the
    /// materialised state directly would be invisible to undo, to version
    /// history, to comparison and to synchronisation — four features that are
    /// consequences of one mechanism rather than four things to remember.
    ///
    /// # Errors
    ///
    /// [`Status::InvalidArgument`] for a clip the project does not hold, and
    /// [`Status::Refused`] when the log will not accept the operation.
    pub fn move_placement(
        &mut self,
        placement: u64,
        position: i64,
        lane: u32,
        timestamp_micros: i64,
    ) -> Result<(), Status> {
        self.edit(
            placement,
            OperationPayload::MovePlacement {
                placement: PlacementId::new(placement),
                position: Frames::new(position),
                lane,
            },
            timestamp_micros,
        )
    }

    /// Changes how long a clip plays for.
    ///
    /// # Errors
    ///
    /// As [`Engine::move_placement`], and [`Status::InvalidArgument`] for a
    /// length that is not positive — a clip of no length is a clip that cannot
    /// be found again to fix.
    pub fn trim_placement(
        &mut self,
        placement: u64,
        length: i64,
        timestamp_micros: i64,
    ) -> Result<(), Status> {
        if length <= 0 {
            return Err(Status::InvalidArgument);
        }
        self.edit(
            placement,
            OperationPayload::TrimPlacement {
                placement: PlacementId::new(placement),
                length: Frames::new(length),
            },
            timestamp_micros,
        )
    }

    /// Takes a clip off the timeline.
    ///
    /// Not a deletion of anything: the operation that placed it is still in the
    /// log, so undo restores it and the history still says what happened.
    ///
    /// # Errors
    ///
    /// As [`Engine::move_placement`].
    pub fn remove_placement(
        &mut self,
        placement: u64,
        timestamp_micros: i64,
    ) -> Result<(), Status> {
        self.edit(
            placement,
            OperationPayload::RemovePlacement {
                placement: PlacementId::new(placement),
            },
            timestamp_micros,
        )
    }

    /// Authors one edit against an existing clip.
    fn edit(
        &mut self,
        placement: u64,
        payload: OperationPayload,
        timestamp_micros: i64,
    ) -> Result<(), Status> {
        // Checked here rather than left to the fold, because an operation
        // naming a clip that does not exist would be appended, synchronised and
        // silently ignored on every device that received it.
        if !self
            .state
            .placements
            .contains_key(&PlacementId::new(placement))
        {
            return Err(Status::InvalidArgument);
        }

        let operation = self.log.author(self.device, timestamp_micros, payload);
        self.log.append(operation).map_err(|_| Status::Refused)?;
        self.refold()
    }

    /// Undoes this device's last edit, if that can be done without discarding
    /// somebody else's.
    ///
    /// Returns how many operations the undo took — zero when there is nothing
    /// to undo *and* zero when another device has since changed the same thing,
    /// which [`Engine::undo_is_available`] tells apart.
    ///
    /// # Why refusing is the right answer rather than a limitation
    ///
    /// In a shared project, "undo my last edit" can collide with somebody
    /// else's later one. If I move a clip and you then move it again, the
    /// inverse of *my* move puts the clip back where it was before either of us
    /// touched it — discarding your work without saying so. Master Prompt #24
    /// forbids that, and there is no correct silent answer.
    ///
    /// # Errors
    ///
    /// [`Status::Refused`] when the log will not accept the inverse.
    pub fn undo(&mut self, timestamp_micros: i64) -> Result<u64, Status> {
        let undo = self.log.undo_for(self.device, timestamp_micros);
        let operations = undo.operations().to_vec();
        if operations.is_empty() {
            return Ok(0);
        }
        let count = operations.len();
        for operation in operations {
            self.log.append(operation).map_err(|_| Status::Refused)?;
        }
        self.refold()?;
        Ok(count.try_into().unwrap_or(u64::MAX))
    }

    /// Why undo would do nothing, when it would.
    ///
    /// Zero when there is something to undo; one when there is nothing; two
    /// when another device changed the same thing afterwards; three when the
    /// core gave a reason this build has no name for. A host shows the second
    /// and third differently, because "nothing to undo" is a disabled button
    /// and "somebody else moved this" is a sentence.
    #[must_use]
    pub fn undo_is_available(&self, timestamp_micros: i64) -> i32 {
        match self.log.undo_for(self.device, timestamp_micros) {
            prv_project::Undo::Operations(_) => 0,
            prv_project::Undo::Nothing => 1,
            prv_project::Undo::Superseded { .. } => 2,
            // `Undo` is `non_exhaustive`, so this arm cannot be removed. It gets
            // a number of its own rather than borrowing "nothing to undo":
            // those are different answers, and a host that showed a reason it
            // could not name as "nothing to undo" would be telling a small lie
            // about why a control is disabled.
            _ => 3,
        }
    }

    /// One placement of the project, by position in identity order.
    ///
    /// # Why a host needs these and not just a count
    ///
    /// A timeline is drawn from them. Until now the boundary reported how many
    /// placements a project held and how long the whole thing was, which is
    /// enough to say "four tracks, thirty-eight minutes" and not enough to draw
    /// a single clip.
    ///
    /// Ordered by identity rather than by position, and deliberately: identity
    /// order is stable while a user drags a clip, and a list that reordered
    /// itself under the hand doing the dragging is why timelines flicker. A host
    /// that wants time order sorts them, which it can do because it has the
    /// positions.
    ///
    /// # Errors
    ///
    /// [`Status::InvalidArgument`] for an index past the end.
    pub fn placement(&self, index: u64) -> Result<Placement, Status> {
        let index = usize::try_from(index).map_err(|_| Status::InvalidArgument)?;
        let (id, placement) = self
            .state
            .placements
            .iter()
            .nth(index)
            .ok_or(Status::InvalidArgument)?;
        Ok(Placement {
            id: id.get(),
            track: placement.track.get(),
            position: placement.position.get(),
            length: placement.length.get(),
            lane: placement.lane,
            source_offset: placement.source_offset.get(),
        })
    }

    /// How many operations the project's history holds.
    ///
    /// A host shows it as the length of the history. It is also the honest way
    /// to check that a refused edit did not reach the log — a boundary that
    /// reported success and appended nothing, or reported failure and appended
    /// anyway, would be a nastier defect than either alone.
    #[must_use]
    pub fn log_length(&self) -> u64 {
        self.log.len().try_into().unwrap_or(u64::MAX)
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

    /// Moves the placement allocator past every identity this device has
    /// already spent, according to the log.
    ///
    /// # The defect this exists to prevent
    ///
    /// Placement identities are allocated from a counter that lives in memory.
    /// Operations arrive from elsewhere — a peer, or a project file, which is
    /// the same thing — carrying placements *this same device* allocated in an
    /// earlier session. The counter knew nothing about them.
    ///
    /// So opening a saved project and adding one track re-used an identity the
    /// project already held, and the new `PlaceTrack` overwrote an existing clip
    /// in the fold. A track vanished, quietly, on the most ordinary action there
    /// is. A test caught it; nothing else would have until a user did.
    ///
    /// Scanned from the log rather than from the folded state, because a
    /// placement that has been removed still owns its number: re-using it would
    /// collide with the operations that placed and removed it.
    ///
    /// Called after a merge rather than on every refold. A merge is rare — a
    /// project is opened once and synchronised occasionally — and an edit is
    /// not, so the linear scan belongs on the rare path.
    fn absorb_placement_identities(&mut self) {
        let base = placement_base(self.device);
        let ceiling = base.saturating_add(u64::from(u32::MAX));

        let highest = self
            .log
            .operations()
            .filter_map(|operation| match &operation.payload {
                OperationPayload::PlaceTrack { placement, .. } => Some(placement.get()),
                _ => None,
            })
            .filter(|id| (base..=ceiling).contains(id))
            .max();

        if let Some(highest) = highest {
            self.next_placement = self.next_placement.max(highest.saturating_add(1));
        }
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

    /// The whole project, as bytes.
    ///
    /// # A project file is a message to your future self
    ///
    /// It is the same encoding synchronisation uses, produced the same way, for
    /// a reader that has seen nothing. That is not a shortcut: ADR-0003 says the
    /// project *is* its log, so "everything I have" and "everything a new peer
    /// would need" are the same set of operations, and giving them two formats
    /// would mean two things to keep in step and one of them getting less
    /// attention.
    ///
    /// It includes operations this build cannot interpret, so a project saved by
    /// an older build and reopened by a newer one recovers them — the same
    /// promotion that happens after an upgrade, from a file rather than a peer.
    ///
    /// Opening one needs no call of its own: a fresh engine and
    /// [`Engine::sync_merge`] is exactly what opening means.
    ///
    /// Writes nothing and returns the size required when `into` is too small.
    ///
    /// # Errors
    ///
    /// [`Status::Refused`] if the project will not fit a single message.
    pub fn document(&self, into: &mut [u8]) -> Result<usize, Status> {
        let everything = VersionVector::new();
        let operations = self.log.operations_since(&everything);
        let carried = self.log.carried_since(&everything);
        let bytes = wire::encode_all(&operations, &carried).map_err(|_| Status::Refused)?;
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
        // Both halves. What this build cannot read, it can still be the reason
        // somebody else receives — and a peer cannot tell the difference,
        // because the two are interleaved into the one total order.
        let operations = self.log.operations_since(&peer);
        let carried = self.log.carried_since(&peer);
        self.outbound = wire::encode_all(&operations, &carried).map_err(|_| Status::Refused)?;
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
        let report = self
            .log
            .merge(&message.to_operations())
            .map_err(|_| Status::Refused)?;
        // Kept, not merely counted. The log records them as seen, which stops
        // peers resending them — honest only because it also holds the bytes.
        let carried = self
            .log
            .carry(&message.to_carried())
            .map_err(|_| Status::Refused)?;
        self.absorb_placement_identities();
        self.refold()?;
        Ok(SyncReport {
            applied: report.applied.try_into().unwrap_or(u64::MAX),
            already_present: report.already_present.try_into().unwrap_or(u64::MAX),
            conflicts: report.conflicts.len().try_into().unwrap_or(u64::MAX),
            carried: carried.try_into().unwrap_or(u64::MAX),
        })
    }

    /// How many operations this project holds that were made by a newer build.
    ///
    /// Non-zero means part of the project was made with a newer version of the
    /// application. It is being kept and passed on, and it cannot be shown.
    #[must_use]
    pub fn carried_count(&self) -> u64 {
        self.log.carried_count().try_into().unwrap_or(u64::MAX)
    }

    /// Re-reads carried operations, keeping the ones this build now understands.
    ///
    /// Returns how many became ordinary operations. What an upgrade is for:
    /// work that arrived from a newer build and could only be carried becomes
    /// part of the project the moment this build learns its meaning. Cheap when
    /// there is nothing to do, so the natural place to call it is immediately
    /// after opening a project.
    ///
    /// # Errors
    ///
    /// [`Status::InvalidArgument`] if the project cannot be refolded, which
    /// cannot happen for a shape that was already valid.
    pub fn promote_carried(&mut self) -> Result<u64, Status> {
        let promoted = self.log.promote_carried();
        if promoted > 0 {
            self.refold()?;
        }
        Ok(promoted.try_into().unwrap_or(u64::MAX))
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
    fn the_timeline_can_be_read_back_clip_by_clip() {
        // What a mix editor is drawn from. Until this existed, a host could say
        // how many clips a project held and how long it was, and could not draw
        // a single one of them.
        let mut engine = engine(1);
        let first = engine
            .place_track(7, 0, 4_096, 512, 0, 1_000)
            .expect("places");
        let second = engine
            .place_track(9, 4_096, 8_192, 0, 1, 1_000)
            .expect("places");

        assert_eq!(engine.placement_count(), 2);

        let clips: Vec<Placement> = (0..2)
            .map(|index| engine.placement(index).expect("a clip"))
            .collect();

        let one = clips
            .iter()
            .find(|clip| clip.id == first)
            .expect("the first");
        assert_eq!(one.track, 7);
        assert_eq!(one.position, 0);
        assert_eq!(one.length, 4_096);
        assert_eq!(one.lane, 0);
        assert_eq!(one.source_offset, 512, "the source offset did not survive");

        let two = clips
            .iter()
            .find(|clip| clip.id == second)
            .expect("the second");
        assert_eq!(two.track, 9);
        assert_eq!(two.position, 4_096);
        assert_eq!(two.lane, 1);
        assert_eq!(two.source_offset, 0);
    }

    #[test]
    fn an_edit_survives_being_undone_and_the_history_still_says_what_happened() {
        // ADR-0003: the project *is* its log, so undo is a consequence of the
        // mechanism rather than a feature bolted beside it.
        let mut engine = engine(1);
        let clip = engine
            .place_track(1, 0, 4_096, 0, 0, 1_000)
            .expect("places");

        engine.move_placement(clip, 8_192, 1, 2_000).expect("moves");
        assert_eq!(engine.placement(0).expect("a clip").position, 8_192);
        assert_eq!(engine.placement(0).expect("a clip").lane, 1);

        assert_eq!(engine.undo_is_available(3_000), 0);
        assert!(engine.undo(3_000).expect("undoes") > 0);
        assert_eq!(engine.placement(0).expect("a clip").position, 0);
        assert_eq!(engine.placement(0).expect("a clip").lane, 0);
    }

    #[test]
    fn removing_a_clip_is_reversible_because_nothing_was_deleted() {
        let mut engine = engine(1);
        let clip = engine
            .place_track(1, 0, 4_096, 0, 0, 1_000)
            .expect("places");
        engine.remove_placement(clip, 2_000).expect("removes");
        assert_eq!(engine.placement_count(), 0);

        assert!(engine.undo(3_000).expect("undoes") > 0);
        assert_eq!(engine.placement_count(), 1);
        assert_eq!(engine.placement(0).expect("a clip").id, clip);
    }

    #[test]
    fn trimming_refuses_a_length_that_would_lose_the_clip() {
        let mut engine = engine(1);
        let clip = engine
            .place_track(1, 0, 4_096, 0, 0, 1_000)
            .expect("places");

        assert_eq!(
            engine.trim_placement(clip, 0, 2_000).err(),
            Some(Status::InvalidArgument)
        );
        assert_eq!(
            engine.trim_placement(clip, -1, 2_000).err(),
            Some(Status::InvalidArgument)
        );
        engine.trim_placement(clip, 2_048, 2_000).expect("trims");
        assert_eq!(engine.placement(0).expect("a clip").length, 2_048);
    }

    #[test]
    fn editing_a_clip_that_is_not_there_is_refused_at_the_boundary() {
        // Not left to the fold. An operation naming a clip that does not exist
        // would be appended, synchronised, and silently ignored on every device
        // that received it — a no-op propagated as though it were work.
        let mut engine = engine(1);
        engine
            .place_track(1, 0, 4_096, 0, 0, 1_000)
            .expect("places");

        assert_eq!(
            engine.move_placement(9_999, 0, 0, 2_000).err(),
            Some(Status::InvalidArgument)
        );
        assert_eq!(
            engine.remove_placement(9_999, 2_000).err(),
            Some(Status::InvalidArgument)
        );
        assert_eq!(engine.log_length(), 1, "a refused edit reached the log");
    }

    #[test]
    fn an_undo_that_would_discard_somebody_elses_work_is_refused_rather_than_done() {
        // The case with no correct silent answer. I move a clip, you move it
        // again; the inverse of my move puts it back where it was before either
        // of us touched it, discarding your work without saying so.
        let mut mine = engine(1);
        let clip = mine.place_track(1, 0, 4_096, 0, 0, 1_000).expect("places");
        mine.move_placement(clip, 4_096, 0, 2_000).expect("moves");

        // The other device's later edit arrives.
        let theirs = {
            let mut log = prv_project::OperationLog::new();
            log.merge(&mine.log.operations().cloned().collect::<Vec<_>>())
                .expect("merges");
            let operation = log.author(
                DeviceId::new(2),
                3_000,
                OperationPayload::MovePlacement {
                    placement: PlacementId::new(clip),
                    position: Frames::new(8_192),
                    lane: 0,
                },
            );
            log.append(operation.clone()).expect("appends");
            operation
        };
        mine.log.merge(&[theirs]).expect("merges");

        assert_eq!(mine.undo_is_available(4_000), 2, "the collision was missed");
        assert_eq!(mine.undo(4_000).expect("refuses quietly"), 0);
    }

    #[test]
    fn a_clip_past_the_end_is_refused_rather_than_invented() {
        let mut engine = engine(1);
        engine
            .place_track(1, 0, 4_096, 0, 0, 1_000)
            .expect("places");

        assert_eq!(engine.placement(1).err(), Some(Status::InvalidArgument));
        assert_eq!(
            engine.placement(u64::MAX).err(),
            Some(Status::InvalidArgument)
        );
    }

    #[test]
    fn identity_order_does_not_move_when_a_clip_does() {
        // The reason the order is by identity rather than by position: a list
        // that reordered itself under the hand doing the dragging is why
        // timelines flicker.
        let mut engine = engine(1);
        for index in 0..4 {
            engine
                .place_track(1, index * 4_096, 4_096, 0, 0, 1_000)
                .expect("places");
        }
        let before: Vec<u64> = (0..4)
            .map(|index| engine.placement(index).expect("a clip").id)
            .collect();

        // Move the first clip past all the others. Time order is now different;
        // identity order must not be.
        let moved = before.first().copied().expect("a clip");
        engine
            .move_placement(moved, 999_999, 0, 2_000)
            .expect("moves");

        let after: Vec<u64> = (0..4)
            .map(|index| engine.placement(index).expect("a clip").id)
            .collect();
        assert_eq!(before, after, "identity order changed when a clip moved");
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

    /// A message from a build that does not exist yet, carrying one operation
    /// this one cannot read. Built by hand, because the encoder cannot make it.
    fn a_message_from_the_future(author: u64, sequence: u64) -> Vec<u8> {
        let mut entry = Vec::new();
        entry.extend_from_slice(&author.to_le_bytes());
        entry.extend_from_slice(&sequence.to_le_bytes());
        entry.extend_from_slice(&0_i64.to_le_bytes());
        entry.extend_from_slice(&0_u32.to_le_bytes());
        entry.extend_from_slice(&60_000_u16.to_le_bytes());
        entry.extend_from_slice(&4_u32.to_le_bytes());
        entry.extend_from_slice(b"soon");

        let mut header = Vec::new();
        header.extend_from_slice(&1_u32.to_le_bytes());

        let mut message = Vec::new();
        message.extend_from_slice(b"PRVL");
        message.extend_from_slice(&1_u16.to_le_bytes());
        message.extend_from_slice(&0_u16.to_le_bytes());
        message.extend_from_slice(&u32::try_from(header.len()).expect("fits").to_le_bytes());
        message.extend_from_slice(&header);
        message.extend_from_slice(&u32::try_from(entry.len()).expect("fits").to_le_bytes());
        message.extend_from_slice(&entry);
        message
    }

    #[test]
    fn a_stale_device_carries_work_it_cannot_read_to_the_device_that_can() {
        // Three installations, the middle one a version behind. The property
        // that makes it a relay rather than a hole: what it cannot apply, it
        // keeps and passes on, and the third receives it byte for byte.
        let mut relay = engine(2);
        let received = relay
            .sync_merge(&a_message_from_the_future(3, 1))
            .expect("merges");
        assert_eq!(received.applied, 0);
        assert_eq!(received.carried, 1);
        assert_eq!(relay.carried_count(), 1);

        // It also does its own work, which must travel in the same message.
        relay.place_track(1, 0, 4_096, 0, 0, 1_000).expect("places");

        let onward = engine(4);
        let mut state = vec![0_u8; 512];
        let needed = onward.sync_state(&mut state).expect("state");
        let size = relay.sync_prepare(&state[..needed]).expect("prepares");
        let mut message = vec![0_u8; size];
        relay.sync_outbound(&mut message).expect("copies");

        let mut onward = onward;
        let arrived = onward.sync_merge(&message).expect("merges");
        assert_eq!(arrived.applied, 1, "the relay's own edit did not travel");
        assert_eq!(arrived.carried, 1, "the future's edit did not travel");
        assert_eq!(onward.carried_count(), 1);
    }

    #[test]
    fn a_carried_operation_is_not_sent_back_to_the_peer_that_sent_it() {
        // Carrying records the identity as seen, so the sender stops resending.
        // Honest only because the bytes are kept — which the previous test is
        // what proves.
        let mut relay = engine(2);
        relay
            .sync_merge(&a_message_from_the_future(3, 1))
            .expect("merges");

        let mut state = vec![0_u8; 512];
        let needed = relay.sync_state(&mut state).expect("state");
        let seen = wire::decode_vector(&state[..needed]).expect("decodes");
        assert!(seen.has_seen(prv_project::OperationId::new(DeviceId::new(3), 1)));
    }

    #[test]
    fn promoting_finds_nothing_while_the_work_is_still_beyond_this_build() {
        let mut relay = engine(2);
        relay
            .sync_merge(&a_message_from_the_future(3, 1))
            .expect("merges");

        assert_eq!(relay.promote_carried().expect("promotes"), 0);
        assert_eq!(relay.carried_count(), 1, "an operation was lost to a retry");
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
    fn a_project_saved_and_opened_is_the_same_project() {
        // What the application has never been able to do. Opening needs no call
        // of its own: a fresh engine and a merge *is* what opening means,
        // because a project file is a message to your future self.
        let mut authored = engine(1);
        for index in 0..5_u64 {
            let position = i64::try_from(index).expect("small") * 4_096;
            authored
                .place_track(index + 1, position, 4_096, 0, 0, 1_000)
                .expect("places");
        }
        let clip = authored.placement(0).expect("a clip").id;
        authored
            .move_placement(clip, 99_999, 1, 2_000)
            .expect("moves");

        let mut size = vec![0_u8; 0];
        let needed = authored.document(&mut size).expect("sizes");
        let mut document = vec![0_u8; needed];
        assert_eq!(authored.document(&mut document).expect("writes"), needed);

        let mut opened = engine(1);
        let report = opened.sync_merge(&document).expect("opens");
        assert_eq!(
            report.conflicts, 0,
            "opening a project conflicted with itself"
        );

        assert_eq!(opened.placement_count(), authored.placement_count());
        assert_eq!(opened.duration(), authored.duration());
        assert_eq!(opened.log_length(), authored.log_length());
        for index in 0..opened.placement_count() {
            assert_eq!(
                opened.placement(index).expect("a clip"),
                authored.placement(index).expect("a clip")
            );
        }
    }

    #[test]
    fn editing_after_opening_does_not_reuse_a_number_the_file_already_used() {
        // The failure this would otherwise have: the reopened project authors
        // an operation with a sequence the file already spent, and the two are
        // the same operation by name and different by content — which a merge
        // reports as a conflict, on a project nobody else has touched.
        let mut authored = engine(1);
        authored
            .place_track(1, 0, 4_096, 0, 0, 1_000)
            .expect("places");
        authored
            .place_track(2, 4_096, 4_096, 0, 0, 1_000)
            .expect("places");

        let mut document = vec![0_u8; authored.document(&mut [][..]).expect("sizes")];
        authored.document(&mut document).expect("writes");

        let mut opened = engine(1);
        opened.sync_merge(&document).expect("opens");
        let before = opened.log_length();
        opened
            .place_track(3, 8_192, 4_096, 0, 0, 2_000)
            .expect("places");

        assert_eq!(opened.log_length(), before + 1, "an identity was reused");
        assert_eq!(opened.placement_count(), 3);
    }

    #[test]
    fn a_project_file_carries_what_this_build_cannot_read() {
        // A project saved by an older build and reopened by a newer one recovers
        // work the older one could only carry. Saving has to keep it for that to
        // be possible at all.
        let mut relay = engine(2);
        relay
            .sync_merge(&a_message_from_the_future(3, 1))
            .expect("merges");
        assert_eq!(relay.carried_count(), 1);

        let mut document = vec![0_u8; relay.document(&mut [][..]).expect("sizes")];
        relay.document(&mut document).expect("writes");

        let mut opened = engine(2);
        let report = opened.sync_merge(&document).expect("opens");
        assert_eq!(report.carried, 1, "saving dropped what could not be read");
        assert_eq!(opened.carried_count(), 1);
    }

    #[test]
    fn an_empty_project_still_saves_to_something_that_opens() {
        let empty = engine(1);
        let mut document = vec![0_u8; empty.document(&mut [][..]).expect("sizes")];
        empty.document(&mut document).expect("writes");
        assert!(!document.is_empty(), "an empty project saved to nothing");

        let mut opened = engine(1);
        assert_eq!(opened.sync_merge(&document).expect("opens").applied, 0);
        assert_eq!(opened.placement_count(), 0);
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
