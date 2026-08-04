//! Generates the C header for the boundary, from the boundary.
//!
//! # Why this exists
//!
//! The architecture overview says bindings "are never hand-written, because
//! hand-written bindings drift". A drifting binding is a particularly nasty
//! defect: it does not fail to compile, it computes the wrong answer. A header
//! that still says `PRV_PLAYBACK_PAUSED` is 4 after the Rust side moved it to 5
//! will compile, link, run, and show the wrong thing on stage.
//!
//! So the header is generated from the same tables the library itself uses. If
//! [`prv_ffi::Status`] gains a variant, the header gains it too, because both
//! read `Status::ALL`.
//!
//! # What is still written twice, and what stops it drifting
//!
//! The *function declarations* are written out here in C, because Rust cannot
//! print its own signatures. That would be exactly the drift this tool exists to
//! prevent — so each declaration is paired with a typed reference to the real
//! function, and the type it is checked against is spelled out in full. Changing
//! a parameter in `exports.rs` and not here is a compile error in this file
//! rather than a wrong number in somebody's audio callback.
//!
//! # Two files, not one
//!
//! The header is what C reads. The module map beside it is what Swift reads —
//! `swiftc` imports a C library through a Clang module, and without a module map
//! there is no module to import. Both are binding artefacts, so both are
//! generated; a hand-written module map next to a generated header would be the
//! one file nobody remembers to update.
//!
//! # Usage
//!
//! - `bridgegen <path>` writes the header and the module map beside it.
//! - `bridgegen --check <path>` exits non-zero if either file on disk differs,
//!   which is what continuous integration runs.

#![allow(
    clippy::print_stdout,
    clippy::print_stderr,
    reason = "a command-line tool reports on the command line"
)]

use std::fmt::Write as _;
use std::process::ExitCode;

use prv_ffi::collection::{SORT_KEYS, TEXT_FIELDS};
use prv_ffi::delivery::{COMPLIANCE, DEPTHS, FORMATS, TARGETS};
use prv_ffi::experience::{notice_code, ATTENTION, MODES};
use prv_ffi::mapping::{PLAYBACK_STATES, TRANSPORT_EVENTS};
use prv_ffi::planning::{CREATIVITY_SETTINGS, ENERGY_SHAPES};
use prv_ffi::policy::{purpose_code, tier_code};
use prv_ffi::{abi, Status};

/// One exported function, as C sees it.
struct Declaration {
    /// The C text, minus the trailing semicolon.
    signature: &'static str,
    /// What the function is for, in the host author's terms.
    doc: &'static [&'static str],
}

