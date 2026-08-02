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
use crate::engine::{Engine, ReadAudio};
use crate::guard::{as_mut, as_ref, guarded_try};
use crate::mapping::event_from_code;
use crate::planning::Planner;
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
