//! The functions a host actually calls.
//!
//! # Every one of these is the same shape
//!
//! Check the pointers, do the work inside a guard, write results through
//! out-parameters, return a status. The uniformity is deliberate: a boundary
//! where each function is arranged slightly differently is a boundary where the
//! one that forgot its null check looks exactly like the others.
//!
//! # Nothing here allocates on behalf of the host
//!
//! Except [`prv_engine_create`], which allocates the engine and hands back the
//! only pointer to it. Everything else writes into memory the caller already
//! owns. That means there is exactly one thing a host must remember to free, and
//! exactly one function that frees it, which is about as small as an ownership
//! story gets.

use crate::abi;
use crate::analysis::Analysis;
use crate::collection::Collection;
use crate::delivery::Delivery;
use crate::engine::{Engine, ReadAudio};
use crate::experience::Experience;
use crate::guard::{as_mut, as_ref, buffer_capacity, guarded_try, readable, writable};
use crate::mapping::event_from_code;
use crate::planning::Planner;
use crate::policy::Policy;
use crate::status::Status;

/// The version of this boundary, packed as `major << 16 | minor << 8 | patch`.
///
/// The first call a host should make. See [`crate::abi`] for what a host does
/// with the answer.
#[no_mangle]
pub extern "C" fn prv_abi_version() -> u32 {
    abi::version()
}

/// Whether a host built against `host_major` can use this library.
#[no_mangle]
pub extern "C" fn prv_abi_is_compatible(host_major: u32) -> i32 {
    i32::from(abi::is_compatible_with(host_major))
}

/// A static, NUL-terminated description of a status code.
///
/// Never null: an unrecognised code returns a string saying so, because a host
/// reporting an error is already having a bad day and should not also have to
/// null-check the explanation.
#[no_mangle]
pub extern "C" fn prv_status_message(code: i32) -> *const core::ffi::c_char {
    let text = match Status::from_code(code) {
        Some(status) => status.message(),
        None => "unrecognised status code\0",
    };
    text.as_ptr().cast()
}

/// Creates an engine.
///
/// Writes the handle to `out_engine` on success. On failure `out_engine` is set
/// to null and a status is returned, so a host that ignores the status still
/// gets a null pointer rather than an uninitialised one.
///
/// # Safety
///
/// `out_engine` must be a valid, writable pointer to a single pointer.
#[no_mangle]
pub unsafe extern "C" fn prv_engine_create(
    sample_rate: u32,
    channels: u32,
    max_block_frames: u32,
    out_engine: *mut *mut Engine,
) -> i32 {
    guarded_try(|| {
        // SAFETY: the caller's documented contract.
        let slot = unsafe { as_mut(out_engine) }?;
        *slot = core::ptr::null_mut();

        let engine = Engine::new(sample_rate, channels, max_block_frames)?;
        *slot = Box::into_raw(Box::new(engine));
        Ok(())
    })
    .code()
}

/// Destroys an engine.
///
/// Null is accepted and does nothing, which is what every C library that is
/// pleasant to use does with its free function.
///
/// # Safety
///
/// `engine` must be a pointer returned by [`prv_engine_create`] and not yet
/// destroyed. Passing it twice is a double free.
#[no_mangle]
pub unsafe extern "C" fn prv_engine_destroy(engine: *mut Engine) {
    if engine.is_null() {
        return;
    }
    // The drop runs inside the guard for the same reason every other call does:
    // a panic in a destructor unwinding into C is undefined behaviour, and a
    // host calling a free function has no way to respond to one anyway.
    let _ = crate::guard::guarded(|| {
        // SAFETY: the caller's documented contract — a live pointer from
        // `prv_engine_create`, not previously destroyed. Reconstructing the box
        // is what returns the allocation.
        drop(unsafe { Box::from_raw(engine) });
        Status::Ok
    });
}

/// Registers where the renderer gets audio.
///
/// Passing a null callback detaches the source; the engine then renders silence
/// and reports every placement as incomplete, which is a defined state rather
/// than a crash.
///
/// # Safety
///
/// `engine` must be live. `user_data` is never dereferenced by the core, but
/// must stay valid for as long as the callback can be invoked — that is, until
/// the engine is destroyed or another source replaces this one.
#[no_mangle]
pub unsafe extern "C" fn prv_engine_set_source(
    engine: *mut Engine,
    read: Option<ReadAudio>,
    user_data: *mut core::ffi::c_void,
) -> i32 {
    guarded_try(|| {
        // SAFETY: the caller's documented contract.
        let engine = unsafe { as_mut(engine) }?;
        engine.set_source(read, user_data);
        Ok(())
    })
    .code()
}

/// Applies a transport event, named by its ABI code.
///
/// # Safety
///
/// `engine` must be live.
#[no_mangle]
pub unsafe extern "C" fn prv_engine_transport(engine: *mut Engine, event_code: i32) -> i32 {
    guarded_try(|| {
        // SAFETY: the caller's documented contract.
        let engine = unsafe { as_mut(engine) }?;
        let Some(event) = event_from_code(event_code) else {
            return Err(Status::InvalidArgument);
        };
        engine.transport(event)
    })
    .code()
}

/// Moves the playhead to an absolute frame position.
///
/// # Safety
///
/// `engine` must be live.
#[no_mangle]
pub unsafe extern "C" fn prv_engine_seek(engine: *mut Engine, position: i64) -> i32 {
    guarded_try(|| {
        // SAFETY: the caller's documented contract.
        let engine = unsafe { as_mut(engine) }?;
        engine.seek(position);
        Ok(())
    })
    .code()
}

/// Reads the playhead position, in frames.
///
/// # Safety
///
/// `engine` must be live and `out_position` writable.
#[no_mangle]
pub unsafe extern "C" fn prv_engine_position(engine: *const Engine, out_position: *mut i64) -> i32 {
    guarded_try(|| {
        // SAFETY: the caller's documented contract.
        let engine = unsafe { as_ref(engine) }?;
        // SAFETY: as above.
        let slot = unsafe { as_mut(out_position) }?;
        *slot = engine.position();
        Ok(())
    })
    .code()
}

/// Reads the playback state, as its ABI code.
///
/// # Safety
///
/// `engine` must be live and `out_state` writable.
#[no_mangle]
pub unsafe extern "C" fn prv_engine_playback_state(
    engine: *const Engine,
    out_state: *mut i32,
) -> i32 {
    guarded_try(|| {
        // SAFETY: the caller's documented contract.
        let engine = unsafe { as_ref(engine) }?;
        // SAFETY: as above.
        let slot = unsafe { as_mut(out_state) }?;
        *slot = engine.playback_state();
        Ok(())
    })
    .code()
}

/// Places a track on the timeline and returns its placement identity.
///
/// `source_offset` is how far into the track playback begins; zero means the
/// start. `timestamp_micros` is supplied by the host because ADR-0001 keeps the
/// core away from the clock.
///
/// # Safety
///
/// `engine` must be live and `out_placement` writable.
#[no_mangle]
#[allow(
    clippy::too_many_arguments,
    reason = "a placement is seven independent facts and bundling them into a \
              struct would put a layout promise into the ABI for no benefit"
)]
pub unsafe extern "C" fn prv_engine_place_track(
    engine: *mut Engine,
    track: u64,
    position: i64,
    length: i64,
    source_offset: i64,
    lane: u32,
    timestamp_micros: i64,
    out_placement: *mut u64,
) -> i32 {
    guarded_try(|| {
        // SAFETY: the caller's documented contract.
        let engine = unsafe { as_mut(engine) }?;
        // SAFETY: as above.
        let slot = unsafe { as_mut(out_placement) }?;
        *slot = 0;
        *slot = engine.place_track(
            track,
            position,
            length,
            source_offset,
            lane,
            timestamp_micros,
        )?;
        Ok(())
    })
    .code()
}

/// Reads the project's length in frames.
///
/// # Safety
///
/// `engine` must be live and `out_duration` writable.
#[no_mangle]
pub unsafe extern "C" fn prv_engine_duration(engine: *const Engine, out_duration: *mut i64) -> i32 {
    guarded_try(|| {
        // SAFETY: the caller's documented contract.
        let engine = unsafe { as_ref(engine) }?;
        // SAFETY: as above.
        let slot = unsafe { as_mut(out_duration) }?;
        *slot = engine.duration();
        Ok(())
    })
    .code()
}