/// Every function the header declares.
///
/// The order is the order a host meets them in: ask the version, make an engine,
/// use it, destroy it.
#[allow(
    clippy::too_many_lines,
    reason = "one entry per exported function, each a literal; splitting it would \
              scatter the boundary across several places to look"
)]
fn declarations() -> Vec<Declaration> {
    // Each entry is checked against the real function below. The casts are the
    // point of this block: they do not compile unless the signature in
    // `exports.rs` is exactly what the C text says it is.
    let _: extern "C" fn() -> u32 = prv_ffi::exports::prv_abi_version;
    let _: extern "C" fn(u32) -> i32 = prv_ffi::exports::prv_abi_is_compatible;
    let _: extern "C" fn(i32) -> *const core::ffi::c_char = prv_ffi::exports::prv_status_message;
    let _: unsafe extern "C" fn(u32, u32, u32, *mut *mut prv_ffi::Engine) -> i32 =
        prv_ffi::exports::prv_engine_create;
    let _: unsafe extern "C" fn(*mut prv_ffi::Engine) = prv_ffi::exports::prv_engine_destroy;
    let _: unsafe extern "C" fn(
        *mut prv_ffi::Engine,
        Option<prv_ffi::ReadAudio>,
        *mut core::ffi::c_void,
    ) -> i32 = prv_ffi::exports::prv_engine_set_source;
    let _: unsafe extern "C" fn(*mut prv_ffi::Engine, i32) -> i32 =
        prv_ffi::exports::prv_engine_transport;
    let _: unsafe extern "C" fn(*mut prv_ffi::Engine, i64) -> i32 =
        prv_ffi::exports::prv_engine_seek;
    let _: unsafe extern "C" fn(*const prv_ffi::Engine, *mut i64) -> i32 =
        prv_ffi::exports::prv_engine_position;
    let _: unsafe extern "C" fn(*const prv_ffi::Engine, *mut i32) -> i32 =
        prv_ffi::exports::prv_engine_playback_state;
    let _: unsafe extern "C" fn(
        *mut prv_ffi::Engine,
        u64,
        i64,
        i64,
        i64,
        u32,
        i64,
        *mut u64,
    ) -> i32 = prv_ffi::exports::prv_engine_place_track;
    let _: unsafe extern "C" fn(*const prv_ffi::Engine, *mut i64) -> i32 =
        prv_ffi::exports::prv_engine_duration;
    let _: unsafe extern "C" fn(*const prv_ffi::Engine, *mut u64) -> i32 =
        prv_ffi::exports::prv_engine_placement_count;
    let _: unsafe extern "C" fn(*mut prv_ffi::Engine, *mut f32, u32, u32) -> i32 =
        prv_ffi::exports::prv_engine_render;
    let _: unsafe extern "C" fn(*const prv_ffi::Engine, *mut i32) -> i32 =
        prv_ffi::exports::prv_engine_render_was_complete;
    let _: unsafe extern "C" fn(*mut prv_ffi::Engine, u64) -> i32 =
        prv_ffi::exports::prv_engine_set_device;
    let _: unsafe extern "C" fn(*const prv_ffi::Engine, *mut u8, u64, *mut u64) -> i32 =
        prv_ffi::exports::prv_engine_sync_state;
    let _: unsafe extern "C" fn(*mut prv_ffi::Engine, *const u8, u64, *mut u64) -> i32 =
        prv_ffi::exports::prv_engine_sync_prepare;
    let _: unsafe extern "C" fn(*const prv_ffi::Engine, *mut u8, u64, *mut u64) -> i32 =
        prv_ffi::exports::prv_engine_sync_outbound;
    let _: unsafe extern "C" fn(
        *mut prv_ffi::Engine,
        *const u8,
        u64,
        *mut u64,
        *mut u64,
        *mut u64,
        *mut u64,
    ) -> i32 = prv_ffi::exports::prv_engine_sync_merge;
    let _: unsafe extern "C" fn(
        *const prv_ffi::Engine,
        u64,
        *mut u64,
        *mut u64,
        *mut i64,
        *mut i64,
        *mut u32,
        *mut i64,
    ) -> i32 = prv_ffi::exports::prv_engine_placement;
    let _: unsafe extern "C" fn(*mut prv_ffi::Engine, u64, i64, u32, i64) -> i32 =
        prv_ffi::exports::prv_engine_move_placement;
    let _: unsafe extern "C" fn(*mut prv_ffi::Engine, u64, i64, i64) -> i32 =
        prv_ffi::exports::prv_engine_trim_placement;
    let _: unsafe extern "C" fn(*mut prv_ffi::Engine, u64, i64) -> i32 =
        prv_ffi::exports::prv_engine_remove_placement;
    let _: unsafe extern "C" fn(*mut prv_ffi::Engine, i64, *mut u64) -> i32 =
        prv_ffi::exports::prv_engine_undo;
    let _: unsafe extern "C" fn(*const prv_ffi::Engine, i64, *mut i32) -> i32 =
        prv_ffi::exports::prv_engine_undo_available;
    let _: unsafe extern "C" fn(*const prv_ffi::Engine, *mut u64) -> i32 =
        prv_ffi::exports::prv_engine_log_length;
    let _: unsafe extern "C" fn(*const prv_ffi::Engine, *mut u64) -> i32 =
        prv_ffi::exports::prv_engine_carried_count;
    let _: unsafe extern "C" fn(*mut *mut prv_ffi::Sync) -> i32 = prv_ffi::exports::prv_sync_create;
    let _: unsafe extern "C" fn(*mut prv_ffi::Sync) = prv_ffi::exports::prv_sync_destroy;
    let _: unsafe extern "C" fn(*mut prv_ffi::Sync, i32) -> i32 = prv_ffi::exports::prv_sync_apply;
    let _: unsafe extern "C" fn(*const prv_ffi::Sync, *mut i32) -> i32 =
        prv_ffi::exports::prv_sync_state;
    let _: unsafe extern "C" fn(*const prv_ffi::Sync, *mut i32, *mut i32, *mut i32) -> i32 =
        prv_ffi::exports::prv_sync_flags;
    let _: unsafe extern "C" fn(*mut prv_ffi::Sync, u64, u64) -> i32 =
        prv_ffi::exports::prv_sync_hold;
    let _: unsafe extern "C" fn(*mut prv_ffi::Sync, u64, u64) -> i32 =
        prv_ffi::exports::prv_sync_acknowledge;
    let _: unsafe extern "C" fn(*const prv_ffi::Sync, *mut u64, *mut i32) -> i32 =
        prv_ffi::exports::prv_sync_waiting;
    let _: unsafe extern "C" fn(*mut prv_ffi::Planner, u64, i32, u64, *mut u64) -> i32 =
        prv_ffi::exports::prv_planner_neighbours;
    let _: unsafe extern "C" fn(*const prv_ffi::Planner, u64, *mut u64, *mut f32, *mut i32) -> i32 =
        prv_ffi::exports::prv_planner_neighbour;
    let _: unsafe extern "C" fn(*mut prv_ffi::Engine, *mut u64) -> i32 =
        prv_ffi::exports::prv_engine_promote_carried;
    let _: unsafe extern "C" fn(*mut *mut prv_ffi::Planner) -> i32 =
        prv_ffi::exports::prv_planner_create;
    let _: unsafe extern "C" fn(*mut prv_ffi::Planner) = prv_ffi::exports::prv_planner_destroy;
    let _: unsafe extern "C" fn(
        *mut prv_ffi::Planner,
        u64,
        i64,
        f64,
        f32,
        i32,
        i32,
        f32,
        f32,
        i32,
    ) -> i32 = prv_ffi::exports::prv_planner_add_candidate;
    let _: unsafe extern "C" fn(*mut prv_ffi::Planner, u64, i64, f32, i32) -> i32 =
        prv_ffi::exports::prv_planner_add_mix_point;
    let _: unsafe extern "C" fn(*const prv_ffi::Planner, *mut u64) -> i32 =
        prv_ffi::exports::prv_planner_candidate_count;
    let _: unsafe extern "C" fn(*mut prv_ffi::Planner) -> i32 = prv_ffi::exports::prv_planner_clear;
    let _: unsafe extern "C" fn(
        *mut prv_ffi::Planner,
        i64,
        u32,
        i32,
        i32,
        f32,
        f32,
        *mut u64,
    ) -> i32 = prv_ffi::exports::prv_planner_plan;
    let _: unsafe extern "C" fn(*mut prv_ffi::Planner, u64) -> i32 =
        prv_ffi::exports::prv_planner_select;
    let _: unsafe extern "C" fn(*const prv_ffi::Planner, *mut u64) -> i32 =
        prv_ffi::exports::prv_planner_track_count;
    let _: unsafe extern "C" fn(*const prv_ffi::Planner, *mut i64) -> i32 =
        prv_ffi::exports::prv_planner_duration;
    let _: unsafe extern "C" fn(*const prv_ffi::Planner, *mut f32) -> i32 =
        prv_ffi::exports::prv_planner_score;
    let _: unsafe extern "C" fn(
        *const prv_ffi::Planner,
        u64,
        *mut u64,
        *mut i64,
        *mut i64,
        *mut f32,
    ) -> i32 = prv_ffi::exports::prv_planner_track;
    let _: unsafe extern "C" fn(*const prv_ffi::Planner, *mut prv_ffi::Engine, i64) -> i32 =
        prv_ffi::exports::prv_planner_apply;
    let _: unsafe extern "C" fn(*const f32, u64, u32, *mut *mut prv_ffi::Analysis) -> i32 =
        prv_ffi::exports::prv_analysis_run;
    let _: unsafe extern "C" fn(*mut prv_ffi::Analysis) = prv_ffi::exports::prv_analysis_destroy;
    let _: unsafe extern "C" fn(*const prv_ffi::Analysis, *mut f64, *mut f32) -> i32 =
        prv_ffi::exports::prv_analysis_tempo;
    let _: unsafe extern "C" fn(*const prv_ffi::Analysis, *mut i32, *mut i32, *mut f32) -> i32 =
        prv_ffi::exports::prv_analysis_key;
    let _: unsafe extern "C" fn(*const prv_ffi::Analysis, *mut f64, *mut f64) -> i32 =
        prv_ffi::exports::prv_analysis_loudness;
    let _: unsafe extern "C" fn(*const prv_ffi::Analysis, *mut f64) -> i32 =
        prv_ffi::exports::prv_analysis_true_peak;
    let _: unsafe extern "C" fn(*const prv_ffi::Analysis, *mut f32) -> i32 =
        prv_ffi::exports::prv_analysis_energy;
    let _: unsafe extern "C" fn(*const prv_ffi::Analysis, *mut i64) -> i32 =
        prv_ffi::exports::prv_analysis_duration;
    let _: unsafe extern "C" fn(*const prv_ffi::Analysis, *mut u64) -> i32 =
        prv_ffi::exports::prv_analysis_transition_point_count;
    let _: unsafe extern "C" fn(*const prv_ffi::Analysis, u64, *mut i64, *mut f32) -> i32 =
        prv_ffi::exports::prv_analysis_transition_point;
    let _: unsafe extern "C" fn(*mut *mut prv_ffi::Policy) -> i32 =
        prv_ffi::exports::prv_policy_create;
    let _: unsafe extern "C" fn(*mut prv_ffi::Policy) = prv_ffi::exports::prv_policy_destroy;
    let _: unsafe extern "C" fn(*mut prv_ffi::Policy, i32, u64) -> i32 =
        prv_ffi::exports::prv_policy_grant;
    let _: unsafe extern "C" fn(*mut prv_ffi::Policy, i32) -> i32 =
        prv_ffi::exports::prv_policy_withdraw;
    let _: unsafe extern "C" fn(*mut prv_ffi::Policy) -> i32 =
        prv_ffi::exports::prv_policy_withdraw_all;
    let _: unsafe extern "C" fn(*const prv_ffi::Policy, i32, *mut i32) -> i32 =
        prv_ffi::exports::prv_policy_allows;
    let _: unsafe extern "C" fn(*const prv_ffi::Policy, *mut i32) -> i32 =
        prv_ffi::exports::prv_policy_anything_leaves_the_device;
    let _: unsafe extern "C" fn(i32, *mut i32) -> i32 = prv_ffi::exports::prv_purpose_sends_content;
    let _: unsafe extern "C" fn(*mut prv_ffi::Policy, i32) -> i32 =
        prv_ffi::exports::prv_policy_set_tier;
    let _: unsafe extern "C" fn(*mut prv_ffi::Policy) -> i32 = prv_ffi::exports::prv_policy_expire;
    let _: unsafe extern "C" fn(*const prv_ffi::Policy, *mut i32) -> i32 =
        prv_ffi::exports::prv_policy_tier;
    let _: unsafe extern "C" fn(*const prv_ffi::Policy, i32, *mut i32) -> i32 =
        prv_ffi::exports::prv_policy_feature_allowed;
    let _: unsafe extern "C" fn(*const prv_ffi::Policy, i32, *mut i32) -> i32 =
        prv_ffi::exports::prv_policy_feature_is_essential;
    let _: unsafe extern "C" fn(*mut *mut prv_ffi::Collection) -> i32 =
        prv_ffi::exports::prv_collection_create;
    let _: unsafe extern "C" fn(*mut prv_ffi::Collection) =
        prv_ffi::exports::prv_collection_destroy;
    let _: unsafe extern "C" fn(
        *mut prv_ffi::Collection,
        u64,
        *const core::ffi::c_char,
        *const core::ffi::c_char,
        *const core::ffi::c_char,
        *const core::ffi::c_char,
        i64,
        i64,
    ) -> i32 = prv_ffi::exports::prv_collection_add;
    let _: unsafe extern "C" fn(
        *mut prv_ffi::Collection,
        u64,
        *const core::ffi::c_char,
        *const core::ffi::c_char,
        *const core::ffi::c_char,
    ) -> i32 = prv_ffi::exports::prv_collection_update_metadata;
    let _: unsafe extern "C" fn(*mut prv_ffi::Collection, u64) -> i32 =
        prv_ffi::exports::prv_collection_remove;
    let _: unsafe extern "C" fn(*mut prv_ffi::Collection, u64) -> i32 =
        prv_ffi::exports::prv_collection_restore;
    let _: unsafe extern "C" fn(*const prv_ffi::Collection, *mut u64) -> i32 =
        prv_ffi::exports::prv_collection_count;
    let _: unsafe extern "C" fn(
        *mut prv_ffi::Collection,
        *const core::ffi::c_char,
        i32,
        i32,
        *mut u64,
    ) -> i32 = prv_ffi::exports::prv_collection_search;
    let _: unsafe extern "C" fn(*const prv_ffi::Collection, u64, *mut u64) -> i32 =
        prv_ffi::exports::prv_collection_result;
    let _: unsafe extern "C" fn(
        *const prv_ffi::Collection,
        u64,
        i32,
        *mut u8,
        u64,
        *mut u64,
    ) -> i32 = prv_ffi::exports::prv_collection_text_field;
    let _: unsafe extern "C" fn(*const prv_ffi::Collection, u64, *mut i64) -> i32 =
        prv_ffi::exports::prv_collection_duration;
    let _: unsafe extern "C" fn(
        *const prv_ffi::Analysis,
        i32,
        i32,
        i32,
        *mut *mut prv_ffi::Delivery,
    ) -> i32 = prv_ffi::exports::prv_delivery_judge;
    let _: unsafe extern "C" fn(*mut prv_ffi::Delivery) = prv_ffi::exports::prv_delivery_destroy;
    let _: unsafe extern "C" fn(
        *const prv_ffi::Delivery,
        *mut i32,
        *mut f64,
        *mut f64,
        *mut f64,
        *mut f64,
        *mut f64,
        *mut i32,
    ) -> i32 = prv_ffi::exports::prv_delivery_verdict;
    let _: unsafe extern "C" fn(*const prv_ffi::Delivery, *mut i32) -> i32 =
        prv_ffi::exports::prv_delivery_needs_attention;
    let _: unsafe extern "C" fn(*mut *mut prv_ffi::Experience) -> i32 =
        prv_ffi::exports::prv_experience_create;
    let _: unsafe extern "C" fn(*mut prv_ffi::Experience) =
        prv_ffi::exports::prv_experience_destroy;
    let _: unsafe extern "C" fn(*mut prv_ffi::Experience, i32) -> i32 =
        prv_ffi::exports::prv_experience_set_mode;
    let _: unsafe extern "C" fn(*const prv_ffi::Experience, *mut i32) -> i32 =
        prv_ffi::exports::prv_experience_mode;
    let _: unsafe extern "C" fn(*mut prv_ffi::Experience, i32) -> i32 =
        prv_ffi::exports::prv_experience_set_attention;
    let _: unsafe extern "C" fn(*const prv_ffi::Experience, i32, *mut i32) -> i32 =
        prv_ffi::exports::prv_experience_flag;
    let _: unsafe extern "C" fn(*mut prv_ffi::Experience, i32, i32) -> i32 =
        prv_ffi::exports::prv_experience_set_flag;
    let _: unsafe extern "C" fn(*mut prv_ffi::Experience, i32, *mut i32) -> i32 =
        prv_ffi::exports::prv_experience_raise;
    let _: unsafe extern "C" fn(*mut prv_ffi::Experience, *mut u64) -> i32 =
        prv_ffi::exports::prv_experience_release;
    let _: unsafe extern "C" fn(*const prv_ffi::Experience, u64, *mut i32, *mut u32) -> i32 =
        prv_ffi::exports::prv_experience_released;
    let _: unsafe extern "C" fn(*const prv_ffi::Experience, *mut i32) -> i32 =
        prv_ffi::exports::prv_experience_has_waiting;
    let _: unsafe extern "C" fn(i32, *mut i32) -> i32 =
        prv_ffi::exports::prv_notice_concerns_the_sound;

    vec![
        Declaration {
            signature: "uint32_t prv_abi_version(void)",
            doc: &[
                "The version of this boundary, packed as major << 16 | minor << 8 | patch.",
                "",
                "Call this first, before anything else. Compare the major field against",
                "PRV_ABI_MAJOR, which is the version this header describes.",
            ],
        },
        Declaration {
            signature: "int32_t prv_abi_is_compatible(uint32_t host_major)",
            doc: &["Non-zero if a host built against `host_major` can use this library."],
        },
        Declaration {
            signature: "const char *prv_status_message(int32_t code)",
            doc: &[
                "A static, NUL-terminated description of a status code.",
                "",
                "Never null, never freed, valid for as long as the library is loaded.",
                "An unrecognised code returns a string saying so.",
            ],
        },
        Declaration {
            signature: "int32_t prv_engine_create(uint32_t sample_rate, uint32_t channels, \
                        uint32_t max_block_frames, PrvEngine **out_engine)",
            doc: &[
                "Creates an engine.",
                "",
                "On success writes the handle to *out_engine. On failure writes NULL,",
                "so a caller that ignores the status still gets a pointer it can test.",
            ],
        },
        Declaration {
            signature: "void prv_engine_destroy(PrvEngine *engine)",
            doc: &[
                "Destroys an engine. NULL is accepted and does nothing.",
                "",
                "Passing the same handle twice is a double free.",
            ],
        },
        Declaration {
            signature: "int32_t prv_engine_set_source(PrvEngine *engine, PrvReadAudio read, \
                        void *user_data)",
            doc: &[
                "Registers where the renderer gets audio.",
                "",
                "`user_data` is opaque: never dereferenced, never copied, never freed by",
                "the core. Keep it alive until the engine is destroyed or another source",
                "replaces this one. A NULL callback detaches the source, after which the",
                "engine renders silence and reports every placement incomplete.",
            ],
        },
        Declaration {
            signature: "int32_t prv_engine_transport(PrvEngine *engine, int32_t event)",
            doc: &[
                "Applies a transport event. See PrvTransportEvent.",
                "",
                "Returns PRV_INVALID_STATE when the transition is not one the machine",
                "defines — pressing play on a deck with nothing loaded, for instance.",
                "That is a real answer, not a failure to act on.",
            ],
        },
        Declaration {
            signature: "int32_t prv_engine_seek(PrvEngine *engine, int64_t position)",
            doc: &["Moves the playhead to an absolute frame position."],
        },
        Declaration {
            signature:
                "int32_t prv_engine_position(const PrvEngine *engine, int64_t *out_position)",
            doc: &["Reads the playhead position, in frames."],
        },
        Declaration {
            signature: "int32_t prv_engine_playback_state(const PrvEngine *engine, \
                        int32_t *out_state)",
            doc: &["Reads the playback state. See PrvPlaybackState."],
        },
        Declaration {
            signature: "int32_t prv_engine_place_track(PrvEngine *engine, uint64_t track, \
                        int64_t position, int64_t length, int64_t source_offset, uint32_t lane, \
                        int64_t timestamp_micros, uint64_t *out_placement)",
            doc: &[
                "Places a track on the timeline and returns its placement identity.",
                "",
                "`source_offset` is how far into the track playback begins; zero is the",
                "start. `timestamp_micros` comes from the host because the core has no",
                "clock of its own.",
            ],
        },
        Declaration {
            signature:
                "int32_t prv_engine_duration(const PrvEngine *engine, int64_t *out_duration)",
            doc: &["Reads the project's length, in frames."],
        },
        Declaration {
            signature: "int32_t prv_engine_placement_count(const PrvEngine *engine, \
                        uint64_t *out_count)",
            doc: &["Reads how many placements the project holds."],
        },
        Declaration {
            signature: "int32_t prv_engine_render(PrvEngine *engine, float *planar, \
                        uint32_t channels, uint32_t frames)",
            doc: &[
                "Renders one block into a caller-owned buffer.",
                "",
                "`planar` points at channels * frames floats, channel-major: all of",
                "channel 0, then all of channel 1.",
                "",
                "THIS IS THE AUDIO THREAD. It allocates nothing, locks nothing and waits",
                "for nothing. It is the only function here a host may call from a",
                "realtime context, and the only one it must.",
                "",
                "A block larger than the engine was built for is refused rather than",
                "truncated: a half-filled buffer would play its own leftovers.",
            ],
        },
        Declaration {
            signature: "int32_t prv_engine_render_was_complete(const PrvEngine *engine, \
                        int32_t *out_complete)",
            doc: &[
                "Whether every placement the last render touched was read in full.",
                "",
                "Zero means some audio was missing. That is worth showing a user, but it",
                "is not an error: the render happened and what was there is correct.",
            ],
        },
        Declaration {
            signature: "int32_t prv_engine_set_device(PrvEngine *engine, uint64_t device)",
            doc: &[
                "Declares which device this is.",
                "",
                "An operation is named by a device plus a number that device allocates",
                "itself, which is what lets two machines edit the same project offline",
                "without colliding. The device part must be stable for this",
                "installation and distinct from every other — facts about the machine,",
                "which is why the host supplies them.",
                "",
                "Call it before the project holds any operations. A host that never",
                "calls it gets a default that is right for one machine and wrong for a",
                "fleet — and wrong loudly: two devices sharing an identity produce",
                "operations with the same name and different contents, which a merge",
                "reports as a conflict rather than resolving by luck.",
            ],
        },
        Declaration {
            signature: "int32_t prv_engine_sync_state(const PrvEngine *engine, uint8_t *into, \
                        uint64_t capacity, uint64_t *out_needed)",
            doc: &[
                "Writes what this project has seen, for a peer to answer.",
                "",
                "The core never opens a socket. It produces bytes and reads bytes; the",
                "host carries them, over whatever it likes.",
                "",
                "Pass NULL and a capacity of zero to learn the size, then allocate once.",
                "out_needed is written whether or not the buffer fitted.",
            ],
        },
        Declaration {
            signature: "int32_t prv_engine_sync_prepare(PrvEngine *engine, \
                        const uint8_t *peer_state, uint64_t peer_state_len, \
                        uint64_t *out_needed)",
            doc: &[
                "Prepares the operations a peer has not seen, and reports their size.",
                "",
                "Nothing is copied out here: the message is built once and held, so a",
                "host learns the exact size before allocating. prv_engine_sync_outbound",
                "then hands it over, and may be called again if the buffer was too",
                "small.",
            ],
        },
        Declaration {
            signature: "int32_t prv_engine_sync_outbound(const PrvEngine *engine, uint8_t *into, \
                        uint64_t capacity, uint64_t *out_needed)",
            doc: &["Copies out the message prv_engine_sync_prepare built."],
        },
        Declaration {
            signature: "int32_t prv_engine_sync_merge(PrvEngine *engine, const uint8_t *bytes, \
                        uint64_t len, uint64_t *out_applied, uint64_t *out_already_present, \
                        uint64_t *out_conflicts, uint64_t *out_carried)",
            doc: &[
                "Merges a message from a peer.",
                "",
                "The four counts mean four different things: work arrived, work was",
                "already here, work disagrees and needs a person, and work could not be",
                "read because a newer build made it.",
                "",
                "The last is not a failure. A message carrying operations this build",
                "cannot interpret can still be passed on byte for byte, so a device on",
                "an older version relays rather than blocking. What it cannot do is",
                "show them, so tell the user that some of the project was made with a",
                "newer version of the app.",
            ],
        },
        Declaration {
            signature: "int32_t prv_engine_move_placement(PrvEngine *engine, uint64_t placement, \
                        int64_t position, uint32_t lane, int64_t timestamp_micros)",
            doc: &[
                "Moves a clip.",
                "",
                "Every edit is an operation on the log, so undo, version history,",
                "comparison and synchronisation all work on it without anything further",
                "being written.",
            ],
        },
        Declaration {
            signature: "int32_t prv_engine_trim_placement(PrvEngine *engine, uint64_t placement, \
                        int64_t length, int64_t timestamp_micros)",
            doc: &["Changes how long a clip plays for."],
        },
        Declaration {
            signature: "int32_t prv_engine_remove_placement(PrvEngine *engine, \
                        uint64_t placement, int64_t timestamp_micros)",
            doc: &[
                "Takes a clip off the timeline.",
                "",
                "Nothing is deleted: the operation that placed it stays in the log, so",
                "undo restores it and the history still says what happened.",
            ],
        },
        Declaration {
            signature: "int32_t prv_engine_undo(PrvEngine *engine, int64_t timestamp_micros, \
                        uint64_t *out_operations)",
            doc: &[
                "Undoes this device's last edit, writing how many operations it took.",
                "",
                "Zero means nothing happened, and there are two reasons for that.",
                "prv_engine_undo_available tells them apart.",
            ],
        },
        Declaration {
            signature: "int32_t prv_engine_undo_available(const PrvEngine *engine, \
                        int64_t timestamp_micros, int32_t *out_reason)",
            doc: &[
                "Whether undo would do anything, and why not when it would not.",
                "",
                "0: there is something to undo. 1: there is nothing. 2: another device",
                "changed the same thing afterwards, and undoing would discard their",
                "work — for which there is no correct silent answer, so it refuses.",
                "3: the core gave a reason this build of the boundary has no name for.",
                "",
                "A host shows the last two differently: one is a disabled button and",
                "the other is a sentence.",
            ],
        },
        Declaration {
            signature: "int32_t prv_engine_log_length(const PrvEngine *engine, \
                        uint64_t *out_length)",
            doc: &["How many operations the project's history holds."],
        },
        Declaration {
            signature: "int32_t prv_engine_placement(const PrvEngine *engine, uint64_t index, \
                        uint64_t *out_placement, uint64_t *out_track, int64_t *out_position, \
                        int64_t *out_length, uint32_t *out_lane, int64_t *out_source_offset)",
            doc: &[
                "Reads one clip of the timeline, by position in identity order.",
                "",
                "A timeline is drawn from these. The count and the total duration are",
                "enough to say \"four tracks, thirty-eight minutes\" and not enough to",
                "draw a single clip.",
                "",
                "Identity order rather than time order, deliberately: it is stable while",
                "a user drags a clip, and a list that reordered itself under the hand",
                "doing the dragging is why timelines flicker. Sort by out_position for",
                "time order.",
            ],
        },
        Declaration {
            signature: "int32_t prv_engine_carried_count(const PrvEngine *engine, \
                        uint64_t *out_count)",
            doc: &[
                "How many operations this project holds that a newer build made.",
                "",
                "Non-zero means part of the project was made with a newer version of",
                "the application. It is kept and passed on to other devices, and it",
                "cannot be shown here — worth telling the person at the screen, because",
                "otherwise the project silently appears to be missing work.",
            ],
        },
        Declaration {
            signature: "int32_t prv_engine_promote_carried(PrvEngine *engine, \
                        uint64_t *out_promoted)",
            doc: &[
                "Re-reads carried operations, keeping the ones this build now",
                "understands.",
                "",
                "What an upgrade is for. Work that arrived from a newer version and",
                "could only be carried becomes part of the project the moment this build",
                "learns its meaning — the same bytes the author wrote, not a",
                "reconstruction of them.",
                "",
                "Cheap when there is nothing to do. Call it after opening a project.",
            ],
        },
        Declaration {
            signature: "int32_t prv_planner_create(PrvPlanner **out_planner)",
            doc: &[
                "Creates a planner.",
                "",
                "A separate handle from the engine, deliberately. A library and a plan",
                "are not a project: a host may plan with no project open, and may keep",
                "one open while replanning.",
            ],
        },
        Declaration {
            signature: "int32_t prv_sync_create(PrvSync **out_sync)",
            doc: &[
                "Creates a synchronisation state: offline, with nothing waiting.",
                "",
                "Offline rather than idle, because that is the state a device starts in",
                "before anything has confirmed otherwise. Assuming the optimistic one",
                "would make the first seconds of every launch a lie.",
            ],
        },
        Declaration {
            signature: "void prv_sync_destroy(PrvSync *sync)",
            doc: &["Destroys it. NULL is accepted and does nothing."],
        },
        Declaration {
            signature: "int32_t prv_sync_apply(PrvSync *sync, int32_t event_code)",
            doc: &[
                "Reports something that happened, by its PrvSyncEvent code.",
                "",
                "Total: every state and event pair has an answer, and a pair that means",
                "nothing leaves the state alone rather than failing. Events arrive from",
                "a network and from a user at the same time, so \"that cannot happen\"",
                "is a claim about timing that no amount of care makes true.",
                "",
                "A code this version does not define is refused rather than guessed at.",
            ],
        },
        Declaration {
            signature: "int32_t prv_sync_state(const PrvSync *sync, int32_t *out_state)",
            doc: &["Reads the current state, as a PrvSyncState."],
        },
        Declaration {
            signature: "int32_t prv_sync_flags(const PrvSync *sync, int32_t *out_editing_allowed, \
                        int32_t *out_transferring, int32_t *out_needs_the_user)",
            doc: &[
                "Three questions a host asks before drawing anything.",
                "",
                "out_editing_allowed is non-zero in every state, and worth asking",
                "anyway: a host that asks is a host that was considering disabling",
                "something, and the answer is that offline is the normal case rather",
                "than a mode with fewer features.",
            ],
        },
        Declaration {
            signature: "int32_t prv_sync_hold(PrvSync *sync, uint64_t device, uint64_t sequence)",
            doc: &[
                "Records that an operation was authored here and has gone nowhere yet.",
                "",
                "Refuses when full rather than discarding its oldest entry, which is the",
                "opposite of what an audit log does with the same problem: one holds a",
                "record of what happened, and this holds the work itself.",
            ],
        },
        Declaration {
            signature: "int32_t prv_sync_acknowledge(PrvSync *sync, uint64_t device, \
                        uint64_t sequence)",
            doc: &[
                "Records that an operation reached somewhere else.",
                "",
                "Acknowledging something already acknowledged does nothing, which is",
                "what makes a lost reply safe: the client sends again, and the second",
                "delivery is recognised rather than corrupting anything.",
            ],
        },
        Declaration {
            signature: "int32_t prv_sync_waiting(const PrvSync *sync, uint64_t *out_waiting, \
                        int32_t *out_nearly_full)",
            doc: &[
                "How much work is waiting to leave, and whether that is near the bound.",
                "",
                "Reaching the bound means a session has been offline for a very long",
                "time or a server has been refusing everything, and the user needs to",
                "know either way — which is why the warning exists before the refusal",
                "rather than after it.",
            ],
        },
        Declaration {
            signature: "int32_t prv_planner_neighbours(PrvPlanner *planner, uint64_t track, \
                        int32_t following, uint64_t limit, uint64_t *out_count)",
            doc: &[
                "Ranks the records that sit best after — or before — a given one.",
                "",
                "Answers \"what mixes out of this?\" without a set, which is the question",
                "a person asks at import and every time they look at a record and",
                "wonder. Different from planning: a plan judges a move against where",
                "the evening is going, and this judges the pair.",
                "",
                "following non-zero asks what comes AFTER track; zero asks what comes",
                "BEFORE. They are genuinely different lists — every component that",
                "depends on direction is measured the other way round.",
                "",
                "Records whose keys clash are not in the list at all. That is a musical",
                "fact rather than a preference, and a list that ranked unlistenable",
                "moves at the bottom would be one nobody could trust the top of.",
            ],
        },
        Declaration {
            signature: "int32_t prv_planner_neighbour(const PrvPlanner *planner, uint64_t index, \
                        uint64_t *out_track, float *out_score, int32_t *out_weakest)",
            doc: &[
                "Reads one row of the last ranking.",
                "",
                "out_weakest receives the component that costs the pairing the most. It",
                "is what an interface says out loud: a DJ told \"0.71\" learns nothing,",
                "and a DJ told \"the tempo is the hard part here\" knows what to do.",
            ],
        },
        Declaration {
            signature: "void prv_planner_destroy(PrvPlanner *planner)",
            doc: &["Destroys a planner. NULL is accepted and does nothing."],
        },
        Declaration {
            signature: "int32_t prv_planner_add_candidate(PrvPlanner *planner, uint64_t track, \
                        int64_t duration, double bpm, float energy, int32_t key_semitones, \
                        int32_t key_is_minor, float key_confidence, float loudness_lufs, \
                        int32_t has_vocals)",
            doc: &[
                "Adds one track to the library the planner chooses from.",
                "",
                "`key_confidence` at or below zero means the key is unknown. `has_vocals`",
                "is -1 for unknown, 0 for no, 1 for yes — and unknown is a different",
                "answer from no, which scores differently.",
                "",
                "Facts are arguments rather than a struct on purpose: a struct here would",
                "be a permanent layout promise, and the first field anybody wants to add",
                "next year would break every host compiled against it.",
            ],
        },
        Declaration {
            signature: "int32_t prv_planner_add_mix_point(PrvPlanner *planner, uint64_t track, \
                        int64_t position, float energy, int32_t is_exit)",
            doc: &[
                "Adds a place the analysis says a track can be left or entered.",
                "",
                "A track with no exit point is mixed out of near its end, which the",
                "planner treats as a real answer rather than a missing one.",
            ],
        },
        Declaration {
            signature: "int32_t prv_planner_candidate_count(const PrvPlanner *planner, \
                        uint64_t *out_count)",
            doc: &["Reads how many candidates the library holds."],
        },
        Declaration {
            signature: "int32_t prv_planner_clear(PrvPlanner *planner)",
            doc: &["Forgets the library and any plan made from it."],
        },
        Declaration {
            signature: "int32_t prv_planner_plan(PrvPlanner *planner, int64_t target_frames, \
                        uint32_t sample_rate, int32_t shape, int32_t creativity, \
                        float tempo_floor, float tempo_ceiling, uint64_t *out_count)",
            doc: &[
                "Plans up to three genuinely different sets.",
                "",
                "Pass zero for both tempo bounds to leave the range open; half a range is",
                "treated as no range, because honouring it would constrain the set in a",
                "way nobody asked for.",
                "",
                "Returns PRV_REFUSED when no set could be built. That is a real answer",
                "about the library — nothing in it fits — not a malfunction.",
            ],
        },
        Declaration {
            signature: "int32_t prv_planner_select(PrvPlanner *planner, uint64_t index)",
            doc: &["Chooses which alternative subsequent reads describe."],
        },
        Declaration {
            signature: "int32_t prv_planner_track_count(const PrvPlanner *planner, \
                        uint64_t *out_count)",
            doc: &["Reads how many tracks the selected plan holds."],
        },
        Declaration {
            signature: "int32_t prv_planner_duration(const PrvPlanner *planner, \
                        int64_t *out_duration)",
            doc: &["Reads how long the selected plan runs for, in frames."],
        },
        Declaration {
            signature: "int32_t prv_planner_score(const PrvPlanner *planner, float *out_score)",
            doc: &["Reads the selected plan's mean transition score, from zero to one."],
        },
        Declaration {
            signature: "int32_t prv_planner_track(const PrvPlanner *planner, uint64_t index, \
                        uint64_t *out_track, int64_t *out_start, int64_t *out_duration, \
                        float *out_score)",
            doc: &[
                "Reads one track of the selected plan.",
                "",
                "The score is the move *into* this track, and is 1.0 for the opening",
                "track, which was chosen rather than transitioned into.",
            ],
        },
        Declaration {
            signature: "int32_t prv_planner_apply(const PrvPlanner *planner, PrvEngine *engine, \
                        int64_t timestamp_micros)",
            doc: &[
                "Applies the selected plan to an engine's project.",
                "",
                "The plan becomes ordinary operations on the log — the same ones a",
                "hand-made edit produces. After this there is nothing in the document",
                "that says which placements a person made and which the planner did,",
                "which is what makes a generated mix editable rather than merely",
                "promised to be.",
            ],
        },
        Declaration {
            signature: "int32_t prv_analysis_run(const float *samples, uint64_t frames, \
                        uint32_t sample_rate, PrvAnalysis **out_analysis)",
            doc: &[
                "Analyses a track. `samples` is mono, `frames` long.",
                "",
                "The audio is borrowed for the duration of this call and never retained.",
                "",
                "NOT THE AUDIO THREAD. This allocates and takes seconds on a long track.",
                "It belongs to the background domain; calling it from a render callback",
                "would drop out.",
            ],
        },
        Declaration {
            signature: "void prv_analysis_destroy(PrvAnalysis *analysis)",
            doc: &["Destroys an analysis. NULL is accepted and does nothing."],
        },
        Declaration {
            signature: "int32_t prv_analysis_tempo(const PrvAnalysis *analysis, double *out_bpm, \
                        float *out_confidence)",
            doc: &[
                "Reads the tempo in beats per minute, and how sure the estimate is.",
                "",
                "Returns PRV_REFUSED when no pulse was found. That is the answer, not a",
                "failure: a track with no discernible tempo has none, and a guessed 120",
                "would reach the planner and a whole set would be built on it.",
            ],
        },
        Declaration {
            signature: "int32_t prv_analysis_key(const PrvAnalysis *analysis, \
                        int32_t *out_semitones, int32_t *out_is_minor, float *out_confidence)",
            doc: &["Reads the key: semitones above C, whether it is minor, and confidence."],
        },
        Declaration {
            signature: "int32_t prv_analysis_loudness(const PrvAnalysis *analysis, \
                        double *out_integrated, double *out_range)",
            doc: &["Reads the integrated loudness in LUFS and the loudness range."],
        },
        Declaration {
            signature: "int32_t prv_analysis_true_peak(const PrvAnalysis *analysis, \
                        double *out_true_peak)",
            doc: &["Reads the true peak, in decibels relative to full scale."],
        },
        Declaration {
            signature: "int32_t prv_analysis_energy(const PrvAnalysis *analysis, \
                        float *out_energy)",
            doc: &[
                "Reads the track's overall energy, from zero to one.",
                "",
                "Absent rather than defaulted when no structure was found, for the same",
                "reason the tempo is: the planner shapes a whole set around this number.",
            ],
        },
        Declaration {
            signature: "int32_t prv_analysis_duration(const PrvAnalysis *analysis, \
                        int64_t *out_frames)",
            doc: &["Reads how long the analysed audio was, in frames."],
        },
        Declaration {
            signature: "int32_t prv_analysis_transition_point_count(const PrvAnalysis *analysis, \
                        uint64_t *out_count)",
            doc: &["Reads how many places the analysis found a transition could happen."],
        },
        Declaration {
            signature: "int32_t prv_analysis_transition_point(const PrvAnalysis *analysis, \
                        uint64_t index, int64_t *out_position, float *out_energy)",
            doc: &[
                "Reads one place a transition could happen: where, and how quiet.",
                "",
                "Quieter is better to mix on, which is why the energy comes back with the",
                "position rather than needing a second call.",
            ],
        },
        Declaration {
            signature: "int32_t prv_policy_create(PrvPolicy **out_policy)",
            doc: &[
                "Creates a policy: nothing agreed to, free tier.",
                "",
                "Both are the safe end of their range. A host that never configures this",
                "can still run the whole product offline.",
            ],
        },
        Declaration {
            signature: "void prv_policy_destroy(PrvPolicy *policy)",
            doc: &["Destroys a policy. NULL is accepted and does nothing."],
        },
        Declaration {
            signature: "int32_t prv_policy_grant(PrvPolicy *policy, int32_t purpose, \
                        uint64_t ordinal)",
            doc: &[
                "Records that the user agreed to a purpose.",
                "",
                "`ordinal` identifies *which* agreement was given — a version of the",
                "wording, or a sequence. It is what makes this a consent record rather",
                "than a boolean.",
            ],
        },
        Declaration {
            signature: "int32_t prv_policy_withdraw(PrvPolicy *policy, int32_t purpose)",
            doc: &["Records that the user withdrew a purpose."],
        },
        Declaration {
            signature: "int32_t prv_policy_withdraw_all(PrvPolicy *policy)",
            doc: &[
                "Withdraws every agreement at once.",
                "",
                "One call rather than a loop in the host, because a loop in the host is",
                "one that can be interrupted half way.",
            ],
        },
        Declaration {
            signature: "int32_t prv_policy_allows(const PrvPolicy *policy, int32_t purpose, \
                        int32_t *out_allowed)",
            doc: &["Whether a purpose is currently agreed to."],
        },
        Declaration {
            signature: "int32_t prv_policy_anything_leaves_the_device(const PrvPolicy *policy, \
                        int32_t *out_leaves)",
            doc: &[
                "Whether anything at all currently leaves the device.",
                "",
                "The single question a privacy screen leads with. Composed in the core",
                "from every purpose that transmits, so a purpose added later is included",
                "without any host being changed.",
            ],
        },
        Declaration {
            signature: "int32_t prv_purpose_sends_content(int32_t purpose, int32_t *out_sends)",
            doc: &[
                "Whether a purpose sends the user's own material, or a fact about it.",
                "",
                "A different question from whether anything leaves the device: a crash",
                "report leaves and carries no music. A consent screen needs both, and one",
                "built on either alone misleads in one direction or the other.",
            ],
        },
        Declaration {
            signature: "int32_t prv_policy_set_tier(PrvPolicy *policy, int32_t tier)",
            doc: &["Sets the licence tier."],
        },
        Declaration {
            signature: "int32_t prv_policy_expire(PrvPolicy *policy)",
            doc: &[
                "Marks the licence expired.",
                "",
                "Not a lock-out. Everything essential survives.",
            ],
        },
        Declaration {
            signature: "int32_t prv_policy_tier(const PrvPolicy *policy, int32_t *out_tier)",
            doc: &["Reads the current licence tier."],
        },
        Declaration {
            signature: "int32_t prv_policy_feature_allowed(const PrvPolicy *policy, \
                        int32_t feature, int32_t *out_allowed)",
            doc: &["Whether a feature is available under the current licence."],
        },
        Declaration {
            signature: "int32_t prv_policy_feature_is_essential(const PrvPolicy *policy, \
                        int32_t feature, int32_t *out_essential)",
            doc: &[
                "Whether a feature is essential, and so present at every tier.",
                "",
                "An essential feature that is somehow unavailable is a defect, not an",
                "upsell, and a host should say so differently.",
            ],
        },
        Declaration {
            signature: "int32_t prv_collection_create(PrvCollection **out_collection)",
            doc: &["Creates an empty library."],
        },
        Declaration {
            signature: "void prv_collection_destroy(PrvCollection *collection)",
            doc: &["Destroys a library. NULL is accepted and does nothing."],
        },
        Declaration {
            signature: "int32_t prv_collection_add(PrvCollection *collection, uint64_t id, \
                        const char *title, const char *artist, const char *album, \
                        const char *media, int64_t duration, int64_t imported_at_micros)",
            doc: &[
                "Adds a track. Strings are UTF-8 and NUL-terminated.",
                "",
                "Returns PRV_REFUSED when the identity is already in use. A re-import is",
                "not a new track, and overwriting would lose whatever the user edited.",
            ],
        },
        Declaration {
            signature: "int32_t prv_collection_update_metadata(PrvCollection *collection, \
                        uint64_t id, const char *title, const char *artist, const char *album)",
            doc: &["Changes a track's title, artist and album. Its identity does not change."],
        },
        Declaration {
            signature: "int32_t prv_collection_remove(PrvCollection *collection, uint64_t id)",
            doc: &[
                "Hides a track without destroying it.",
                "",
                "Its rating, tags and play count survive, and prv_collection_restore",
                "brings it back. Repeating the call succeeds and changes nothing.",
            ],
        },
        Declaration {
            signature: "int32_t prv_collection_restore(PrvCollection *collection, uint64_t id)",
            doc: &["Brings a removed track back, with everything it had."],
        },
        Declaration {
            signature: "int32_t prv_collection_count(const PrvCollection *collection, \
                        uint64_t *out_count)",
            doc: &["How many tracks the library holds."],
        },
        Declaration {
            signature: "int32_t prv_collection_search(PrvCollection *collection, \
                        const char *text, int32_t sort, int32_t descending, uint64_t *out_count)",
            doc: &[
                "Runs a search and keeps the result for reading back.",
                "",
                "An empty `text` matches everything, which is what a list view showing",
                "the whole library asks for.",
            ],
        },
        Declaration {
            signature: "int32_t prv_collection_result(const PrvCollection *collection, \
                        uint64_t index, uint64_t *out_id)",
            doc: &["The identity of one search result."],
        },
        Declaration {
            signature: "int32_t prv_collection_text_field(const PrvCollection *collection, \
                        uint64_t id, int32_t field, uint8_t *into, uint64_t capacity, \
                        uint64_t *out_needed)",
            doc: &[
                "Copies one text field of a track into a caller-owned buffer.",
                "",
                "`out_needed` is how many bytes the field requires including its",
                "terminator, whether or not it fitted — so a caller given",
                "PRV_BUFFER_TOO_SMALL can allocate exactly and call once more rather",
                "than guessing upward. A capacity of zero asks the size and writes",
                "nothing.",
                "",
                "The result is always NUL-terminated when it fits, including when the",
                "field is empty.",
            ],
        },
        Declaration {
            signature: "int32_t prv_collection_duration(const PrvCollection *collection, \
                        uint64_t id, int64_t *out_duration)",
            doc: &["A track's length in frames."],
        },
        Declaration {
            signature: "int32_t prv_delivery_judge(const PrvAnalysis *analysis, int32_t target, \
                        int32_t format, int32_t depth, PrvDelivery **out_delivery)",
            doc: &[
                "Judges a rendered master against a delivery target.",
                "",
                "The analysis is of the rendered mix, so the number gating the export is",
                "the number the meter showed. The core writes no files: it answers what",
                "must happen to the master, and the host applies the gain and encodes.",
            ],
        },
        Declaration {
            signature: "void prv_delivery_destroy(PrvDelivery *delivery)",
            doc: &["Destroys a delivery report. NULL is accepted and does nothing."],
        },
        Declaration {
            signature: "int32_t prv_delivery_verdict(const PrvDelivery *delivery, \
                        int32_t *out_compliance, double *out_measured_lufs, \
                        double *out_measured_true_peak, double *out_gain_db, \
                        double *out_resulting_true_peak, double *out_headroom_db, \
                        int32_t *out_needs_dither)",
            doc: &[
                "Reads the whole verdict at once.",
                "",
                "Together rather than a call per number: a host showing a gain without",
                "the resulting true peak is showing half the decision, and separate calls",
                "are how the other half gets forgotten.",
                "",
                "A gain of zero on a master far below target is not an oversight. That is",
                "usually a mistake upstream — a muted lane, the wrong project — and",
                "turning it up produces a loud version of the wrong thing.",
            ],
        },
        Declaration {
            signature: "int32_t prv_delivery_needs_attention(const PrvDelivery *delivery, \
                        int32_t *out_needs)",
            doc: &["Whether a person should look before exporting."],
        },
        Declaration {
            signature: "int32_t prv_experience_create(PrvExperience **out_experience)",
            doc: &["Creates an experience: standard mode, at the desk, nothing queued."],
        },
        Declaration {
            signature: "void prv_experience_destroy(PrvExperience *experience)",
            doc: &["Destroys an experience. NULL is accepted and does nothing."],
        },
        Declaration {
            signature: "int32_t prv_experience_set_mode(PrvExperience *experience, int32_t mode)",
            doc: &["Sets the experience mode."],
        },
        Declaration {
            signature: "int32_t prv_experience_mode(const PrvExperience *experience, \
                        int32_t *out_mode)",
            doc: &["Reads the experience mode."],
        },
        Declaration {
            signature: "int32_t prv_experience_set_attention(PrvExperience *experience, \
                        int32_t attention)",
            doc: &[
                "Sets where the user's attention is.",
                "",
                "PRV_ATTENTION_PERFORMING is what stops anything that can wait from",
                "appearing over a set. A dialogue during a performance is worse than the",
                "problem it reports, almost always.",
            ],
        },
        Declaration {
            signature: "int32_t prv_experience_flag(const PrvExperience *experience, \
                        int32_t setting, int32_t *out_value)",
            doc: &["Reads a boolean setting."],
        },
        Declaration {
            signature: "int32_t prv_experience_set_flag(PrvExperience *experience, \
                        int32_t setting, int32_t value)",
            doc: &["Sets a boolean setting."],
        },
        Declaration {
            signature: "int32_t prv_experience_raise(PrvExperience *experience, int32_t notice, \
                        int32_t *out_shown)",
            doc: &[
                "Raises a notice, and says whether it will be shown now.",
                "",
                "Zero does not mean discarded. A notice raised while performing is held",
                "and comes back from prv_experience_release.",
            ],
        },
        Declaration {
            signature: "int32_t prv_experience_release(PrvExperience *experience, \
                        uint64_t *out_count)",
            doc: &["Hands over everything held back during a performance."],
        },
        Declaration {
            signature: "int32_t prv_experience_released(const PrvExperience *experience, \
                        uint64_t index, int32_t *out_notice, uint32_t *out_occurrences)",
            doc: &[
                "One released notice: which it was, and how many times it happened.",
                "",
                "The count matters. Six identical warnings during a set are one problem",
                "that happened six times, and six dialogues afterwards would be the",
                "notification doing more damage than the fault.",
            ],
        },
        Declaration {
            signature: "int32_t prv_experience_has_waiting(const PrvExperience *experience, \
                        int32_t *out_waiting)",
            doc: &["Whether anything is waiting to be shown."],
        },
        Declaration {
            signature: "int32_t prv_notice_concerns_the_sound(int32_t notice, \
                        int32_t *out_concerns)",
            doc: &[
                "Whether a notice concerns the sound happening right now.",
                "",
                "The one class that may interrupt a performance: a performer not told the",
                "right deck is silent finds out from the room.",
            ],
        },
    ]
}

