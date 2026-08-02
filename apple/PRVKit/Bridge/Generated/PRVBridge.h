/*
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

#ifndef PRV_BRIDGE_H
#define PRV_BRIDGE_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* The version this header describes. */
#define PRV_ABI_MAJOR 1
#define PRV_ABI_MINOR 2
#define PRV_ABI_PATCH 0

/* The result of a call. Zero is success, and it is the only success. */
typedef enum PrvStatus {
    PRV_OK = 0,
    PRV_NULL_POINTER = 1,
    PRV_INVALID_ARGUMENT = 2,
    PRV_INVALID_HANDLE = 3,
    PRV_INVALID_STATE = 4,
    PRV_BUFFER_TOO_SMALL = 5,
    PRV_REFUSED = 6,
    PRV_PANICKED = 7,
    /* Not a value. Present so the underlying type is signed, matching the
       int32_t every function here takes. Never returned, never compared. */
    PRV_STATUS_FORCE_SIGNED = -1,
} PrvStatus;

/* What the transport is doing. */
typedef enum PrvPlaybackState {
    PRV_PLAYBACK_STOPPED = 0,
    PRV_PLAYBACK_LOADING = 1,
    PRV_PLAYBACK_READY = 2,
    PRV_PLAYBACK_PLAYING = 3,
    PRV_PLAYBACK_PAUSED = 4,
    PRV_PLAYBACK_SEEKING = 5,
    PRV_PLAYBACK_BUFFERING = 6,
    PRV_PLAYBACK_RECOVERING = 7,
    PRV_PLAYBACK_ERROR = 8,
    /* Not a value. Present so the underlying type is signed, matching the
       int32_t every function here takes. Never returned, never compared. */
    PRV_PLAYBACK_FORCE_SIGNED = -1,
} PrvPlaybackState;

/* What can happen to the transport. */
typedef enum PrvTransportEvent {
    PRV_EVENT_LOAD = 0,
    PRV_EVENT_LOAD_SUCCEEDED = 1,
    PRV_EVENT_LOAD_FAILED = 2,
    PRV_EVENT_UNLOAD = 3,
    PRV_EVENT_PLAY = 4,
    PRV_EVENT_PAUSE = 5,
    PRV_EVENT_STOP = 6,
    PRV_EVENT_SEEK_REQUESTED = 7,
    PRV_EVENT_SEEK_COMPLETED = 8,
    PRV_EVENT_BUFFER_EXHAUSTED = 9,
    PRV_EVENT_BUFFER_REFILLED = 10,
    PRV_EVENT_DEVICE_LOST = 11,
    PRV_EVENT_DEVICE_RESTORED = 12,
    PRV_EVENT_FAULT = 13,
    PRV_EVENT_RESET = 14,
    /* Not a value. Present so the underlying type is signed, matching the
       int32_t every function here takes. Never returned, never compared. */
    PRV_EVENT_FORCE_SIGNED = -1,
} PrvTransportEvent;

/* The shape of a set's energy over its length. */
typedef enum PrvEnergyShape {
    PRV_ENERGY_RISING = 0,
    PRV_ENERGY_ARC = 1,
    PRV_ENERGY_PLATEAU = 2,
    PRV_ENERGY_WAVE = 3,
    PRV_ENERGY_FALLING = 4,
    /* Not a value. Present so the underlying type is signed, matching the
       int32_t every function here takes. Never returned, never compared. */
    PRV_ENERGY_FORCE_SIGNED = -1,
} PrvEnergyShape;

/* How far the planner may depart from established practice.
*
* This never relaxes a hard constraint. A clashing key is not generated at
* any setting; creativity widens the soft limits only. */
typedef enum PrvCreativity {
    PRV_CREATIVITY_CONSERVATIVE = 0,
    PRV_CREATIVITY_BALANCED = 1,
    PRV_CREATIVITY_ADVENTUROUS = 2,
    /* Not a value. Present so the underlying type is signed, matching the
       int32_t every function here takes. Never returned, never compared. */
    PRV_CREATIVITY_FORCE_SIGNED = -1,
} PrvCreativity;

/* An engine. Opaque: the host never sees inside it, which is what lets the
 * layout change without touching this header. */
typedef struct PrvEngine PrvEngine;

/* A planner: a library of candidates, and the sets planned from it. */
typedef struct PrvPlanner PrvPlanner;

/* What the analysis found out about one track. */
typedef struct PrvAnalysis PrvAnalysis;

/*
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

/*
 * The version of this boundary, packed as major << 16 | minor << 8 | patch.
 *
 * Call this first, before anything else. Compare the major field against
 * PRV_ABI_MAJOR, which is the version this header describes.
 */
uint32_t prv_abi_version(void);

/*
 * Non-zero if a host built against `host_major` can use this library.
 */
int32_t prv_abi_is_compatible(uint32_t host_major);