/// Reads how many placements the project holds.
///
/// # Safety
///
/// `engine` must be live and `out_count` writable.
#[no_mangle]
pub unsafe extern "C" fn prv_engine_placement_count(
    engine: *const Engine,
    out_count: *mut u64,
) -> i32 {
    guarded_try(|| {
        // SAFETY: the caller's documented contract.
        let engine = unsafe { as_ref(engine) }?;
        // SAFETY: as above.
        let slot = unsafe { as_mut(out_count) }?;
        *slot = engine.placement_count();
        Ok(())
    })
    .code()
}

/// Renders one block into a caller-owned planar buffer.
///
/// # This is the audio thread
///
/// `planar` must point at `channels * frames` floats, channel-major. The call
/// allocates nothing, locks nothing and waits for nothing. It is the only
/// function in this header a host may call from a realtime context, and the only
/// one it must.
///
/// # Safety
///
/// `engine` must be live, and `planar` must be writable for
/// `channels * frames` floats.
#[no_mangle]
pub unsafe extern "C" fn prv_engine_render(
    engine: *mut Engine,
    planar: *mut f32,
    channels: u32,
    frames: u32,
) -> i32 {
    guarded_try(|| {
        // SAFETY: the caller's documented contract.
        let engine = unsafe { as_mut(engine) }?;
        if planar.is_null() {
            return Err(Status::NullPointer);
        }
        if channels == 0 || frames == 0 {
            return Err(Status::InvalidArgument);
        }
        let channel_count = usize::try_from(channels).unwrap_or(0);
        let frame_count = usize::try_from(frames).unwrap_or(0);
        let total = channel_count
            .checked_mul(frame_count)
            .ok_or(Status::InvalidArgument)?;

        // SAFETY: the caller promises `planar` is writable for exactly this many
        // floats. The slice is used only within this call and never escapes it.
        let samples = unsafe { core::slice::from_raw_parts_mut(planar, total) };
        engine.render_into(samples, channel_count, frame_count)
    })
    .code()
}

/// Whether every placement the last render touched was read in full.
///
/// A host shows this as "some audio was missing" rather than treating it as an
/// error: the render happened, and the parts that were there are correct.
///
/// # Safety
///
/// `engine` must be live and `out_complete` writable.
#[no_mangle]
pub unsafe extern "C" fn prv_engine_render_was_complete(
    engine: *const Engine,
    out_complete: *mut i32,
) -> i32 {
    guarded_try(|| {
        // SAFETY: the caller's documented contract.
        let engine = unsafe { as_ref(engine) }?;
        // SAFETY: as above.
        let slot = unsafe { as_mut(out_complete) }?;
        *slot = i32::from(engine.render_was_complete());
        Ok(())
    })
    .code()
}

// ---------------------------------------------------------------------------
// Synchronisation
//
// The core produces bytes and reads bytes; the host owns the socket. ADR-0001
// makes that non-negotiable, and it is also the arrangement that lets the same
// four calls serve a cloud service, a local network, a USB stick and a file
// attached to an email — none of which the core has to know about.
//
// The protocol is two messages and three steps. A device sends its version
// vector; the other side answers with the operations that vector has not seen;
// each merges what it received. Both sides may do it at once, and neither is
// authoritative.
// ---------------------------------------------------------------------------

/// Declares which device this is.
///
/// Must be called before the project holds any operations, and must be given a
/// value that is stable for this installation and distinct from every other —
/// both of which are facts about the machine, which is why the host supplies
/// them.
///
/// A host that never calls this gets a default that is correct for one machine
/// and wrong for a fleet. It fails loudly rather than quietly: two devices
/// sharing an identity produce operations with the same name and different
/// contents, and a merge reports those as conflicts instead of letting one
/// overwrite the other.
///
/// # Safety
///
/// `engine` must be live.
#[no_mangle]
pub unsafe extern "C" fn prv_engine_set_device(engine: *mut Engine, device: u64) -> i32 {
    guarded_try(|| {
        // SAFETY: the caller's documented contract.
        unsafe { as_mut(engine) }?.set_device(device)
    })
    .code()
}

/// Writes what this project has seen, for a peer to answer.
///
/// Writes to `out_needed` how many bytes the vector requires whether or not it
/// fitted, so a caller may pass a null buffer with a zero capacity to ask the
/// size and then allocate exactly.
///
/// # Safety
///
/// `engine` must be live, `into` writable for `capacity` bytes, and `out_needed`
/// writable.
#[no_mangle]
pub unsafe extern "C" fn prv_engine_sync_state(
    engine: *const Engine,
    into: *mut u8,
    capacity: u64,
    out_needed: *mut u64,
) -> i32 {
    guarded_try(|| {
        // SAFETY: the caller's documented contract.
        let engine = unsafe { as_ref(engine) }?;
        // SAFETY: as above.
        let buffer = unsafe { writable(into, capacity) }?;
        let needed = engine.sync_state(buffer)?;
        // SAFETY: as above.
        *unsafe { as_mut(out_needed) }? = needed.try_into().unwrap_or(u64::MAX);
        if needed > buffer_capacity(capacity) {
            return Err(Status::BufferTooSmall);
        }
        Ok(())
    })
    .code()
}

/// Prepares the operations a peer has not seen and reports their size.
///
/// Nothing is copied out here. The message is built once and held, so a host
/// learns the exact size before allocating and
/// [`prv_engine_sync_outbound`] hands it over — which also means a host whose
/// buffer was too small may simply ask again rather than rebuild.
///
/// # Safety
///
/// `engine` must be live, `peer_state` readable for `peer_state_len` bytes, and
/// `out_needed` writable.
#[no_mangle]
pub unsafe extern "C" fn prv_engine_sync_prepare(
    engine: *mut Engine,
    peer_state: *const u8,
    peer_state_len: u64,
    out_needed: *mut u64,
) -> i32 {
    guarded_try(|| {
        // SAFETY: the caller's documented contract.
        let engine = unsafe { as_mut(engine) }?;
        // SAFETY: as above.
        let peer = unsafe { readable(peer_state, peer_state_len) }?;
        let needed = engine.sync_prepare(peer)?;
        // SAFETY: as above.
        *unsafe { as_mut(out_needed) }? = needed.try_into().unwrap_or(u64::MAX);
        Ok(())
    })
    .code()
}

/// Copies out the message [`prv_engine_sync_prepare`] built.
///
/// # Safety
///
/// `engine` must be live, `into` writable for `capacity` bytes, and `out_needed`
/// writable.
#[no_mangle]
pub unsafe extern "C" fn prv_engine_sync_outbound(
    engine: *const Engine,
    into: *mut u8,
    capacity: u64,
    out_needed: *mut u64,
) -> i32 {
    guarded_try(|| {
        // SAFETY: the caller's documented contract.
        let engine = unsafe { as_ref(engine) }?;
        // SAFETY: as above.
        let buffer = unsafe { writable(into, capacity) }?;
        let needed = engine.sync_outbound(buffer)?;
        // SAFETY: as above.
        *unsafe { as_mut(out_needed) }? = needed.try_into().unwrap_or(u64::MAX);
        if needed > buffer_capacity(capacity) {
            return Err(Status::BufferTooSmall);
        }
        Ok(())
    })
    .code()
}

/// Merges a message from a peer.
///
/// The four counts are written whatever happens, and they mean four different
/// things: work arrived, work was already here, work disagrees and needs a
/// person, and work could not be read because it was made by a newer build.
///
/// # Safety
///
/// `engine` must be live, `bytes` readable for `len` bytes, and each out-pointer
/// writable.
#[no_mangle]
pub unsafe extern "C" fn prv_engine_sync_merge(
    engine: *mut Engine,
    bytes: *const u8,
    len: u64,
    out_applied: *mut u64,
    out_already_present: *mut u64,
    out_conflicts: *mut u64,
    out_carried: *mut u64,
) -> i32 {
    guarded_try(|| {
        // SAFETY: the caller's documented contract.
        let engine = unsafe { as_mut(engine) }?;
        // SAFETY: as above.
        let message = unsafe { readable(bytes, len) }?;
        let report = engine.sync_merge(message)?;
        // SAFETY: as above.
        *unsafe { as_mut(out_applied) }? = report.applied;
        // SAFETY: as above.
        *unsafe { as_mut(out_already_present) }? = report.already_present;
        // SAFETY: as above.
        *unsafe { as_mut(out_conflicts) }? = report.conflicts;
        // SAFETY: as above.
        *unsafe { as_mut(out_carried) }? = report.carried;
        Ok(())
    })
    .code()
}