/// Forces every generated enum to have a signed underlying type.
///
/// Not a value any function returns, and not one a host should ever compare
/// against. It exists so the enum's underlying type is `int32_t`, which is what
/// every function in the header actually takes.
/// Enumeration constants share one namespace in C, so each enum needs its own
/// sentinel name — three enums declaring `PRV_ENUM_FORCE_SIGNED` is a
/// redefinition error, not three private constants.
fn signed_sentinel(name: &str) -> String {
    format!(
        "    /* Not a value. Present so the underlying type is signed, matching the\n\
        \x20      int32_t every function here takes. Never returned, never compared. */\n\
        \x20   {name} = -1,\n"
    )
}

/// Emits one C enum: a comment, the members in code order, and a sentinel.
///
/// Six enums reach the header and every one of them had the same eleven lines
/// written out. Extracting it is not tidying — a sixth copy is where the
/// sentinel gets forgotten, and a signed enum that quietly became unsigned is a
/// Swift build failure a hundred lines from anything that looks related.
fn emit_enum(
    out: &mut String,
    comment: &str,
    type_name: &str,
    sentinel: &str,
    members: impl Iterator<Item = (String, i32)>,
) {
    let _ = writeln!(out, "{comment}\ntypedef enum {type_name} {{");
    for (name, code) in members {
        let _ = writeln!(out, "    {name} = {code},");
    }
    out.push_str(&signed_sentinel(sentinel));
    let _ = writeln!(out, "}} {type_name};\n");
}