/*
 * A static, NUL-terminated description of a status code.
 *
 * Never null, never freed, valid for as long as the library is loaded.
 * An unrecognised code returns a string saying so.
 */
const char *prv_status_message(int32_t code);

/*
 * Creates an engine.
 *
 * On success writes the handle to *out_engine. On failure writes NULL,
 * so a caller that ignores the status still gets a pointer it can test.
 */
int32_t prv_engine_create(uint32_t sample_rate, uint32_t channels,
                          uint32_t max_block_frames, PrvEngine **out_engine);

/*
 * Destroys an engine. NULL is accepted and does nothing.
 *
 * Passing the same handle twice is a double free.
 */
void prv_engine_destroy(PrvEngine *engine);

/*
 * Registers where the renderer gets audio.
 *
 * `user_data` is opaque: never dereferenced, never copied, never freed by
 * the core. Keep it alive until the engine is destroyed or another source
 * replaces this one. A NULL callback detaches the source, after which the
 * engine renders silence and reports every placement incomplete.
 */
int32_t prv_engine_set_source(PrvEngine *engine, PrvReadAudio read, void *user_data);

/*
 * Applies a transport event. See PrvTransportEvent.
 *
 * Returns PRV_INVALID_STATE when the transition is not one the machine
 * defines — pressing play on a deck with nothing loaded, for instance.
 * That is a real answer, not a failure to act on.
 */
int32_t prv_engine_transport(PrvEngine *engine, int32_t event);

/*
 * Moves the playhead to an absolute frame position.
 */
int32_t prv_engine_seek(PrvEngine *engine, int64_t position);

/*
 * Reads the playhead position, in frames.
 */
int32_t prv_engine_position(const PrvEngine *engine, int64_t *out_position);

/*
 * Reads the playback state. See PrvPlaybackState.
 */
int32_t prv_engine_playback_state(const PrvEngine *engine, int32_t *out_state);

/*
 * Places a track on the timeline and returns its placement identity.
 *
 * `source_offset` is how far into the track playback begins; zero is the
 * start. `timestamp_micros` comes from the host because the core has no
 * clock of its own.
 */
int32_t prv_engine_place_track(PrvEngine *engine, uint64_t track, int64_t position,
                               int64_t length, int64_t source_offset, uint32_t lane,
                               int64_t timestamp_micros, uint64_t *out_placement);

/*
 * Reads the project's length, in frames.
 */
int32_t prv_engine_duration(const PrvEngine *engine, int64_t *out_duration);

/*
 * Reads how many placements the project holds.
 */
int32_t prv_engine_placement_count(const PrvEngine *engine, uint64_t *out_count);

/*
 * Renders one block into a caller-owned buffer.
 *
 * `planar` points at channels * frames floats, channel-major: all of
 * channel 0, then all of channel 1.
 *
 * THIS IS THE AUDIO THREAD. It allocates nothing, locks nothing and waits
 * for nothing. It is the only function here a host may call from a
 * realtime context, and the only one it must.
 *
 * A block larger than the engine was built for is refused rather than
 * truncated: a half-filled buffer would play its own leftovers.
 */
int32_t prv_engine_render(PrvEngine *engine, float *planar, uint32_t channels,
                          uint32_t frames);

/*
 * Whether every placement the last render touched was read in full.
 *
 * Zero means some audio was missing. That is worth showing a user, but it
 * is not an error: the render happened and what was there is correct.
 */
int32_t prv_engine_render_was_complete(const PrvEngine *engine, int32_t *out_complete);

/*
 * Creates a planner.
 *
 * A separate handle from the engine, deliberately. A library and a plan
 * are not a project: a host may plan with no project open, and may keep
 * one open while replanning.
 */
int32_t prv_planner_create(PrvPlanner **out_planner);

/*
 * Destroys a planner. NULL is accepted and does nothing.
 */
void prv_planner_destroy(PrvPlanner *planner);

/*
 * Adds one track to the library the planner chooses from.
 *
 * `key_confidence` at or below zero means the key is unknown. `has_vocals`
 * is -1 for unknown, 0 for no, 1 for yes — and unknown is a different
 * answer from no, which scores differently.
 *
 * Facts are arguments rather than a struct on purpose: a struct here would
 * be a permanent layout promise, and the first field anybody wants to add
 * next year would break every host compiled against it.
 */
int32_t prv_planner_add_candidate(PrvPlanner *planner, uint64_t track, int64_t duration,
                                  double bpm, float energy, int32_t key_semitones,
                                  int32_t key_is_minor, float key_confidence,
                                  float loudness_lufs, int32_t has_vocals);

/*
 * Adds a place the analysis says a track can be left or entered.
 *
 * A track with no exit point is mixed out of near its end, which the
 * planner treats as a real answer rather than a missing one.
 */
int32_t prv_planner_add_mix_point(PrvPlanner *planner, uint64_t track, int64_t position,
                                  float energy, int32_t is_exit);