/// How many operations this project holds that were made by a newer build.
///
/// Non-zero means part of the project was made with a newer version of the
/// application. It is being kept and passed on to other devices, and it cannot
/// be shown here — which is worth telling the person looking at the screen,
/// because otherwise the project silently appears to be missing work.
///
/// # Safety
///
/// `engine` must be live and `out_count` writable.
#[no_mangle]
pub unsafe extern "C" fn prv_engine_carried_count(
    engine: *const Engine,
    out_count: *mut u64,
) -> i32 {
    guarded_try(|| {
        // SAFETY: the caller's documented contract.
        let count = unsafe { as_ref(engine) }?.carried_count();
        // SAFETY: as above.
        *unsafe { as_mut(out_count) }? = count;
        Ok(())
    })
    .code()
}

/// Re-reads carried operations, keeping the ones this build now understands.
///
/// What an upgrade is for. Work that arrived from a newer version of the
/// application and could only be carried becomes part of the project the moment
/// this build learns its meaning — the same bytes the author wrote, not a
/// reconstruction of them.
///
/// Cheap when there is nothing to do, so the natural place to call it is
/// immediately after opening a project.
///
/// # Safety
///
/// `engine` must be live and `out_promoted` writable.
#[no_mangle]
pub unsafe extern "C" fn prv_engine_promote_carried(
    engine: *mut Engine,
    out_promoted: *mut u64,
) -> i32 {
    guarded_try(|| {
        // SAFETY: the caller's documented contract.
        let promoted = unsafe { as_mut(engine) }?.promote_carried()?;
        // SAFETY: as above.
        *unsafe { as_mut(out_promoted) }? = promoted;
        Ok(())
    })
    .code()
}

// ---------------------------------------------------------------------------
// Planning
//
// A separate handle from the engine, deliberately. A library and a plan are not
// a project: a host may plan against a library with no project open, and may
// keep a project open while replanning. Tying them together would make one
// impossible and the other awkward, and the two have no invariant in common.
// ---------------------------------------------------------------------------

/// Creates a planner.
///
/// # Safety
///
/// `out_planner` must be a valid, writable pointer to a single pointer.
#[no_mangle]
pub unsafe extern "C" fn prv_planner_create(out_planner: *mut *mut Planner) -> i32 {
    guarded_try(|| {
        // SAFETY: the caller's documented contract.
        let slot = unsafe { as_mut(out_planner) }?;
        *slot = core::ptr::null_mut();
        *slot = Box::into_raw(Box::new(Planner::new()));
        Ok(())
    })
    .code()
}

/// Destroys a planner. Null is accepted and does nothing.
///
/// # Safety
///
/// `planner` must be a pointer returned by [`prv_planner_create`] and not yet
/// destroyed.
#[no_mangle]
pub unsafe extern "C" fn prv_planner_destroy(planner: *mut Planner) {
    if planner.is_null() {
        return;
    }
    let _ = crate::guard::guarded(|| {
        // SAFETY: the caller's documented contract.
        drop(unsafe { Box::from_raw(planner) });
        Status::Ok
    });
}

/// Adds one track to the library the planner chooses from.
///
/// `key_confidence` at or below zero means the key is unknown. `has_vocals` is
/// `-1` for unknown, `0` for no, `1` for yes — and unknown is a different answer
/// from no, which scores differently.
///
/// # Safety
///
/// `planner` must be live.
#[no_mangle]
#[allow(
    clippy::too_many_arguments,
    reason = "a candidate is what the analysis found out about a track; a \
              repr(C) struct here would be a permanent layout promise"
)]
pub unsafe extern "C" fn prv_planner_add_candidate(
    planner: *mut Planner,
    track: u64,
    duration: i64,
    bpm: f64,
    energy: f32,
    key_semitones: i32,
    key_is_minor: i32,
    key_confidence: f32,
    loudness_lufs: f32,
    has_vocals: i32,
) -> i32 {
    guarded_try(|| {
        // SAFETY: the caller's documented contract.
        let planner = unsafe { as_mut(planner) }?;
        planner.add_candidate(
            track,
            duration,
            bpm,
            energy,
            key_semitones,
            key_is_minor != 0,
            key_confidence,
            loudness_lufs,
            has_vocals,
        )
    })
    .code()
}

/// Adds a place the analysis says a track can be left or entered.
///
/// # Safety
///
/// `planner` must be live.
#[no_mangle]
pub unsafe extern "C" fn prv_planner_add_mix_point(
    planner: *mut Planner,
    track: u64,
    position: i64,
    energy: f32,
    is_exit: i32,
) -> i32 {
    guarded_try(|| {
        // SAFETY: the caller's documented contract.
        let planner = unsafe { as_mut(planner) }?;
        planner.add_mix_point(track, position, energy, is_exit != 0)
    })
    .code()
}

/// Reads how many candidates the library holds.
///
/// # Safety
///
/// `planner` must be live and `out_count` writable.
#[no_mangle]
pub unsafe extern "C" fn prv_planner_candidate_count(
    planner: *const Planner,
    out_count: *mut u64,
) -> i32 {
    guarded_try(|| {
        // SAFETY: the caller's documented contract.
        let planner = unsafe { as_ref(planner) }?;
        // SAFETY: as above.
        let slot = unsafe { as_mut(out_count) }?;
        *slot = planner.candidate_count();
        Ok(())
    })
    .code()
}

/// Forgets the library and any plan made from it.
///
/// # Safety
///
/// `planner` must be live.
#[no_mangle]
pub unsafe extern "C" fn prv_planner_clear(planner: *mut Planner) -> i32 {
    guarded_try(|| {
        // SAFETY: the caller's documented contract.
        let planner = unsafe { as_mut(planner) }?;
        planner.clear();
        Ok(())
    })
    .code()
}

/// Plans up to three genuinely different sets.
///
/// Pass zero for both tempo bounds to leave the range open. Half a range is
/// treated as no range, because honouring it would constrain a set in a way
/// nobody asked for.
///
/// Writes how many alternatives were produced. Returns `PRV_REFUSED` when no set
/// could be built, which is a real answer about the library rather than a
/// malfunction.
///
/// # Safety
///
/// `planner` must be live and `out_count` writable.
#[no_mangle]
#[allow(
    clippy::too_many_arguments,
    reason = "a goal is what the user asked for, and its parts are independent"
)]
pub unsafe extern "C" fn prv_planner_plan(
    planner: *mut Planner,
    target_frames: i64,
    sample_rate: u32,
    shape: i32,
    creativity: i32,
    tempo_floor: f32,
    tempo_ceiling: f32,
    out_count: *mut u64,
) -> i32 {
    guarded_try(|| {
        // SAFETY: the caller's documented contract.
        let planner = unsafe { as_mut(planner) }?;
        // SAFETY: as above.
        let slot = unsafe { as_mut(out_count) }?;
        *slot = 0;
        *slot = planner.plan(
            target_frames,
            sample_rate,
            shape,
            creativity,
            tempo_floor,
            tempo_ceiling,
        )?;
        Ok(())
    })
    .code()
}

/// Chooses which alternative subsequent reads describe.
///
/// # Safety
///
/// `planner` must be live.
#[no_mangle]
pub unsafe extern "C" fn prv_planner_select(planner: *mut Planner, index: u64) -> i32 {
    guarded_try(|| {
        // SAFETY: the caller's documented contract.
        let planner = unsafe { as_mut(planner) }?;
        planner.select(index)
    })
    .code()
}

