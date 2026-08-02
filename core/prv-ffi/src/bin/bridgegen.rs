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

use prv_ffi::mapping::{PLAYBACK_STATES, TRANSPORT_EVENTS};
use prv_ffi::planning::{CREATIVITY_SETTINGS, ENERGY_SHAPES};
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
    out.push_str(
        "/* The result of a call. Zero is success, and it is the only success. */\ntypedef enum PrvStatus {\n",
    );
    for status in Status::ALL {
        let _ = writeln!(out, "    {} = {},", status.c_name(), status.code());
    }
    out.push_str(&signed_sentinel("PRV_STATUS_FORCE_SIGNED"));
    out.push_str("} PrvStatus;\n\n");

    out.push_str("/* What the transport is doing. */\ntypedef enum PrvPlaybackState {\n");
    for (state, name) in PLAYBACK_STATES {
        let _ = writeln!(
            out,
            "    {} = {},",
            name,
            prv_ffi::mapping::playback_code(*state)
        );
    }
    out.push_str(&signed_sentinel("PRV_PLAYBACK_FORCE_SIGNED"));
    out.push_str("} PrvPlaybackState;\n\n");

    out.push_str("/* What can happen to the transport. */\ntypedef enum PrvTransportEvent {\n");
    for (index, (_, name)) in TRANSPORT_EVENTS.iter().enumerate() {
        let _ = writeln!(out, "    {name} = {index},");
    }
    out.push_str(&signed_sentinel("PRV_EVENT_FORCE_SIGNED"));
    out.push_str("} PrvTransportEvent;\n\n");

    out.push_str(
        "/* The shape of a set's energy over its length. */\ntypedef enum PrvEnergyShape {\n",
    );
    for (index, (_, name)) in ENERGY_SHAPES.iter().enumerate() {
        let _ = writeln!(out, "    {name} = {index},");
    }
    out.push_str(&signed_sentinel("PRV_ENERGY_FORCE_SIGNED"));
    out.push_str("} PrvEnergyShape;\n\n");

    out.push_str(
        "/* How far the planner may depart from established practice.\n\
         *\n\
         * This never relaxes a hard constraint. A clashing key is not generated at\n\
         * any setting; creativity widens the soft limits only. */\ntypedef enum PrvCreativity {\n",
    );
    for (index, (_, name)) in CREATIVITY_SETTINGS.iter().enumerate() {
        let _ = writeln!(out, "    {name} = {index},");
    }
    out.push_str(&signed_sentinel("PRV_CREATIVITY_FORCE_SIGNED"));
    out.push_str("} PrvCreativity;\n\n");

    out.push_str(
        r"/* An engine. Opaque: the host never sees inside it, which is what lets the
 * layout change without touching this header. */
typedef struct PrvEngine PrvEngine;

/* A planner: a library of candidates, and the sets planned from it. */
typedef struct PrvPlanner PrvPlanner;

/* What the analysis found out about one track. */
typedef struct PrvAnalysis PrvAnalysis;

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