/// Wraps a signature across lines the way a reader of C expects.
fn write_signature(out: &mut String, signature: &str) {
    /// Where a declaration gets too wide to read.
    const WIDTH: usize = 88;

    if signature.len() <= WIDTH {
        out.push_str(signature);
        out.push_str(";\n");
        return;
    }

    // Break after the opening parenthesis and at argument boundaries, indenting
    // continuations under the first argument the way clang-format would.
    let Some(open) = signature.find('(') else {
        out.push_str(signature);
        out.push_str(";\n");
        return;
    };
    let (head, rest) = signature.split_at(open + 1);
    let indent = " ".repeat(head.len());
    let arguments = rest.trim_end_matches(')');

    let mut line = String::from(head);
    for (index, argument) in arguments.split(", ").enumerate() {
        if index == 0 {
            line.push_str(argument);
            continue;
        }
        // The separator belongs to the line being *left*, not the one being
        // started. Getting that backwards drops the comma at every wrap, and the
        // result is a header that looks right and does not parse.
        if line.len() + argument.len() + 2 > WIDTH {
            line.push(',');
            out.push_str(&line);
            out.push('\n');
            line.clone_from(&indent);
            line.push_str(argument);
        } else {
            line.push_str(", ");
            line.push_str(argument);
        }
    }
    // The last line has no wrap to flush it, so it is pushed here. Leaving this
    // out cost the final two arguments of every wrapped declaration, and the
    // header still looked plausible.
    out.push_str(&line);
    out.push_str(");\n");
}