/// Reads how many tracks the selected plan holds.
///
/// # Safety
///
/// `planner` must be live and `out_count` writable.
#[no_mangle]
pub unsafe extern "C" fn prv_planner_track_count(
    planner: *const Planner,
    out_count: *mut u64,
) -> i32 {
    guarded_try(|| {
        // SAFETY: the caller's documented contract.
        let planner = unsafe { as_ref(planner) }?;
        // SAFETY: as above.
        let slot = unsafe { as_mut(out_count) }?;
        *slot = planner.track_count()?;
        Ok(())
    })
    .code()
}

/// Reads how long the selected plan runs for, in frames.
///
/// # Safety
///
/// `planner` must be live and `out_duration` writable.
#[no_mangle]
pub unsafe extern "C" fn prv_planner_duration(
    planner: *const Planner,
    out_duration: *mut i64,
) -> i32 {
    guarded_try(|| {
        // SAFETY: the caller's documented contract.
        let planner = unsafe { as_ref(planner) }?;
        // SAFETY: as above.
        let slot = unsafe { as_mut(out_duration) }?;
        *slot = planner.duration()?;
        Ok(())
    })
    .code()
}

/// Reads the selected plan's mean transition score, from zero to one.
///
/// # Safety
///
/// `planner` must be live and `out_score` writable.
#[no_mangle]
pub unsafe extern "C" fn prv_planner_score(planner: *const Planner, out_score: *mut f32) -> i32 {
    guarded_try(|| {
        // SAFETY: the caller's documented contract.
        let planner = unsafe { as_ref(planner) }?;
        // SAFETY: as above.
        let slot = unsafe { as_mut(out_score) }?;
        *slot = planner.score()?;
        Ok(())
    })
    .code()
}

/// Reads one track of the selected plan.
///
/// The score is the move *into* this track, and is 1.0 for the opening track,
/// which was chosen rather than transitioned into.
///
/// # Safety
///
/// `planner` must be live and every out-parameter writable.
#[no_mangle]
pub unsafe extern "C" fn prv_planner_track(
    planner: *const Planner,
    index: u64,
    out_track: *mut u64,
    out_start: *mut i64,
    out_duration: *mut i64,
    out_score: *mut f32,
) -> i32 {
    guarded_try(|| {
        // SAFETY: the caller's documented contract.
        let planner = unsafe { as_ref(planner) }?;
        let (track, start, duration, score) = planner.track(index)?;
        // SAFETY: as above, for each out-parameter.
        unsafe {
            *as_mut(out_track)? = track;
            *as_mut(out_start)? = start;
            *as_mut(out_duration)? = duration;
            *as_mut(out_score)? = score;
        }
        Ok(())
    })
    .code()
}

/// Applies the selected plan to an engine's project.
///
/// The plan becomes ordinary operations on the log — the same ones a hand-made
/// edit produces. Master Prompt #3B requires the user to be able to edit
/// everything the system decides, and after this call there is nothing to
/// distinguish a generated placement from one somebody dragged.
///
/// # Safety
///
/// `planner` and `engine` must both be live.
#[no_mangle]
pub unsafe extern "C" fn prv_planner_apply(
    planner: *const Planner,
    engine: *mut Engine,
    timestamp_micros: i64,
) -> i32 {
    guarded_try(|| {
        // SAFETY: the caller's documented contract.
        let planner = unsafe { as_ref(planner) }?;
        // SAFETY: as above.
        let engine = unsafe { as_mut(engine) }?;
        engine.apply_plan(planner, timestamp_micros)
    })
    .code()
}

/// Ranks the records that sit best after — or before — a given one.
///
/// Answers "what mixes out of this?" without a set, which is the question a
/// person asks at import and every time they look at a record and wonder. It is
/// a different question from planning: a plan judges a move against where the
/// evening is going, and this judges the pair.
///
/// `following` non-zero asks what comes *after* `track`; zero asks what comes
/// *before*. They are genuinely different lists — every component that depends
/// on direction is measured the other way round.
///
/// Writes how many were found to `out_count`, then read them with
/// [`prv_planner_neighbour`]. Records whose keys clash with `track` are not in
/// the list at all: that is a musical fact rather than a preference, and a list
/// that ranked unlistenable moves at the bottom would be one nobody could trust
/// the top of.
///
/// # Safety
///
/// `planner` must be live and `out_count` writable.
#[no_mangle]
pub unsafe extern "C" fn prv_planner_neighbours(
    planner: *mut Planner,
    track: u64,
    following: i32,
    limit: u64,
    out_count: *mut u64,
) -> i32 {
    guarded_try(|| {
        // SAFETY: the caller's documented contract.
        let planner = unsafe { as_mut(planner) }?;
        let found = planner.neighbours(track, following != 0, limit)?;
        // SAFETY: as above.
        *unsafe { as_mut(out_count) }? = found.try_into().unwrap_or(u64::MAX);
        Ok(())
    })
    .code()
}

/// Reads one row of the last ranking.
///
/// `out_weakest` receives the component that costs the pairing the most, as a
/// `PrvComponent`. It is what an interface says out loud: a DJ told "0.71"
/// learns nothing, and a DJ told "the tempo is the hard part here" knows what to
/// do about it.
///
/// # Safety
///
/// `planner` must be live and every out-pointer writable.
#[no_mangle]
pub unsafe extern "C" fn prv_planner_neighbour(
    planner: *const Planner,
    index: u64,
    out_track: *mut u64,
    out_score: *mut f32,
    out_weakest: *mut i32,
) -> i32 {
    guarded_try(|| {
        // SAFETY: the caller's documented contract.
        let (track, score, weakest) = unsafe { as_ref(planner) }?.neighbour(index)?;
        // SAFETY: as above.
        *unsafe { as_mut(out_track) }? = track;
        // SAFETY: as above.
        *unsafe { as_mut(out_score) }? = score;
        // SAFETY: as above.
        *unsafe { as_mut(out_weakest) }? = weakest;
        Ok(())
    })
    .code()
}

// ---------------------------------------------------------------------------
// Analysis
//
// Not the audio thread. These allocate and take seconds on a long track; they
// belong to the background domain. A host that called one from a render
// callback would drop out.
// ---------------------------------------------------------------------------

/// Analyses a track and returns a handle to what was found.
///
/// `samples` is mono, `frames` long. The audio is borrowed for the duration of
/// this call and never retained.
///
/// # Safety
///
/// `samples` must be readable for `frames` floats, and `out_analysis` writable.
#[no_mangle]
pub unsafe extern "C" fn prv_analysis_run(
    samples: *const f32,
    frames: u64,
    sample_rate: u32,
    out_analysis: *mut *mut Analysis,
) -> i32 {
    guarded_try(|| {
        // SAFETY: the caller's documented contract.
        let slot = unsafe { as_mut(out_analysis) }?;
        *slot = core::ptr::null_mut();
        if samples.is_null() {
            return Err(Status::NullPointer);
        }
        let count = usize::try_from(frames).map_err(|_| Status::InvalidArgument)?;
        if count == 0 || count > crate::analysis::MAX_FRAMES {
            return Err(Status::InvalidArgument);
        }

        // SAFETY: the caller promises `samples` is readable for `frames` floats.
        // The slice is used only within this call and never escapes it.
        let audio = unsafe { core::slice::from_raw_parts(samples, count) };
        let analysis = Analysis::run(audio, sample_rate)?;
        *slot = Box::into_raw(Box::new(analysis));
        Ok(())
    })
    .code()
}

/// Destroys an analysis. Null is accepted and does nothing.
///
/// # Safety
///
/// `analysis` must come from [`prv_analysis_run`] and not yet be destroyed.
#[no_mangle]
pub unsafe extern "C" fn prv_analysis_destroy(analysis: *mut Analysis) {
    if analysis.is_null() {
        return;
    }
    let _ = crate::guard::guarded(|| {
        // SAFETY: the caller's documented contract.
        drop(unsafe { Box::from_raw(analysis) });
        Status::Ok
    });
}

