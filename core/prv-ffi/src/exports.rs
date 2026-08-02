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
use crate::engine::{Engine, ReadAudio};
use crate::guard::{as_mut, as_ref, guarded_try};
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