/// Emits every type the header declares: the enums, the opaque handles and the
/// callback.
///
/// Split from [`header`] because the two halves answer different questions — one
/// is "what can a host name", the other is "what can a host do" — and because a
/// single function that did both had grown past the point where anybody would
/// read it to the end.
fn emit_types(out: &mut String) {
    emit_enum(
        out,
        "/* The result of a call. Zero is success, and it is the only success. */",
        "PrvStatus",
        "PRV_STATUS_FORCE_SIGNED",
        Status::ALL
            .iter()
            .map(|status| (status.c_name().to_owned(), status.code())),
    );

    emit_enum(
        out,
        "/* What the transport is doing. */",
        "PrvPlaybackState",
        "PRV_PLAYBACK_FORCE_SIGNED",
        PLAYBACK_STATES
            .iter()
            .map(|(state, name)| ((*name).to_owned(), prv_ffi::mapping::playback_code(*state))),
    );

    emit_enum(
        out,
        "/* What can happen to the transport. */",
        "PrvTransportEvent",
        "PRV_EVENT_FORCE_SIGNED",
        TRANSPORT_EVENTS
            .iter()
            .enumerate()
            .map(|(index, (_, name))| ((*name).to_owned(), i32::try_from(index).unwrap_or(0))),
    );

    emit_score_enums(out);
    emit_sync_enums(out);
    emit_domain_enums(out);

    emit_enum(
        out,
        "/* How a list of tracks is ordered. */",
        "PrvSortKey",
        "PRV_SORT_FORCE_SIGNED",
        SORT_KEYS
            .iter()
            .enumerate()
            .map(|(index, (_, name))| ((*name).to_owned(), i32::try_from(index).unwrap_or(0))),
    );

    emit_experience_enums(out);
}