/// Reads the tempo, in beats per minute, and how sure the estimate is.
///
/// Returns `PRV_REFUSED` when no pulse was found. That is the answer, not a
/// failure: a track with no discernible tempo has none, and a guessed 120 would
/// reach the planner and a whole set would be built on it.
///
/// # Safety
///
/// `analysis` must be live and both out-parameters writable.
#[no_mangle]
pub unsafe extern "C" fn prv_analysis_tempo(
    analysis: *const Analysis,
    out_bpm: *mut f64,
    out_confidence: *mut f32,
) -> i32 {
    guarded_try(|| {
        // SAFETY: the caller's documented contract.
        let analysis = unsafe { as_ref(analysis) }?;
        let (bpm, confidence) = analysis.tempo()?;
        // SAFETY: as above.
        unsafe {
            *as_mut(out_bpm)? = bpm;
            *as_mut(out_confidence)? = confidence;
        }
        Ok(())
    })
    .code()
}

/// Reads the key: semitones above C, whether it is minor, and the confidence.
///
/// # Safety
///
/// `analysis` must be live and every out-parameter writable.
#[no_mangle]
pub unsafe extern "C" fn prv_analysis_key(
    analysis: *const Analysis,
    out_semitones: *mut i32,
    out_is_minor: *mut i32,
    out_confidence: *mut f32,
) -> i32 {
    guarded_try(|| {
        // SAFETY: the caller's documented contract.
        let analysis = unsafe { as_ref(analysis) }?;
        let (semitones, is_minor, confidence) = analysis.key()?;
        // SAFETY: as above.
        unsafe {
            *as_mut(out_semitones)? = semitones;
            *as_mut(out_is_minor)? = i32::from(is_minor);
            *as_mut(out_confidence)? = confidence;
        }
        Ok(())
    })
    .code()
}

/// Reads the integrated loudness in LUFS and the loudness range.
///
/// # Safety
///
/// `analysis` must be live and both out-parameters writable.
#[no_mangle]
pub unsafe extern "C" fn prv_analysis_loudness(
    analysis: *const Analysis,
    out_integrated: *mut f64,
    out_range: *mut f64,
) -> i32 {
    guarded_try(|| {
        // SAFETY: the caller's documented contract.
        let analysis = unsafe { as_ref(analysis) }?;
        let (integrated, range) = analysis.loudness()?;
        // SAFETY: as above.
        unsafe {
            *as_mut(out_integrated)? = integrated;
            *as_mut(out_range)? = range;
        }
        Ok(())
    })
    .code()
}

/// Reads the true peak, in decibels relative to full scale.
///
/// # Safety
///
/// `analysis` must be live and `out_true_peak` writable.
#[no_mangle]
pub unsafe extern "C" fn prv_analysis_true_peak(
    analysis: *const Analysis,
    out_true_peak: *mut f64,
) -> i32 {
    guarded_try(|| {
        // SAFETY: the caller's documented contract.
        let analysis = unsafe { as_ref(analysis) }?;
        // SAFETY: as above.
        *unsafe { as_mut(out_true_peak) }? = analysis.true_peak()?;
        Ok(())
    })
    .code()
}

/// Reads the track's overall energy, from zero to one.
///
/// # Safety
///
/// `analysis` must be live and `out_energy` writable.
#[no_mangle]
pub unsafe extern "C" fn prv_analysis_energy(
    analysis: *const Analysis,
    out_energy: *mut f32,
) -> i32 {
    guarded_try(|| {
        // SAFETY: the caller's documented contract.
        let analysis = unsafe { as_ref(analysis) }?;
        // SAFETY: as above.
        *unsafe { as_mut(out_energy) }? = analysis.energy()?;
        Ok(())
    })
    .code()
}

/// Reads how long the analysed audio was, in frames.
///
/// # Safety
///
/// `analysis` must be live and `out_frames` writable.
#[no_mangle]
pub unsafe extern "C" fn prv_analysis_duration(
    analysis: *const Analysis,
    out_frames: *mut i64,
) -> i32 {
    guarded_try(|| {
        // SAFETY: the caller's documented contract.
        let analysis = unsafe { as_ref(analysis) }?;
        // SAFETY: as above.
        *unsafe { as_mut(out_frames) }? = analysis.duration();
        Ok(())
    })
    .code()
}

/// Reads how many places a transition could happen.
///
/// # Safety
///
/// `analysis` must be live and `out_count` writable.
#[no_mangle]
pub unsafe extern "C" fn prv_analysis_transition_point_count(
    analysis: *const Analysis,
    out_count: *mut u64,
) -> i32 {
    guarded_try(|| {
        // SAFETY: the caller's documented contract.
        let analysis = unsafe { as_ref(analysis) }?;
        // SAFETY: as above.
        *unsafe { as_mut(out_count) }? = analysis.transition_point_count()?;
        Ok(())
    })
    .code()
}

/// Reads one place a transition could happen: where it starts, and how quiet.
///
/// Quieter is better to mix on, which is why the energy comes back with the
/// position rather than needing a second call.
///
/// # Safety
///
/// `analysis` must be live and both out-parameters writable.
#[no_mangle]
pub unsafe extern "C" fn prv_analysis_transition_point(
    analysis: *const Analysis,
    index: u64,
    out_position: *mut i64,
    out_energy: *mut f32,
) -> i32 {
    guarded_try(|| {
        // SAFETY: the caller's documented contract.
        let analysis = unsafe { as_ref(analysis) }?;
        let (position, energy) = analysis.transition_point(index)?;
        // SAFETY: as above.
        unsafe {
            *as_mut(out_position)? = position;
            *as_mut(out_energy)? = energy;
        }
        Ok(())
    })
    .code()
}

// ---------------------------------------------------------------------------
// Consent and entitlement
//
// Asked in this order: may this leave the device, then is this feature
// available. A user who has not agreed to cloud analysis is not shown a paywall
// for it — they are simply not sent anywhere, whatever tier they are on.
// ---------------------------------------------------------------------------

/// Creates a policy: nothing agreed to, free tier.
///
/// # Safety
///
/// `out_policy` must be a valid, writable pointer to a single pointer.
#[no_mangle]
pub unsafe extern "C" fn prv_policy_create(out_policy: *mut *mut Policy) -> i32 {
    guarded_try(|| {
        // SAFETY: the caller's documented contract.
        let slot = unsafe { as_mut(out_policy) }?;
        *slot = core::ptr::null_mut();
        *slot = Box::into_raw(Box::new(Policy::new()));
        Ok(())
    })
    .code()
}

/// Destroys a policy. Null is accepted and does nothing.
///
/// # Safety
///
/// `policy` must come from [`prv_policy_create`] and not yet be destroyed.
#[no_mangle]
pub unsafe extern "C" fn prv_policy_destroy(policy: *mut Policy) {
    if policy.is_null() {
        return;
    }
    let _ = crate::guard::guarded(|| {
        // SAFETY: the caller's documented contract.
        drop(unsafe { Box::from_raw(policy) });
        Status::Ok
    });
}

/// Records that the user agreed to a purpose.
///
/// `ordinal` identifies *which* agreement was given — a version of the wording,
/// or a sequence. It is what makes this a consent record rather than a boolean.
///
/// # Safety
///
/// `policy` must be live.
#[no_mangle]
pub unsafe extern "C" fn prv_policy_grant(policy: *mut Policy, purpose: i32, ordinal: u64) -> i32 {
    guarded_try(|| {
        // SAFETY: the caller's documented contract.
        unsafe { as_mut(policy) }?.grant(purpose, ordinal)
    })
    .code()
}

/// Records that the user withdrew a purpose.
///
/// # Safety
///
/// `policy` must be live.
#[no_mangle]
pub unsafe extern "C" fn prv_policy_withdraw(policy: *mut Policy, purpose: i32) -> i32 {
    guarded_try(|| {
        // SAFETY: the caller's documented contract.
        unsafe { as_mut(policy) }?.withdraw(purpose)
    })
    .code()
}

/// Withdraws every agreement at once.
///
/// One call rather than a loop in the host, because a loop in the host is a loop
/// that can be interrupted half way.
///
/// # Safety
///
/// `policy` must be live.
#[no_mangle]
pub unsafe extern "C" fn prv_policy_withdraw_all(policy: *mut Policy) -> i32 {
    guarded_try(|| {
        // SAFETY: the caller's documented contract.
        unsafe { as_mut(policy) }?.withdraw_all();
        Ok(())
    })
    .code()
}

