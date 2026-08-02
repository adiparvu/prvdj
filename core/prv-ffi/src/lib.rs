//! The C ABI boundary: the portable core, callable from any host language.
//!
//! # What this crate is for
//!
//! Everything else in `core/` decides things. Nothing in `core/` *does*
//! anything, because ADR-0001 puts every file, socket, device and framework
//! outside it. This crate is the seam those two facts meet at: it is the only
//! place in the core that speaks a language other than Rust, and it is the whole
//! of what a host has to understand.
//!
//! The architecture overview states three requirements for this seam, and the
//! crate is arranged around them:
//!
//! - **Calls are coarse-grained.** One opaque handle, not a handle per
//!   subsystem. See [`engine`].
//! - **Calls are versioned.** A host asks before it does anything else. See
//!   [`abi`].
//! - **Audio crosses as pointers into preallocated buffers, never copied per
//!   frame.** The render entry point takes the host's block and fills it; the
//!   source is a callback into the host's own memory. See
//!   [`exports::prv_engine_render`].
//!
//! # Unsafe code lives here, and this is why
//!
//! The workspace denies `unsafe_code`. Two crates are excepted, and both
//! exceptions are the same shape: a small, audited surface where Rust's
//! guarantees genuinely end.
//!
//! In `prv-rt` it is the wait-free structures ADR-0002 rests on. Here it is the
//! fact that C has no borrow checker: a host hands over a pointer and a promise,
//! and there is no mechanism anywhere that can check the promise. Every `unsafe`
//! block in this crate is one of exactly three things — turning a host pointer
//! into a reference after a null check, building a slice from a host pointer and
//! a length the host stated, or calling back into host code — and each one
//! carries a `SAFETY` comment naming the caller-side contract that makes it
//! sound.
//!
//! The confinement is what makes that auditable. Nothing else in the core needs
//! reading to know whether the boundary is safe.
//!
//! # Three rules a host must follow
//!
//! Stated here as well as in the header, because a host author reads whichever
//! they find first.
//!
//! 1. **Check the version before anything else.** [`exports::prv_abi_version`],
//!    compared against the major number you compiled against. Everything below
//!    assumes you did.
//! 2. **One thread at a time per engine.** The handle is not synchronised. The
//!    intended arrangement is the one the architecture overview describes: the
//!    audio thread calls `prv_engine_render` and nothing else; every other call
//!    happens on one other thread.
//! 3. **A status is never ignored.** Out-parameters are written only on success,
//!    except where documented otherwise, and a host that reads one after a
//!    failure is reading what it put there.
//!
//! # Layout
//!
//! | Module | Question it answers |
//! |--------|--------------------|
//! | [`abi`] | Which version of this boundary is loaded? |
//! | [`status`] | What can go wrong, and how is it reported? |
//! | [`mapping`] | What integer does the core's enum have across C? |
//! | [`engine`] | What does a host hold, and what can it ask? |
//! | [`planning`] | How does a host get a set planned? |
//! | [`exports`] | The functions themselves. |
//!
//! # The header is generated
//!
//! `apple/PRVKit/Bridge/Generated/PRVBridge.h` is produced from this crate by
//! `tools/bridgegen`, and continuous integration fails if the committed copy
//! differs from a fresh generation. The architecture overview requires bindings
//! to be generated rather than hand-written, "because hand-written bindings
//! drift" — and a drifting binding does not fail to compile, it computes the
//! wrong answer.

#![allow(
    unsafe_code,
    reason = "this crate is the boundary with C, where Rust's guarantees end; \
              see the crate documentation for the three shapes unsafe takes here \
              and why the confinement is what makes them auditable"
)]

pub mod abi;
pub mod engine;
pub mod exports;
mod guard;
pub mod mapping;
pub mod planning;
pub mod status;