/// Emits the enums that describe how the application behaves and what it
/// delivers, rather than what the music is.
///
/// The third split, along the same line as the first two: a reader looking for
/// how a notice is numbered is not made to scroll through key signatures.
fn emit_experience_enums(out: &mut String) {
    emit_enum(
        out,
        "/* How much the application volunteers. */",
        "PrvExperienceMode",
        "PRV_MODE_FORCE_SIGNED",
        MODES
            .iter()
            .enumerate()
            .map(|(index, (_, name))| ((*name).to_owned(), i32::try_from(index).unwrap_or(0))),
    );

    emit_enum(
        out,
        "/* Where the user's attention is. Performing holds back anything that\n         * can wait. */",
        "PrvAttention",
        "PRV_ATTENTION_FORCE_SIGNED",
        ATTENTION.iter().enumerate().map(|(index, (_, name))| {
            ((*name).to_owned(), i32::try_from(index).unwrap_or(0))
        }),
    );

    emit_enum(
        out,
        "/* What the application may want to tell the user. */",
        "PrvNotice",
        "PRV_NOTICE_FORCE_SIGNED",
        prv_notify::Notice::ALL.iter().map(|notice| {
            (
                format!(
                    "PRV_NOTICE_{}",
                    notice.key().trim_start_matches("notice.").to_uppercase()
                ),
                notice_code(*notice),
            )
        }),
    );

    emit_enum(
        out,
        "/* Where a master is going. */",
        "PrvDeliveryTarget",
        "PRV_TARGET_FORCE_SIGNED",
        TARGETS
            .iter()
            .enumerate()
            .map(|(index, (_, name))| ((*name).to_owned(), i32::try_from(index).unwrap_or(0))),
    );

    emit_enum(
        out,
        "/* What container it goes in. */",
        "PrvFormat",
        "PRV_FORMAT_FORCE_SIGNED",
        FORMATS
            .iter()
            .enumerate()
            .map(|(index, (_, name))| ((*name).to_owned(), i32::try_from(index).unwrap_or(0))),
    );

    emit_enum(
        out,
        "/* At what resolution. Dither follows this, not the format. */",
        "PrvBitDepth",
        "PRV_DEPTH_FORCE_SIGNED",
        DEPTHS
            .iter()
            .enumerate()
            .map(|(index, (_, name))| ((*name).to_owned(), i32::try_from(index).unwrap_or(0))),
    );

    emit_enum(
        out,
        "/* Whether the master can go as it is. */",
        "PrvCompliance",
        "PRV_COMPLIANCE_FORCE_SIGNED",
        COMPLIANCE
            .iter()
            .enumerate()
            .map(|(index, (_, name))| ((*name).to_owned(), i32::try_from(index).unwrap_or(0))),
    );

    emit_enum(
        out,
        "/* Which text field of a track to read. */",
        "PrvTextField",
        "PRV_FIELD_FORCE_SIGNED",
        TEXT_FIELDS
            .iter()
            .enumerate()
            .map(|(index, name)| ((*name).to_owned(), i32::try_from(index).unwrap_or(0))),
    );
}