/// Whether a purpose is currently agreed to.
///
/// # Safety
///
/// `policy` must be live and `out_allowed` writable.
#[no_mangle]
pub unsafe extern "C" fn prv_policy_allows(
    policy: *const Policy,
    purpose: i32,
    out_allowed: *mut i32,
) -> i32 {
    guarded_try(|| {
        // SAFETY: the caller's documented contract.
        let allowed = unsafe { as_ref(policy) }?.allows(purpose)?;
        // SAFETY: as above.
        *unsafe { as_mut(out_allowed) }? = i32::from(allowed);
        Ok(())
    })
    .code()
}

/// Whether anything at all currently leaves the device.
///
/// The single question a privacy screen leads with. Composed in the core from
/// every purpose that transmits, so a purpose added later is included without
/// any host being changed.
///
/// # Safety
///
/// `policy` must be live and `out_leaves` writable.
#[no_mangle]
pub unsafe extern "C" fn prv_policy_anything_leaves_the_device(
    policy: *const Policy,
    out_leaves: *mut i32,
) -> i32 {
    guarded_try(|| {
        // SAFETY: the caller's documented contract.
        let leaves = unsafe { as_ref(policy) }?.anything_leaves_the_device();
        // SAFETY: as above.
        *unsafe { as_mut(out_leaves) }? = i32::from(leaves);
        Ok(())
    })
    .code()
}

/// Whether a purpose sends the user's own material rather than a fact about it.
///
/// A different question from whether anything leaves the device: a crash report
/// leaves and carries no music. A consent screen needs both, and one built on
/// either alone is misleading in one direction or the other.
///
/// # Safety
///
/// `out_sends` must be writable.
#[no_mangle]
pub unsafe extern "C" fn prv_purpose_sends_content(purpose: i32, out_sends: *mut i32) -> i32 {
    guarded_try(|| {
        let sends = Policy::purpose_sends_content(purpose)?;
        // SAFETY: the caller's documented contract.
        *unsafe { as_mut(out_sends) }? = i32::from(sends);
        Ok(())
    })
    .code()
}

/// Sets the licence tier.
///
/// # Safety
///
/// `policy` must be live.
#[no_mangle]
pub unsafe extern "C" fn prv_policy_set_tier(policy: *mut Policy, tier: i32) -> i32 {
    guarded_try(|| {
        // SAFETY: the caller's documented contract.
        unsafe { as_mut(policy) }?.set_tier(tier)
    })
    .code()
}

/// Marks the licence expired.
///
/// Not a lock-out: everything essential survives, because Master Prompt #29
/// forbids restricting essential functionality.
///
/// # Safety
///
/// `policy` must be live.
#[no_mangle]
pub unsafe extern "C" fn prv_policy_expire(policy: *mut Policy) -> i32 {
    guarded_try(|| {
        // SAFETY: the caller's documented contract.
        unsafe { as_mut(policy) }?.expire();
        Ok(())
    })
    .code()
}

/// Reads the current licence tier.
///
/// # Safety
///
/// `policy` must be live and `out_tier` writable.
#[no_mangle]
pub unsafe extern "C" fn prv_policy_tier(policy: *const Policy, out_tier: *mut i32) -> i32 {
    guarded_try(|| {
        // SAFETY: the caller's documented contract.
        let tier = unsafe { as_ref(policy) }?.tier();
        // SAFETY: as above.
        *unsafe { as_mut(out_tier) }? = tier;
        Ok(())
    })
    .code()
}

/// Whether a feature is available under the current licence.
///
/// # Safety
///
/// `policy` must be live and `out_allowed` writable.
#[no_mangle]
pub unsafe extern "C" fn prv_policy_feature_allowed(
    policy: *const Policy,
    feature: i32,
    out_allowed: *mut i32,
) -> i32 {
    guarded_try(|| {
        // SAFETY: the caller's documented contract.
        let allowed = unsafe { as_ref(policy) }?.feature_allowed(feature)?;
        // SAFETY: as above.
        *unsafe { as_mut(out_allowed) }? = i32::from(allowed);
        Ok(())
    })
    .code()
}

/// Whether a feature is essential, and so present at every tier.
///
/// A host uses this to decide whether an unavailable feature deserves an upgrade
/// prompt or a bug report. An essential feature that is somehow unavailable is a
/// defect, not an upsell.
///
/// # Safety
///
/// `policy` must be live and `out_essential` writable.
#[no_mangle]
pub unsafe extern "C" fn prv_policy_feature_is_essential(
    policy: *const Policy,
    feature: i32,
    out_essential: *mut i32,
) -> i32 {
    guarded_try(|| {
        // SAFETY: the caller's documented contract.
        let essential = unsafe { as_ref(policy) }?.feature_is_essential(feature)?;
        // SAFETY: as above.
        *unsafe { as_mut(out_essential) }? = i32::from(essential);
        Ok(())
    })
    .code()
}

// ---------------------------------------------------------------------------
// The collection
//
// A search returns a count; the host reads identities back by index and asks
// for whichever fields it is about to draw. A list view asks for three fields
// per visible row and nothing for the rows it is not showing.
// ---------------------------------------------------------------------------

/// Creates an empty library.
///
/// # Safety
///
/// `out_collection` must be a valid, writable pointer to a single pointer.
#[no_mangle]
pub unsafe extern "C" fn prv_collection_create(out_collection: *mut *mut Collection) -> i32 {
    guarded_try(|| {
        // SAFETY: the caller's documented contract.
        let slot = unsafe { as_mut(out_collection) }?;
        *slot = core::ptr::null_mut();
        *slot = Box::into_raw(Box::new(Collection::new()));
        Ok(())
    })
    .code()
}

/// Destroys a library. Null is accepted and does nothing.
///
/// # Safety
///
/// `collection` must come from [`prv_collection_create`] and not yet be
/// destroyed.
#[no_mangle]
pub unsafe extern "C" fn prv_collection_destroy(collection: *mut Collection) {
    if collection.is_null() {
        return;
    }
    let _ = crate::guard::guarded(|| {
        // SAFETY: the caller's documented contract.
        drop(unsafe { Box::from_raw(collection) });
        Status::Ok
    });
}

/// Reads a NUL-terminated C string into a Rust one.
///
/// Returns [`Status::NullPointer`] for null and [`Status::InvalidArgument`] for
/// bytes that are not UTF-8 — refused rather than replaced, because a title
/// silently rewritten with replacement characters is a title the user cannot
/// search for and cannot see why.
///
/// # Safety
///
/// `text` must be null or point to a NUL-terminated string.
unsafe fn borrow_str<'a>(text: *const core::ffi::c_char) -> Result<&'a str, Status> {
    if text.is_null() {
        return Err(Status::NullPointer);
    }
    // SAFETY: the caller promises a NUL-terminated string.
    let raw = unsafe { core::ffi::CStr::from_ptr(text) };
    raw.to_str().map_err(|_| Status::InvalidArgument)
}

/// Adds a track.
///
/// # Safety
///
/// `collection` must be live and every string NUL-terminated.
#[no_mangle]
#[allow(
    clippy::too_many_arguments,
    reason = "a track is what a file told us about itself; a repr(C) struct \
              would be a permanent layout promise about a type that exists to grow"
)]
pub unsafe extern "C" fn prv_collection_add(
    collection: *mut Collection,
    id: u64,
    title: *const core::ffi::c_char,
    artist: *const core::ffi::c_char,
    album: *const core::ffi::c_char,
    media: *const core::ffi::c_char,
    duration: i64,
    imported_at_micros: i64,
) -> i32 {
    guarded_try(|| {
        // SAFETY: the caller's documented contract, for the handle and each
        // string.
        unsafe {
            as_mut(collection)?.add(
                id,
                borrow_str(title)?,
                borrow_str(artist)?,
                borrow_str(album)?,
                borrow_str(media)?,
                duration,
                imported_at_micros,
            )
        }
    })
    .code()
}