/*
 * Reads how many candidates the library holds.
 */
int32_t prv_planner_candidate_count(const PrvPlanner *planner, uint64_t *out_count);

/*
 * Forgets the library and any plan made from it.
 */
int32_t prv_planner_clear(PrvPlanner *planner);

/*
 * Plans up to three genuinely different sets.
 *
 * Pass zero for both tempo bounds to leave the range open; half a range is
 * treated as no range, because honouring it would constrain the set in a
 * way nobody asked for.
 *
 * Returns PRV_REFUSED when no set could be built. That is a real answer
 * about the library — nothing in it fits — not a malfunction.
 */
int32_t prv_planner_plan(PrvPlanner *planner, int64_t target_frames,
                         uint32_t sample_rate, int32_t shape, int32_t creativity,
                         float tempo_floor, float tempo_ceiling, uint64_t *out_count);

/*
 * Chooses which alternative subsequent reads describe.
 */
int32_t prv_planner_select(PrvPlanner *planner, uint64_t index);

/*
 * Reads how many tracks the selected plan holds.
 */
int32_t prv_planner_track_count(const PrvPlanner *planner, uint64_t *out_count);

/*
 * Reads how long the selected plan runs for, in frames.
 */
int32_t prv_planner_duration(const PrvPlanner *planner, int64_t *out_duration);

/*
 * Reads the selected plan's mean transition score, from zero to one.
 */
int32_t prv_planner_score(const PrvPlanner *planner, float *out_score);

/*
 * Reads one track of the selected plan.
 *
 * The score is the move *into* this track, and is 1.0 for the opening
 * track, which was chosen rather than transitioned into.
 */
int32_t prv_planner_track(const PrvPlanner *planner, uint64_t index, uint64_t *out_track,
                          int64_t *out_start, int64_t *out_duration, float *out_score);

/*
 * Applies the selected plan to an engine's project.
 *
 * The plan becomes ordinary operations on the log — the same ones a
 * hand-made edit produces. After this there is nothing in the document
 * that says which placements a person made and which the planner did,
 * which is what makes a generated mix editable rather than merely
 * promised to be.
 */
int32_t prv_planner_apply(const PrvPlanner *planner, PrvEngine *engine,
                          int64_t timestamp_micros);

/*
 * Analyses a track. `samples` is mono, `frames` long.
 *
 * The audio is borrowed for the duration of this call and never retained.
 *
 * NOT THE AUDIO THREAD. This allocates and takes seconds on a long track.
 * It belongs to the background domain; calling it from a render callback
 * would drop out.
 */
int32_t prv_analysis_run(const float *samples, uint64_t frames, uint32_t sample_rate,
                         PrvAnalysis **out_analysis);

/*
 * Destroys an analysis. NULL is accepted and does nothing.
 */
void prv_analysis_destroy(PrvAnalysis *analysis);

/*
 * Reads the tempo in beats per minute, and how sure the estimate is.
 *
 * Returns PRV_REFUSED when no pulse was found. That is the answer, not a
 * failure: a track with no discernible tempo has none, and a guessed 120
 * would reach the planner and a whole set would be built on it.
 */
int32_t prv_analysis_tempo(const PrvAnalysis *analysis, double *out_bpm,
                           float *out_confidence);

/*
 * Reads the key: semitones above C, whether it is minor, and confidence.
 */
int32_t prv_analysis_key(const PrvAnalysis *analysis, int32_t *out_semitones,
                         int32_t *out_is_minor, float *out_confidence);

/*
 * Reads the integrated loudness in LUFS and the loudness range.
 */
int32_t prv_analysis_loudness(const PrvAnalysis *analysis, double *out_integrated,
                              double *out_range);

/*
 * Reads the true peak, in decibels relative to full scale.
 */
int32_t prv_analysis_true_peak(const PrvAnalysis *analysis, double *out_true_peak);

/*
 * Reads the track's overall energy, from zero to one.
 *
 * Absent rather than defaulted when no structure was found, for the same
 * reason the tempo is: the planner shapes a whole set around this number.
 */
int32_t prv_analysis_energy(const PrvAnalysis *analysis, float *out_energy);

/*
 * Reads how long the analysed audio was, in frames.
 */
int32_t prv_analysis_duration(const PrvAnalysis *analysis, int64_t *out_frames);

/*
 * Reads how many places the analysis found a transition could happen.
 */
int32_t prv_analysis_transition_point_count(const PrvAnalysis *analysis,
                                            uint64_t *out_count);

/*
 * Reads one place a transition could happen: where, and how quiet.
 *
 * Quieter is better to mix on, which is why the energy comes back with the
 * position rather than needing a second call.
 */
int32_t prv_analysis_transition_point(const PrvAnalysis *analysis, uint64_t index,
                                      int64_t *out_position, float *out_energy);

#ifdef __cplusplus
}
#endif

#endif /* PRV_BRIDGE_H */