pub use engine::{Engine, ReadAudio, MAX_BLOCK_FRAMES, MAX_CHANNELS};
pub use planning::Planner;
pub use status::Status;

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::panic,
        clippy::unwrap_used,
        reason = "a test that cannot build its own fixture should fail loudly"
    )]

    use super::*;
    use crate::exports::{
        prv_abi_version, prv_engine_create, prv_engine_destroy, prv_engine_place_track,
        prv_engine_placement_count, prv_engine_playback_state, prv_engine_position,
        prv_engine_render, prv_engine_render_was_complete, prv_engine_seek, prv_engine_set_source,
        prv_engine_transport,
    };

    /// Frames of test tone the fake host holds.
    const TONE_FRAMES: usize = 4_096;

    /// Five minutes at 48 kHz, the rate every engine in these tests runs at.
    const TRACK: i64 = 48_000 * 300;

    /// A host-side audio source, in the shape a real one has.
    struct FakeHost {
        /// What every frame of every track reads as, so a test can tell audio
        /// from silence by looking at one sample.
        value: f32,
        /// How many frames the host will hand over before reporting the end.
        available: usize,
        /// How many times the callback ran, so a test can say the renderer
        /// actually asked rather than inventing the audio itself.
        calls: usize,
    }

    unsafe extern "C" fn read_audio(
        user_data: *mut core::ffi::c_void,
        _track: u64,
        _source_offset: i64,
        planar: *mut f32,
        channels: u32,
        capacity: u32,
        destination: u32,
        frames: u32,
    ) -> u32 {
        // SAFETY: the engine hands back exactly the pointer registered
        // alongside this callback, which the test keeps alive on its stack.
        let host = unsafe { &mut *user_data.cast::<FakeHost>() };
        host.calls += 1;

        let capacity = capacity.try_into().unwrap_or(0_usize);
        let channels = channels.try_into().unwrap_or(0_usize);
        let destination = destination.try_into().unwrap_or(0_usize);
        let wanted: usize = frames.try_into().unwrap_or(0_usize).min(host.available);

        // SAFETY: the engine documents `planar` as `channels * capacity` floats
        // and this is the buffer it just cleared.
        let samples = unsafe { core::slice::from_raw_parts_mut(planar, channels * capacity) };
        for channel in 0..channels {
            for frame in 0..wanted {
                let index = channel * capacity + destination + frame;
                if let Some(slot) = samples.get_mut(index) {
                    *slot = host.value;
                }
            }
        }
        wanted.try_into().unwrap_or(0_u32)
    }

    fn engine() -> *mut Engine {
        let mut handle: *mut Engine = core::ptr::null_mut();
        // SAFETY: `handle` is a live local.
        let status = unsafe { prv_engine_create(48_000, 2, 512, &raw mut handle) };
        assert_eq!(status, Status::Ok.code(), "the engine would not start");
        assert!(!handle.is_null());
        handle
    }

    #[test]
    fn a_host_can_start_an_engine_place_a_track_and_hear_it() {
        // The whole boundary in one test, taking the path a real host takes.
        let handle = engine();
        let mut host = FakeHost {
            value: 0.5,
            available: TONE_FRAMES,
            calls: 0,
        };

        // SAFETY: `handle` is live and `host` outlives every call below.
        let status = unsafe {
            prv_engine_set_source(
                handle,
                Some(read_audio),
                (&raw mut host).cast::<core::ffi::c_void>(),
            )
        };
        assert_eq!(status, Status::Ok.code());

        let mut placement = 0_u64;
        // SAFETY: `handle` is live and `placement` is a live local.
        let status =
            unsafe { prv_engine_place_track(handle, 7, 0, 2_048, 0, 0, 1_000, &raw mut placement) };
        assert_eq!(status, Status::Ok.code());
        assert_ne!(placement, 0, "a placement identity of zero is not a name");

        // Load, ready, play — the transport refuses to play an empty deck, and
        // that refusal is part of what is being checked.
        for event in [0_i32, 1, 4] {
            // SAFETY: `handle` is live.
            assert_eq!(
                unsafe { prv_engine_transport(handle, event) },
                Status::Ok.code(),
                "event {event} was refused"
            );
        }

        let mut block = vec![0.0_f32; 2 * 256];
        // SAFETY: `handle` is live and `block` holds 2 * 256 floats.
        let status = unsafe { prv_engine_render(handle, block.as_mut_ptr(), 2, 256) };
        assert_eq!(status, Status::Ok.code());

        assert!(
            host.calls > 0,
            "the renderer never asked the host for audio"
        );
        assert!(
            block.iter().any(|sample| *sample != 0.0),
            "the block came back silent"
        );

        // And the transport moved, because it is playing.
        let mut position = 0_i64;
        // SAFETY: `handle` is live and `position` is a live local.
        unsafe { prv_engine_position(handle, &raw mut position) };
        assert_eq!(position, 256);

        // SAFETY: `handle` came from `prv_engine_create` and is destroyed once.
        unsafe { prv_engine_destroy(handle) };
    }

    #[test]
    fn a_stopped_transport_renders_without_moving() {
        // A host that renders while stopped — which every host does, because the
        // audio device keeps asking — must not have the playhead run away.
        let handle = engine();
        let mut block = vec![0.0_f32; 2 * 128];

        for _ in 0..8 {
            // SAFETY: `handle` is live and `block` holds 2 * 128 floats.
            let status = unsafe { prv_engine_render(handle, block.as_mut_ptr(), 2, 128) };
            assert_eq!(status, Status::Ok.code());
        }

        let mut position = 0_i64;
        // SAFETY: `handle` is live and `position` is a live local.
        unsafe { prv_engine_position(handle, &raw mut position) };
        assert_eq!(position, 0, "a stopped transport advanced");

        // SAFETY: destroyed once.
        unsafe { prv_engine_destroy(handle) };
    }

    #[test]
    fn every_call_refuses_a_null_handle_rather_than_dereferencing_it() {
        // The mistake a host makes by accident. Each of these would be a
        // segmentation fault inside somebody else's process without the check.
        let mut position = 0_i64;
        let mut flag = 0_i32;
        let mut placement = 0_u64;
        let mut block = [0.0_f32; 8];

        // SAFETY: every call below is given a null handle deliberately, which is
        // the case each one is documented to reject.
        unsafe {
            assert_eq!(
                prv_engine_transport(core::ptr::null_mut(), 4),
                Status::NullPointer.code()
            );
            assert_eq!(
                prv_engine_seek(core::ptr::null_mut(), 10),
                Status::NullPointer.code()
            );
            assert_eq!(
                prv_engine_position(core::ptr::null(), &raw mut position),
                Status::NullPointer.code()
            );
            assert_eq!(
                prv_engine_playback_state(core::ptr::null(), &raw mut flag),
                Status::NullPointer.code()
            );
            assert_eq!(
                prv_engine_place_track(
                    core::ptr::null_mut(),
                    1,
                    0,
                    10,
                    0,
                    0,
                    0,
                    &raw mut placement
                ),
                Status::NullPointer.code()
            );
            assert_eq!(
                prv_engine_render(core::ptr::null_mut(), block.as_mut_ptr(), 2, 4),
                Status::NullPointer.code()
            );
            assert_eq!(
                prv_engine_render_was_complete(core::ptr::null(), &raw mut flag),
                Status::NullPointer.code()
            );
            assert_eq!(
                prv_engine_set_source(core::ptr::null_mut(), None, core::ptr::null_mut()),
                Status::NullPointer.code()
            );
        }
    }

    #[test]
    fn destroying_a_null_handle_is_a_no_op_rather_than_a_crash() {
        // What every C library that is pleasant to use does with its free
        // function, and what a host's error path will do sooner or later.
        // SAFETY: null is the one value `prv_engine_destroy` documents as safe.
        unsafe { prv_engine_destroy(core::ptr::null_mut()) };
    }

    #[test]
    fn a_failed_create_leaves_a_null_handle_rather_than_a_stale_one() {
        // A host that ignores the status still gets something it can check. If
        // the slot were left untouched it would hold whatever was on the stack.
        let mut handle: *mut Engine = 0x1234_usize as *mut Engine;
        // SAFETY: `handle` is a live local; the rate is deliberately invalid.
        let status = unsafe { prv_engine_create(0, 2, 512, &raw mut handle) };
        assert_eq!(status, Status::InvalidArgument.code());
        assert!(handle.is_null(), "a failed create left a dangling pointer");
    }

    #[test]
    fn a_shape_the_engine_cannot_serve_is_refused_at_the_door() {
        let mut handle: *mut Engine = core::ptr::null_mut();
        for (rate, channels, block) in [
            (48_000, 0, 512),
            (48_000, MAX_CHANNELS + 1, 512),
            (48_000, 2, 0),
            (48_000, 2, MAX_BLOCK_FRAMES + 1),
        ] {
            // SAFETY: `handle` is a live local.
            let status = unsafe { prv_engine_create(rate, channels, block, &raw mut handle) };
            assert_eq!(
                status,
                Status::InvalidArgument.code(),
                "({rate}, {channels}, {block}) was accepted"
            );
        }
    }

    #[test]
    fn a_render_larger_than_the_engine_was_built_for_is_refused_not_truncated() {
        // Truncating would hand the host a half-filled block, and the frames it
        // never wrote would play as whatever the buffer held before.
        let handle = engine();
        let mut block = vec![0.0_f32; 2 * 1_024];

        // SAFETY: `handle` is live and `block` is large enough for the request.
        let status = unsafe { prv_engine_render(handle, block.as_mut_ptr(), 2, 1_024) };
        assert_eq!(status, Status::InvalidArgument.code());

        // And a channel count the engine was not built for.
        // SAFETY: as above.
        let status = unsafe { prv_engine_render(handle, block.as_mut_ptr(), 1, 128) };
        assert_eq!(status, Status::InvalidArgument.code());

        // SAFETY: destroyed once.
        unsafe { prv_engine_destroy(handle) };
    }

    #[test]
    fn an_undefined_transport_code_is_refused_rather_than_applied() {
        // A newer host talking to an older library. Guessing would apply some
        // other event entirely.
        let handle = engine();
        // SAFETY: `handle` is live.
        assert_eq!(
            unsafe { prv_engine_transport(handle, 999) },
            Status::InvalidArgument.code()
        );
        // SAFETY: destroyed once.
        unsafe { prv_engine_destroy(handle) };
    }

    #[test]
    fn playing_a_deck_with_nothing_loaded_is_refused_and_says_so() {
        // The core returns these rather than ignoring them, and so does the
        // boundary. A host that silently dropped it would show a play button
        // that does nothing.
        let handle = engine();
        // SAFETY: `handle` is live. 4 is `Play`, with no `Load` before it.
        assert_eq!(
            unsafe { prv_engine_transport(handle, 4) },
            Status::InvalidState.code()
        );
        // SAFETY: destroyed once.
        unsafe { prv_engine_destroy(handle) };
    }

    #[test]
    fn a_source_that_never_registered_renders_silence_rather_than_faulting() {
        // The defined state. A host may place tracks before its decoder is
        // ready, and the answer is silence plus an incomplete report.
        let handle = engine();
        let mut placement = 0_u64;
        // SAFETY: `handle` is live and `placement` is a live local.
        unsafe { prv_engine_place_track(handle, 1, 0, 4_096, 0, 0, 1, &raw mut placement) };

        let mut block = vec![7.0_f32; 2 * 64];
        // SAFETY: `handle` is live and `block` holds 2 * 64 floats.
        let status = unsafe { prv_engine_render(handle, block.as_mut_ptr(), 2, 64) };
        assert_eq!(status, Status::Ok.code());
        assert!(
            block.iter().all(|sample| *sample == 0.0),
            "the buffer was not cleared, so the host would hear its own leftovers"
        );

        let mut complete = 1_i32;
        // SAFETY: `handle` is live and `complete` is a live local.
        unsafe { prv_engine_render_was_complete(handle, &raw mut complete) };
        assert_eq!(
            complete, 0,
            "a placement that read nothing reported complete"
        );

        // SAFETY: destroyed once.
        unsafe { prv_engine_destroy(handle) };
    }

    #[test]
    fn a_host_that_over_reports_what_it_wrote_is_not_believed() {
        // A defective host claiming it filled more than it was asked for would
        // otherwise convince the renderer that uninitialised memory is audio.
        unsafe extern "C" fn greedy(
            _user_data: *mut core::ffi::c_void,
            _track: u64,
            _offset: i64,
            _planar: *mut f32,
            _channels: u32,
            _capacity: u32,
            _destination: u32,
            frames: u32,
        ) -> u32 {
            frames.saturating_mul(4)
        }

        let handle = engine();
        // SAFETY: `handle` is live; the callback needs no context.
        unsafe { prv_engine_set_source(handle, Some(greedy), core::ptr::null_mut()) };
        let mut placement = 0_u64;
        // SAFETY: `handle` is live and `placement` is a live local.
        unsafe { prv_engine_place_track(handle, 1, 0, 4_096, 0, 0, 1, &raw mut placement) };

        let mut block = vec![0.0_f32; 2 * 64];
        // SAFETY: `handle` is live and `block` holds 2 * 64 floats.
        let status = unsafe { prv_engine_render(handle, block.as_mut_ptr(), 2, 64) };
        assert_eq!(
            status,
            Status::Ok.code(),
            "an over-reporting host brought the render down"
        );

        // SAFETY: destroyed once.
        unsafe { prv_engine_destroy(handle) };
    }

    #[test]
    fn the_whole_product_in_one_test_library_to_plan_to_timeline_to_audio() {
        // The loop the product exists for, entirely across the boundary: a host
        // describes its library, asks for a set, gets one, applies it to the
        // project, and hears it.
        use crate::exports::{
            prv_planner_add_candidate, prv_planner_apply, prv_planner_create, prv_planner_destroy,
            prv_planner_plan, prv_planner_track_count,
        };

        let handle = engine();
        let mut host = FakeHost {
            value: 0.4,
            available: TONE_FRAMES,
            calls: 0,
        };
        // SAFETY: `handle` is live and `host` outlives every call below.
        unsafe {
            prv_engine_set_source(
                handle,
                Some(read_audio),
                (&raw mut host).cast::<core::ffi::c_void>(),
            )
        };

        let mut planner: *mut crate::Planner = core::ptr::null_mut();
        // SAFETY: `planner` is a live local.
        assert_eq!(
            unsafe { prv_planner_create(&raw mut planner) },
            Status::Ok.code()
        );

        // Five minutes each, all mutually compatible.
        for index in 0..12_u64 {
            let semitones = match index % 3 {
                0 => 9,
                1 => 4,
                _ => 2,
            };
            // SAFETY: `planner` is live.
            let status = unsafe {
                prv_planner_add_candidate(
                    planner, index, TRACK, 128.0, 0.6, semitones, 1, 1.0, -8.0, 0,
                )
            };
            assert_eq!(status, Status::Ok.code(), "candidate {index} was refused");
        }

        let mut alternatives = 0_u64;
        // SAFETY: `planner` is live and `alternatives` is a live local.
        let status = unsafe {
            prv_planner_plan(
                planner,
                TRACK * 6,
                48_000,
                2,
                1,
                0.0,
                0.0,
                &raw mut alternatives,
            )
        };
        assert_eq!(status, Status::Ok.code(), "the library would not plan");
        assert!(alternatives >= 1);

        let mut tracks = 0_u64;
        // SAFETY: both pointers are live.
        unsafe { prv_planner_track_count(planner, &raw mut tracks) };
        assert!(tracks >= 2, "a set of one track is not a set");

        // SAFETY: both handles are live.
        let status = unsafe { prv_planner_apply(planner, handle, 1_000) };
        assert_eq!(status, Status::Ok.code(), "the plan would not apply");

        // The project now holds the set, as ordinary placements.
        let mut placements = 0_u64;
        // SAFETY: `handle` is live and `placements` is a live local.
        unsafe { prv_engine_placement_count(handle, &raw mut placements) };
        assert_eq!(
            placements, tracks,
            "the project does not hold the set that was planned"
        );

        // And it plays.
        for event in [0_i32, 1, 4] {
            // SAFETY: `handle` is live.
            assert_eq!(
                unsafe { prv_engine_transport(handle, event) },
                Status::Ok.code()
            );
        }
        let mut block = vec![0.0_f32; 2 * 256];
        // SAFETY: `handle` is live and `block` holds 2 * 256 floats.
        let status = unsafe { prv_engine_render(handle, block.as_mut_ptr(), 2, 256) };
        assert_eq!(status, Status::Ok.code());
        assert!(
            block.iter().any(|sample| *sample != 0.0),
            "a planned, applied set rendered silence"
        );

        // SAFETY: each handle is destroyed exactly once.
        unsafe {
            prv_planner_destroy(planner);
            prv_engine_destroy(handle);
        }
    }

    #[test]
    fn applying_a_plan_twice_does_not_reuse_a_placement_identity() {
        // ADR-0003: an identity is never handed out twice. Reusing one would
        // make the merge drop the second set as "already present", and the user
        // would see a successful edit with half the work missing.
        use crate::exports::{
            prv_planner_add_candidate, prv_planner_apply, prv_planner_create, prv_planner_destroy,
            prv_planner_plan,
        };

        let handle = engine();
        let mut planner: *mut crate::Planner = core::ptr::null_mut();
        // SAFETY: `planner` is a live local.
        unsafe { prv_planner_create(&raw mut planner) };

        for index in 0..8_u64 {
            // SAFETY: `planner` is live.
            unsafe {
                prv_planner_add_candidate(planner, index, TRACK, 128.0, 0.6, 9, 1, 1.0, -8.0, 0)
            };
        }
        let mut alternatives = 0_u64;
        // SAFETY: both pointers are live.
        unsafe {
            prv_planner_plan(
                planner,
                TRACK * 4,
                48_000,
                2,
                1,
                0.0,
                0.0,
                &raw mut alternatives,
            )
        };

        let mut after_first = 0_u64;
        // SAFETY: both handles are live.
        unsafe {
            prv_planner_apply(planner, handle, 1_000);
            prv_engine_placement_count(handle, &raw mut after_first);
        }

        let mut after_second = 0_u64;
        // SAFETY: as above.
        unsafe {
            prv_planner_apply(planner, handle, 2_000);
            prv_engine_placement_count(handle, &raw mut after_second);
        }

        assert_eq!(
            after_second,
            after_first * 2,
            "applying the same plan twice reused identities and lost placements"
        );

        // SAFETY: each handle is destroyed exactly once.
        unsafe {
            prv_planner_destroy(planner);
            prv_engine_destroy(handle);
        }
    }

    #[test]
    fn the_version_is_the_first_thing_that_works() {
        // Callable before anything is created, which is the point: a host checks
        // it before it has a handle to get wrong.
        assert_eq!(prv_abi_version(), abi::version());
        assert_ne!(prv_abi_version(), 0);
    }

    #[test]
    fn every_status_code_has_a_message_and_so_does_one_that_does_not_exist() {
        for status in Status::ALL {
            let pointer = exports::prv_status_message(status.code());
            assert!(!pointer.is_null());
        }
        assert!(
            !exports::prv_status_message(9_999).is_null(),
            "a host reporting an unknown code would have to null-check the explanation"
        );
    }
}