/// Changes a track's title, artist and album.
///
/// # Safety
///
/// `collection` must be live and every string NUL-terminated.
#[no_mangle]
pub unsafe extern "C" fn prv_collection_update_metadata(
    collection: *mut Collection,
    id: u64,
    title: *const core::ffi::c_char,
    artist: *const core::ffi::c_char,
    album: *const core::ffi::c_char,
) -> i32 {
    guarded_try(|| {
        // SAFETY: the caller's documented contract.
        unsafe {
            as_mut(collection)?.update_metadata(
                id,
                borrow_str(title)?,
                borrow_str(artist)?,
                borrow_str(album)?,
            )
        }
    })
    .code()
}

/// Hides a track without destroying it.
///
/// Its rating, tags and play count survive, and [`prv_collection_restore`]
/// brings it back. Repeating the call succeeds and changes nothing.
///
/// # Safety
///
/// `collection` must be live.
#[no_mangle]
pub unsafe extern "C" fn prv_collection_remove(collection: *mut Collection, id: u64) -> i32 {
    guarded_try(|| {
        // SAFETY: the caller's documented contract.
        unsafe { as_mut(collection) }?.remove(id)
    })
    .code()
}

/// Brings a removed track back, with everything it had.
///
/// # Safety
///
/// `collection` must be live.
#[no_mangle]
pub unsafe extern "C" fn prv_collection_restore(collection: *mut Collection, id: u64) -> i32 {
    guarded_try(|| {
        // SAFETY: the caller's documented contract.
        unsafe { as_mut(collection) }?.restore(id)
    })
    .code()
}

/// How many tracks the library holds.
///
/// # Safety
///
/// `collection` must be live and `out_count` writable.
#[no_mangle]
pub unsafe extern "C" fn prv_collection_count(
    collection: *const Collection,
    out_count: *mut u64,
) -> i32 {
    guarded_try(|| {
        // SAFETY: the caller's documented contract.
        let count = unsafe { as_ref(collection) }?.len();
        // SAFETY: as above.
        *unsafe { as_mut(out_count) }? = count;
        Ok(())
    })
    .code()
}

/// Runs a search and keeps the result for reading back.
///
/// An empty `text` matches everything, which is what a list view showing the
/// whole library asks for.
///
/// # Safety
///
/// `collection` must be live, `text` NUL-terminated, `out_count` writable.
#[no_mangle]
pub unsafe extern "C" fn prv_collection_search(
    collection: *mut Collection,
    text: *const core::ffi::c_char,
    sort: i32,
    descending: i32,
    out_count: *mut u64,
) -> i32 {
    guarded_try(|| {
        // SAFETY: the caller's documented contract.
        let found = unsafe {
            let text = borrow_str(text)?;
            as_mut(collection)?.search(text, sort, descending != 0)?
        };
        // SAFETY: as above.
        *unsafe { as_mut(out_count) }? = found;
        Ok(())
    })
    .code()
}

/// The identity of one search result.
///
/// # Safety
///
/// `collection` must be live and `out_id` writable.
#[no_mangle]
pub unsafe extern "C" fn prv_collection_result(
    collection: *const Collection,
    index: u64,
    out_id: *mut u64,
) -> i32 {
    guarded_try(|| {
        // SAFETY: the caller's documented contract.
        let id = unsafe { as_ref(collection) }?.result(index)?;
        // SAFETY: as above.
        *unsafe { as_mut(out_id) }? = id;
        Ok(())
    })
    .code()
}

/// Copies one text field of a track into a caller-owned buffer.
///
/// Writes to `out_needed` how many bytes the field requires including its
/// terminator, whether or not it fitted. A caller whose buffer was too small can
/// therefore allocate exactly and call once more, rather than guessing upward.
///
/// The string is always NUL-terminated when it fits, including when it is empty.
///
/// # Safety
///
/// `collection` must be live, `into` writable for `capacity` bytes, and
/// `out_needed` writable.
#[no_mangle]
pub unsafe extern "C" fn prv_collection_text_field(
    collection: *const Collection,
    id: u64,
    field: i32,
    into: *mut u8,
    capacity: u64,
    out_needed: *mut u64,
) -> i32 {
    guarded_try(|| {
        // SAFETY: the caller's documented contract.
        let collection = unsafe { as_ref(collection) }?;
        let capacity = usize::try_from(capacity).map_err(|_| Status::InvalidArgument)?;
        if into.is_null() && capacity != 0 {
            return Err(Status::NullPointer);
        }

        // A zero capacity is how a caller asks "how big is this" without
        // providing anywhere to put it, so an empty slice is the honest
        // representation rather than a dangling one.
        let buffer: &mut [u8] = if capacity == 0 {
            &mut []
        } else {
            // SAFETY: the caller promises `into` is writable for `capacity`
            // bytes; the slice is used only within this call.
            unsafe { core::slice::from_raw_parts_mut(into, capacity) }
        };

        let needed = collection.text_field(id, field, buffer)?;
        // SAFETY: the caller's documented contract.
        *unsafe { as_mut(out_needed) }? = needed.try_into().unwrap_or(u64::MAX);
        if needed > capacity {
            return Err(Status::BufferTooSmall);
        }
        Ok(())
    })
    .code()
}

/// A track's length in frames.
///
/// # Safety
///
/// `collection` must be live and `out_duration` writable.
#[no_mangle]
pub unsafe extern "C" fn prv_collection_duration(
    collection: *const Collection,
    id: u64,
    out_duration: *mut i64,
) -> i32 {
    guarded_try(|| {
        // SAFETY: the caller's documented contract.
        let duration = unsafe { as_ref(collection) }?.duration(id)?;
        // SAFETY: as above.
        *unsafe { as_mut(out_duration) }? = duration;
        Ok(())
    })
    .code()
}

// ---------------------------------------------------------------------------
// Delivery
//
// The core writes no files. It answers what has to happen to a master before it
// can go where it is going; the host applies the gain, encodes and writes.
// ---------------------------------------------------------------------------

/// Judges a rendered master against a delivery target.
///
/// The analysis is of the rendered mix, so the number gating the export is the
/// number the meter showed.
///
/// # Safety
///
/// `analysis` must be live and `out_delivery` writable.
#[no_mangle]
pub unsafe extern "C" fn prv_delivery_judge(
    analysis: *const Analysis,
    target: i32,
    format: i32,
    depth: i32,
    out_delivery: *mut *mut Delivery,
) -> i32 {
    guarded_try(|| {
        // SAFETY: the caller's documented contract.
        let slot = unsafe { as_mut(out_delivery) }?;
        *slot = core::ptr::null_mut();
        // SAFETY: as above.
        let analysis = unsafe { as_ref(analysis) }?;
        let delivery = Delivery::judge(analysis, target, format, depth)?;
        *slot = Box::into_raw(Box::new(delivery));
        Ok(())
    })
    .code()
}

/// Destroys a delivery report. Null is accepted and does nothing.
///
/// # Safety
///
/// `delivery` must come from [`prv_delivery_judge`] and not yet be destroyed.
#[no_mangle]
pub unsafe extern "C" fn prv_delivery_destroy(delivery: *mut Delivery) {
    if delivery.is_null() {
        return;
    }
    let _ = crate::guard::guarded(|| {
        // SAFETY: the caller's documented contract.
        drop(unsafe { Box::from_raw(delivery) });
        Status::Ok
    });
}

/// Reads the whole verdict at once.
///
/// Everything together rather than a call per number, because a host showing a
/// gain without the resulting peak is showing half the decision, and separate
/// calls are how the other half gets forgotten.
///
/// # Safety
///
/// `delivery` must be live and every out-parameter writable.
#[no_mangle]
#[allow(
    clippy::too_many_arguments,
    reason = "the verdict is seven numbers a host shows together; splitting it \
              into seven calls is how the ones that matter get left out"
)]
pub unsafe extern "C" fn prv_delivery_verdict(
    delivery: *const Delivery,
    out_compliance: *mut i32,
    out_measured_lufs: *mut f64,
    out_measured_true_peak: *mut f64,
    out_gain_db: *mut f64,
    out_resulting_true_peak: *mut f64,
    out_headroom_db: *mut f64,
    out_needs_dither: *mut i32,
) -> i32 {
    guarded_try(|| {
        // SAFETY: the caller's documented contract.
        let delivery = unsafe { as_ref(delivery) }?;
        // SAFETY: as above, for each out-parameter.
        unsafe {
            *as_mut(out_compliance)? = delivery.compliance();
            *as_mut(out_measured_lufs)? = delivery.measured_lufs();
            *as_mut(out_measured_true_peak)? = delivery.measured_true_peak();
            *as_mut(out_gain_db)? = delivery.gain_db();
            *as_mut(out_resulting_true_peak)? = delivery.resulting_true_peak();
            *as_mut(out_headroom_db)? = delivery.headroom_db();
            *as_mut(out_needs_dither)? = i32::from(delivery.needs_dither());
        }
        Ok(())
    })
    .code()
}