/// Emits the enums that name things in the product rather than in the boundary.
///
/// Split from [`emit_types`] along the line that matters: everything there is
/// about *calling* — statuses, handles, the callback — and everything here is
/// about the music and the licence. A reader looking for one is not made to
/// scroll through the other.
/// The vocabulary of a score.
///
/// Its own function rather than part of the domain block: a component is
/// something a *judgement* is made of, and the domain enums are things a user
/// agrees to or pays for. Splitting them keeps either free to grow.
/// The vocabulary of synchronisation.
fn emit_sync_enums(out: &mut String) {
    emit_enum(
        out,
        "/* Where synchronisation is. Numbered from one so that zero is never a\n         * state: a host reading an uninitialised value gets something it can\n         * recognise as wrong rather than \"offline\", which is plausible and\n         * therefore the dangerous answer. */",
        "PrvSyncState",
        "PRV_SYNC_STATE_FORCE_SIGNED",
        prv_sync::SyncState::ALL.iter().map(|state| {
            (
                format!(
                    "PRV_SYNC_{}",
                    state.key().trim_start_matches("sync.").to_uppercase()
                ),
                prv_ffi::sync::state_code(*state),
            )
        }),
    );

    emit_enum(
        out,
        "/* Something that happened to synchronisation. A host reports these; it\n         * does not decide what they mean. */",
        "PrvSyncEvent",
        "PRV_SYNC_EVENT_FORCE_SIGNED",
        prv_sync::SyncEvent::ALL.iter().map(|event| {
            (
                format!(
                    "PRV_SYNC_EVENT_{}",
                    event.key().trim_start_matches("sync.event.").to_uppercase()
                ),
                prv_ffi::sync::event_code(*event),
            )
        }),
    );
}

fn emit_score_enums(out: &mut String) {
    emit_enum(
        out,
        "/* What part of a pairing is hardest. Numbered from one so that zero is\n         * never a component: a host reading an uninitialised value gets something\n         * it can recognise as wrong rather than \"harmonic\". */",
        "PrvComponent",
        "PRV_COMPONENT_FORCE_SIGNED",
        prv_mix::transition::Component::ALL.iter().map(|component| {
            (
                format!(
                    "PRV_COMPONENT_{}",
                    component
                        .key()
                        .trim_start_matches("component.")
                        .to_uppercase()
                ),
                prv_ffi::mapping::component_code(*component),
            )
        }),
    );
}

fn emit_domain_enums(out: &mut String) {
    emit_enum(
        out,
        "/* What a user may agree to. Nothing is agreed to by default. */",
        "PrvPurpose",
        "PRV_PURPOSE_FORCE_SIGNED",
        prv_security::Purpose::ALL.iter().map(|purpose| {
            (
                format!(
                    "PRV_PURPOSE_{}",
                    purpose.key().trim_start_matches("purpose.").to_uppercase()
                ),
                purpose_code(*purpose),
            )
        }),
    );

    emit_enum(
        out,
        "/* Licence tiers, cheapest first. */",
        "PrvTier",
        "PRV_TIER_FORCE_SIGNED",
        prv_entitlements::Tier::ALL.iter().map(|tier| {
            (
                format!(
                    "PRV_TIER_{}",
                    tier.key().trim_start_matches("tier.").to_uppercase()
                ),
                tier_code(*tier),
            )
        }),
    );

    emit_enum(
        out,
        "/* Features a licence may gate. The essential ones are available at every\n         * tier, which Master Prompt #29 requires and prv_policy_feature_is_essential\n         * lets a host check. */",
        "PrvFeature",
        "PRV_FEATURE_FORCE_SIGNED",
        prv_entitlements::Feature::ALL.iter().enumerate().map(|(index, feature)| {
            (
                format!(
                    "PRV_FEATURE_{}",
                    feature.key().trim_start_matches("feature.").to_uppercase()
                ),
                i32::try_from(index).unwrap_or(0),
            )
        }),
    );

    emit_enum(
        out,
        "/* The shape of a set's energy over its length. */",
        "PrvEnergyShape",
        "PRV_ENERGY_FORCE_SIGNED",
        ENERGY_SHAPES
            .iter()
            .enumerate()
            .map(|(index, (_, name))| ((*name).to_owned(), i32::try_from(index).unwrap_or(0))),
    );

    emit_enum(
        out,
        "/* How far the planner may depart from established practice.\n         *\n         * This never relaxes a hard constraint. A clashing key is not generated at\n         * any setting; creativity widens the soft limits only. */",
        "PrvCreativity",
        "PRV_CREATIVITY_FORCE_SIGNED",
        CREATIVITY_SETTINGS.iter().enumerate().map(|(index, (_, name))| {
            ((*name).to_owned(), i32::try_from(index).unwrap_or(0))
        }),
    );

    out.push_str(
        r"/* An engine. Opaque: the host never sees inside it, which is what lets the
 * layout change without touching this header. */
typedef struct PrvEngine PrvEngine;

/* A planner: a library of candidates, and the sets planned from it. */
typedef struct PrvPlanner PrvPlanner;

/* What the analysis found out about one track. */
typedef struct PrvAnalysis PrvAnalysis;

/* What the user agreed to, and what their licence allows. */
typedef struct PrvPolicy PrvPolicy;

/* The user's music, and the last search over it. */
typedef struct PrvCollection PrvCollection;

/* What a master measures, and what a target would need of it. */
typedef struct PrvDelivery PrvDelivery;

/* How the application behaves, and what it has queued to say. */
typedef struct PrvExperience PrvExperience;

/* Where synchronisation is, and what has not gone yet. Belongs to an
 * installation rather than to a project: a user with three projects open is
 * not offline three times. */
typedef struct PrvSync PrvSync;

",
    );

    out.push_str(
        r"/*
 * Reads audio for one track.
 *
 * Called from the audio thread, so it obeys the same contract: no allocation, no
 * locking, no blocking file access. A host that cannot satisfy a read from
 * memory it already holds returns fewer frames than asked for, and the renderer
 * records the shortfall rather than stalling.
 *
 * `planar` points at channels * capacity floats, channel-major. Write `frames`
 * frames beginning at `destination` within each channel and return how many were
 * actually written. Claiming more than was asked for is not believed.
 */
typedef uint32_t (*PrvReadAudio)(void *user_data,
                                 uint64_t track,
                                 int64_t source_offset,
                                 float *planar,
                                 uint32_t channels,
                                 uint32_t capacity,
                                 uint32_t destination,
                                 uint32_t frames);

",
    );
}