/// Whether a person should look before exporting.
///
/// # Safety
///
/// `delivery` must be live and `out_needs` writable.
#[no_mangle]
pub unsafe extern "C" fn prv_delivery_needs_attention(
    delivery: *const Delivery,
    out_needs: *mut i32,
) -> i32 {
    guarded_try(|| {
        // SAFETY: the caller's documented contract.
        let needs = unsafe { as_ref(delivery) }?.needs_attention();
        // SAFETY: as above.
        *unsafe { as_mut(out_needs) }? = i32::from(needs);
        Ok(())
    })
    .code()
}

// ---------------------------------------------------------------------------
// Experience
//
// Settings and notifications together, because they are one decision: whether a
// notice is shown depends on the mode, and a host reading them separately would
// have to reimplement that rule.
// ---------------------------------------------------------------------------

/// Creates an experience: standard mode, at the desk, nothing queued.
///
/// # Safety
///
/// `out_experience` must be a valid, writable pointer to a single pointer.
#[no_mangle]
pub unsafe extern "C" fn prv_experience_create(out_experience: *mut *mut Experience) -> i32 {
    guarded_try(|| {
        // SAFETY: the caller's documented contract.
        let slot = unsafe { as_mut(out_experience) }?;
        *slot = core::ptr::null_mut();
        *slot = Box::into_raw(Box::new(Experience::new()));
        Ok(())
    })
    .code()
}

/// Destroys an experience. Null is accepted and does nothing.
///
/// # Safety
///
/// `experience` must come from [`prv_experience_create`] and not yet be
/// destroyed.
#[no_mangle]
pub unsafe extern "C" fn prv_experience_destroy(experience: *mut Experience) {
    if experience.is_null() {
        return;
    }
    let _ = crate::guard::guarded(|| {
        // SAFETY: the caller's documented contract.
        drop(unsafe { Box::from_raw(experience) });
        Status::Ok
    });
}

/// Sets the experience mode.
///
/// # Safety
///
/// `experience` must be live.
#[no_mangle]
pub unsafe extern "C" fn prv_experience_set_mode(experience: *mut Experience, mode: i32) -> i32 {
    guarded_try(|| {
        // SAFETY: the caller's documented contract.
        unsafe { as_mut(experience) }?.set_mode(mode)
    })
    .code()
}

/// Reads the experience mode.
///
/// # Safety
///
/// `experience` must be live and `out_mode` writable.
#[no_mangle]
pub unsafe extern "C" fn prv_experience_mode(
    experience: *const Experience,
    out_mode: *mut i32,
) -> i32 {
    guarded_try(|| {
        // SAFETY: the caller's documented contract.
        let mode = unsafe { as_ref(experience) }?.mode();
        // SAFETY: as above.
        *unsafe { as_mut(out_mode) }? = mode;
        Ok(())
    })
    .code()
}

/// Sets where the user's attention is.
///
/// Setting this to `PRV_ATTENTION_PERFORMING` is what stops anything that can
/// wait from appearing over a set.
///
/// # Safety
///
/// `experience` must be live.
#[no_mangle]
pub unsafe extern "C" fn prv_experience_set_attention(
    experience: *mut Experience,
    attention: i32,
) -> i32 {
    guarded_try(|| {
        // SAFETY: the caller's documented contract.
        unsafe { as_mut(experience) }?.set_attention(attention)
    })
    .code()
}

/// Reads a boolean setting.
///
/// # Safety
///
/// `experience` must be live and `out_value` writable.
#[no_mangle]
pub unsafe extern "C" fn prv_experience_flag(
    experience: *const Experience,
    setting: i32,
    out_value: *mut i32,
) -> i32 {
    guarded_try(|| {
        // SAFETY: the caller's documented contract.
        let value = unsafe { as_ref(experience) }?.flag(setting)?;
        // SAFETY: as above.
        *unsafe { as_mut(out_value) }? = i32::from(value);
        Ok(())
    })
    .code()
}

/// Sets a boolean setting.
///
/// # Safety
///
/// `experience` must be live.
#[no_mangle]
pub unsafe extern "C" fn prv_experience_set_flag(
    experience: *mut Experience,
    setting: i32,
    value: i32,
) -> i32 {
    guarded_try(|| {
        // SAFETY: the caller's documented contract.
        unsafe { as_mut(experience) }?.set_flag(setting, value != 0)
    })
    .code()
}

/// Raises a notice, and says whether it will be shown now.
///
/// Zero does not mean discarded. A notice raised while performing is held and
/// comes back from [`prv_experience_release`].
///
/// # Safety
///
/// `experience` must be live and `out_shown` writable.
#[no_mangle]
pub unsafe extern "C" fn prv_experience_raise(
    experience: *mut Experience,
    notice: i32,
    out_shown: *mut i32,
) -> i32 {
    guarded_try(|| {
        // SAFETY: the caller's documented contract.
        let shown = unsafe { as_mut(experience) }?.raise(notice)?;
        // SAFETY: as above.
        *unsafe { as_mut(out_shown) }? = i32::from(shown);
        Ok(())
    })
    .code()
}

/// Hands over everything held back during a performance.
///
/// # Safety
///
/// `experience` must be live and `out_count` writable.
#[no_mangle]
pub unsafe extern "C" fn prv_experience_release(
    experience: *mut Experience,
    out_count: *mut u64,
) -> i32 {
    guarded_try(|| {
        // SAFETY: the caller's documented contract.
        let count = unsafe { as_mut(experience) }?.release();
        // SAFETY: as above.
        *unsafe { as_mut(out_count) }? = count;
        Ok(())
    })
    .code()
}

/// One released notice: which it was, and how many times it happened.
///
/// The count matters. Six identical warnings during a set are one problem that
/// happened six times, and six dialogues afterwards would be the notification
/// doing more damage than the fault.
///
/// # Safety
///
/// `experience` must be live and both out-parameters writable.
#[no_mangle]
pub unsafe extern "C" fn prv_experience_released(
    experience: *const Experience,
    index: u64,
    out_notice: *mut i32,
    out_occurrences: *mut u32,
) -> i32 {
    guarded_try(|| {
        // SAFETY: the caller's documented contract.
        let (notice, occurrences) = unsafe { as_ref(experience) }?.released(index)?;
        // SAFETY: as above.
        unsafe {
            *as_mut(out_notice)? = notice;
            *as_mut(out_occurrences)? = occurrences;
        }
        Ok(())
    })
    .code()
}

/// Whether anything is waiting to be shown.
///
/// # Safety
///
/// `experience` must be live and `out_waiting` writable.
#[no_mangle]
pub unsafe extern "C" fn prv_experience_has_waiting(
    experience: *const Experience,
    out_waiting: *mut i32,
) -> i32 {
    guarded_try(|| {
        // SAFETY: the caller's documented contract.
        let waiting = unsafe { as_ref(experience) }?.has_waiting();
        // SAFETY: as above.
        *unsafe { as_mut(out_waiting) }? = i32::from(waiting);
        Ok(())
    })
    .code()
}

/// Whether a notice concerns the sound happening right now.
///
/// The one class that may interrupt a performance: a performer not told the
/// right deck is silent finds out from the room.
///
/// # Safety
///
/// `out_concerns` must be writable.
#[no_mangle]
pub unsafe extern "C" fn prv_notice_concerns_the_sound(notice: i32, out_concerns: *mut i32) -> i32 {
    guarded_try(|| {
        let concerns = Experience::concerns_the_sound(notice)?;
        // SAFETY: the caller's documented contract.
        *unsafe { as_mut(out_concerns) }? = i32::from(concerns);
        Ok(())
    })
    .code()
}