/// Builds the whole header.
fn header() -> String {
    let mut out = String::new();

    out.push_str(
        r"/*
 * PRV AI DJ Studio — the C boundary.
 *
 * GENERATED FILE. Do not edit.
 *
 * Produced by `cargo run -p prv-ffi --bin bridgegen`. The architecture overview
 * requires bindings to be generated rather than hand-written, because
 * hand-written bindings drift — and a drifting binding does not fail to compile,
 * it computes the wrong answer.
 *
 * Three rules for a host:
 *
 *   1. Call prv_abi_version() before anything else and check the major field
 *      against PRV_ABI_MAJOR.
 *   2. One thread at a time per engine. The handle is not synchronised. The
 *      intended arrangement is one audio thread calling prv_engine_render and
 *      nothing else, and one other thread calling everything else.
 *   3. Never ignore a status. Out-parameters are written on success.
 */

",
    );

    out.push_str("#ifndef PRV_BRIDGE_H\n#define PRV_BRIDGE_H\n\n");
    out.push_str("#include <stdint.h>\n\n");
    out.push_str("#ifdef __cplusplus\nextern \"C\" {\n#endif\n\n");

    out.push_str("/* The version this header describes. */\n");
    let _ = writeln!(out, "#define PRV_ABI_MAJOR {}", abi::MAJOR);
    let _ = writeln!(out, "#define PRV_ABI_MINOR {}", abi::MINOR);
    let _ = write!(out, "#define PRV_ABI_PATCH {}\n\n", abi::PATCH);

    // Every enum here carries a negative sentinel. The C standard leaves an
    // enum's underlying type implementation-defined, and a compiler that sees
    // only non-negative members is free to choose `unsigned int` — which is what
    // Clang does, and what Swift then imports as `UInt32`. Every function in this
    // header takes and returns `int32_t`, so the constants would need a cast at
    // every use site in Swift for no reason other than a compiler's freedom to
    // choose. One negative member removes the freedom.
    emit_types(&mut out);

    for declaration in declarations() {
        out.push_str("/*\n");
        for line in declaration.doc {
            if line.is_empty() {
                out.push_str(" *\n");
            } else {
                let _ = writeln!(out, " * {line}");
            }
        }
        out.push_str(" */\n");
        write_signature(&mut out, declaration.signature);
        out.push('\n');
    }

    out.push_str("#ifdef __cplusplus\n}\n#endif\n\n#endif /* PRV_BRIDGE_H */\n");
    out
}

/// The Clang module map Swift imports the header through.
///
/// `link "prv_ffi"` is what makes `import CPRVBridge` pull in the static library
/// as well as the declarations, so a host does not have to repeat the library
/// name in its own linker settings.
fn module_map() -> String {
    "// GENERATED FILE. Do not edit.\n\
     //\n\
     // Produced by `cargo run -p prv-ffi --bin bridgegen`, beside the header it\n\
     // describes. Swift imports a C library through a Clang module, so without\n\
     // this there is nothing for `import CPRVBridge` to find.\n\
     \n\
     module CPRVBridge {\n\
     \x20   header \"PRVBridge.h\"\n\
     \x20   link \"prv_ffi\"\n\
     \x20   export *\n\
     }\n"
    .to_owned()
}

/// Where the module map goes, given where the header goes.
fn module_map_path(header: &str) -> std::path::PathBuf {
    std::path::Path::new(header)
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join("module.modulemap")
}

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let (check, path) = match arguments.as_slice() {
        [flag, path] if flag == "--check" => (true, path.clone()),
        [path] => (false, path.clone()),
        _ => {
            eprintln!("usage: bridgegen [--check] <path to PRVBridge.h>");
            return ExitCode::FAILURE;
        }
    };

    let generated = header();

    let map_path = module_map_path(&path);
    let map = module_map();

    if check {
        let mut current = true;
        for (where_, expected) in [
            (path.clone(), generated),
            (map_path.display().to_string(), map),
        ] {
            match std::fs::read_to_string(&where_) {
                Ok(existing) if existing == expected => {}
                Ok(_) => {
                    eprintln!("FAIL  {where_} differs from a fresh generation.");
                    current = false;
                }
                Err(error) => {
                    eprintln!("FAIL  {where_} could not be read: {error}");
                    current = false;
                }
            }
        }
        if current {
            println!("ok    the generated bindings are current");
            ExitCode::SUCCESS
        } else {
            eprintln!("      Run: cargo run -p prv-ffi --bin bridgegen -- {path}");
            ExitCode::FAILURE
        }
    } else {
        if let Some(parent) = std::path::Path::new(&path).parent() {
            if let Err(error) = std::fs::create_dir_all(parent) {
                eprintln!("FAIL  {} could not be created: {error}", parent.display());
                return ExitCode::FAILURE;
            }
        }
        for (where_, contents) in [
            (path.clone(), generated),
            (map_path.display().to_string(), map),
        ] {
            if let Err(error) = std::fs::write(&where_, contents) {
                eprintln!("FAIL  {where_} could not be written: {error}");
                return ExitCode::FAILURE;
            }
            println!("wrote {where_}");
        }
        ExitCode::SUCCESS
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::panic,
        reason = "a test that cannot build its own fixture should fail loudly"
    )]

    use super::*;

    #[test]
    fn a_wrapped_declaration_keeps_every_comma() {
        // The defect this test was written for: the separator belongs to the
        // line being left, and putting it on the line being started drops it at
        // every wrap. The result parses as a declaration with two types and one
        // name, which is a compile error a hundred lines from anything a reader
        // would suspect.
        let mut out = String::new();
        write_signature(
            &mut out,
            "int32_t prv_engine_place_track(PrvEngine *engine, uint64_t track, int64_t position, \
             int64_t length, int64_t source_offset, uint32_t lane, int64_t timestamp_micros, \
             uint64_t *out_placement)",
        );

        assert!(out.lines().count() > 1, "the fixture did not wrap at all");
        for line in out.lines() {
            let trimmed = line.trim_end();
            assert!(
                trimmed.ends_with(',') || trimmed.ends_with(");") || trimmed.ends_with('('),
                "line does not end in a separator: {trimmed}"
            );
        }
    }

    #[test]
    fn every_argument_survives_the_wrap() {
        let signature = "int32_t prv_engine_place_track(PrvEngine *engine, uint64_t track, \
                         int64_t position, int64_t length, int64_t source_offset, uint32_t lane, \
                         int64_t timestamp_micros, uint64_t *out_placement)";
        let mut out = String::new();
        write_signature(&mut out, signature);

        for name in [
            "engine",
            "track",
            "position",
            "length",
            "source_offset",
            "lane",
            "timestamp_micros",
            "out_placement",
        ] {
            assert!(out.contains(name), "{name} was lost in the wrap");
        }
        assert_eq!(
            out.matches(',').count(),
            7,
            "a wrapped declaration has one comma per argument gap"
        );
    }

    #[test]
    fn a_short_declaration_is_left_on_one_line() {
        let mut out = String::new();
        write_signature(&mut out, "uint32_t prv_abi_version(void)");
        assert_eq!(out, "uint32_t prv_abi_version(void);\n");
    }

    #[test]
    fn the_generated_header_declares_every_exported_function() {
        // The generator walks one list; this checks the list is the whole
        // boundary. A function exported from `exports.rs` and forgotten here is
        // invisible to every host, which is a quiet way to ship half a feature.
        let text = header();
        for declaration in declarations() {
            let name = declaration
                .signature
                .split('(')
                .next()
                .and_then(|head| head.split_whitespace().last())
                .expect("a signature has a name");
            assert!(
                text.contains(name),
                "{name} is declared but did not reach the header"
            );
        }
    }

    #[test]
    fn the_header_has_an_include_guard_and_closes_it() {
        let text = header();
        assert!(text.contains("#ifndef PRV_BRIDGE_H"));
        assert!(text.contains("#define PRV_BRIDGE_H"));
        assert!(text.trim_end().ends_with("#endif /* PRV_BRIDGE_H */"));
        // And the C++ guard, because the Apple layer will include this from
        // Objective-C++ sooner or later.
        assert_eq!(text.matches("__cplusplus").count(), 2);
    }
}
