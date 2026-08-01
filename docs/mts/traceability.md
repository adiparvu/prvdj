# Requirements Traceability

Master Prompt #28 requires traceability between requirements, architecture
decisions, code changes, tests and releases. This is that link.

## How to read it

Every row names a requirement, where it is decided, where it is implemented and
what verifies it. A requirement with no verification column is not yet built, and
says so — that is the point of the document. A row that claimed verification it
did not have would be worse than no row at all.

**Status** is one of:

- **Verified** — implemented and covered by a test that runs on every pull request.
- **Enforced** — a build gate, not a test: violating it fails the build.
- **Decided** — the architecture is fixed in a record; implementation follows.
- **Not started** — named here so its absence is visible.

## Cross-cutting rules

| Requirement | Source | Where | Verification | Status |
|---|---|---|---|---|
| No allocation on the audio thread | MP#18, MS#002 | `prv-rt`, `prv-dsp`, `prv-transport` | `prv-rt/tests/realtime_contract.rs`, `prv-dsp/tests/realtime_contract.rs` — 175 000 blocks, zero allocations, with a positive control | **Verified** |
| No destructor on the audio thread | ADR-0002 | as above | same tests, deallocation counter | **Verified** |
| No locking or waiting on the audio thread | MP#18 | `prv-rt::spsc`, `prv-rt::triple_buffer` | wait-free by construction; concurrency tests | **Verified** |
| No panic on the realtime path | ADR-0002 | workspace lint policy | `unwrap_used`, `expect_used`, `panic`, `indexing_slicing` denied | **Enforced** |
| The core performs no I/O | ADR-0001 | `core/` | `tools/check-architecture.sh` rule 1 | **Enforced** |
| Unsafe confined to one audited crate | ADR-0002 | `prv-rt` only | `tools/check-architecture.sh` rule 2 | **Enforced** |
| No placeholder implementations | MP#13 | `core/` | `tools/check-architecture.sh` rule 3 | **Enforced** |
| Every public item documented | MP#31 | workspace | `missing_docs` denied; `cargo doc` with warnings denied | **Enforced** |
| Every architectural decision recorded | MP#14, MP#31 | `docs/adr/` | rules 5 and 6 check the index resolves and every record has a review date | **Enforced** |
| No hardcoded visual values | MP#16 | `design/tokens/` | `tokengen --check` gate | **Enforced** |
| Dependencies point inward only | MP#4, MP#7 | crate graph | the core cannot name a UI framework — it does not exist in its language | **Enforced** |

## Musical time and transport

| Requirement | Source | Where | Verification | Status |
|---|---|---|---|---|
| Sample-accurate transport | MP#18, MS#002 | `prv-time::TransportClock` | `advancing_never_drifts`, `six_hours_of_playback_has_zero_drift` | **Verified** |
| One authoritative clock; no module creates its own | MS#002 | `prv-time` | structural — no other type can construct musical position | **Verified** |
| Beat and bar position | MS#002 | `prv-time::MusicalTime` | `musical_position_tracks_the_beat_grid` | **Verified** |
| Multiple tempo regions | MP#3A | `prv-time::TempoMap` | `a_tempo_change_only_affects_positions_after_it`, `positions_are_monotonic_across_segments` | **Verified** |
| Tempo drift and manual correction | MP#20 | `prv-time::TempoMap`, `BeatGrid::nudge` | `conversion_round_trips_across_segment_boundaries` | **Verified** |
| Beat grid with editable origin | MP#3A | `prv-time::BeatGrid` | `the_origin_defines_the_first_downbeat`, `nudging_moves_the_whole_grid` | **Verified** |
| Snap to beat, bar, phrase, division, sample | MP#21 | `prv-time::SnapResolution` | `snapping_to_a_bar_and_a_phrase`, `divisions_cover_the_common_loop_lengths`, `directional_snapping_holds_at_every_resolution` | **Verified** |
| Half-time and double-time detection support | MP#20 | `prv-time::Tempo::halved/doubled` | `half_and_double_time_are_exact_for_even_values` | **Verified** |
| Nine playback states, explicit transitions, none hidden | MS#002 | `prv-transport::PlaybackState` | `every_state_and_event_combination_has_a_defined_outcome` — all 405 combinations | **Verified** |
| Recovery from device change without stopping | MS#002, MP#22 | `prv-transport`, `prv-time::set_sample_rate` | `a_device_change_does_not_stop_the_music`, `a_sample_rate_change_rescales_the_loop_and_preserves_the_music` | **Verified** |
| Transport preserved through failure | MP#22 | `prv-transport` | `a_device_lost_during_a_seek_recovers_rather_than_stopping` | **Verified** |
| Loop engine with phase-correct wrapping | MP#3A, MP#22 | `prv-transport::LoopRegion` | `a_loop_repeats_indefinitely_without_drifting`, `a_loop_shorter_than_a_block_still_keeps_phase` | **Verified** |
| Slip mode | MP#3A, MP#22 | `prv-transport::Transport` | `slip_mode_returns_to_the_arrangement` | **Verified** |

## Signal path

| Requirement | Source | Where | Verification | Status |
|---|---|---|---|---|
| No clicks or zipper noise | MP#18 | `prv-rt::LinearSmoother`, `prv-dsp` | `a_ramp_is_monotonic_and_has_no_discontinuity`, `a_level_change_ramps_rather_than_stepping` | **Verified** |
| Silence remains silent | MP#15 | `prv-dsp` | `silence_in_produces_silence_out`, `resetting_clears_the_tail` | **Verified** |
| Three-band EQ with true kill | MP#3A, MP#22 | `prv-dsp::ThreeBandEq` | `killing_the_low_band_removes_the_bass_and_leaves_the_rest` — better than −30 dB | **Verified** |
| Flat response at unity | quality baseline | `prv-dsp::ThreeBandEq` | `at_unity_the_response_is_flat` — within 0.6 dB, ten frequencies | **Verified** |
| Filter sweep | MP#3A, MP#3C | `prv-dsp::DjFilter` | `turning_left_removes_the_treble`, `the_sweep_is_monotonic`, `the_filter_stays_stable_under_a_fast_sweep` | **Verified** |
| Effects stackable, bypassable, latency-reporting | MP#3A, MP#23 | `prv-dsp::Chain` | `latency_sums_only_the_active_processors`, `a_bypassed_processor_is_skipped_entirely` | **Verified** |
| Per-block cost bounded | MP#31 | `prv-dsp::Chain` | `the_chain_length_is_bounded` | **Verified** |
| Wait-free state publication to the interface | MS#002, MS#003 | `prv-rt::triple_buffer` | `values_are_never_torn_under_concurrency` | **Verified** |
| Time stretching, key lock | MP#3A | — | — | Not started |
| Master bus, LUFS, true peak | MP#3A, MP#3C | — | — | Not started |
| Recording and export | MP#3A | — | — | Not started |

## Musical intelligence

| Requirement | Source | Where | Verification | Status |
|---|---|---|---|---|
| Musical decisions computed, not generated | MP#3B, MP#19 | ADR-0006 | — | **Decided** |
| Never produce clashing keys | MP#3B | `prv-harmony::HarmonicSafety` | `opposite_sides_of_the_wheel_clash`, `every_safe_relation_outscores_every_risky_one` | **Verified** |
| Harmonic compatibility and Camelot | MP#3A, MP#20 | `prv-harmony` | all 24 published wheel positions reproduced against an external chart | **Verified** |
| Explanations derived from evidence | MP#3B, ADR-0006 | `prv-harmony::KeyRelation` | classification separate from weighting; `relations_read_as_plain_language` | **Verified** |
| The planner never strands a track | MP#3B | `prv-harmony` | `every_key_has_at_least_six_safe_destinations` | **Verified** |
| Confidence calibrated centrally | MP#25 | ADR-0006 | — | **Decided** |
| Works with cloud AI disabled | MP#26 | ADR-0006 | — | **Decided** |
| Tempo, key, structure, energy detection | MP#20 | — | — | Not started |
| Transition scoring at import | MP#20 | — | — | Not started |
| Mix planner, versions A/B/C | MP#3B | — | — | Not started |
| Learning profile | MP#5 | — | — | Not started |
| Agent registry and orchestration | MP#6, MP#19 | — | — | Not started |

## Product and platform

| Requirement | Source | Where | Verification | Status |
|---|---|---|---|---|
| Non-destructive editing, version history | MP#3C, MP#7, MP#9 | ADR-0003 | — | **Decided** |
| Offline-first | MP#1, MP#24 | ADR-0001, ADR-0006 | the core has no network dependency, enforced by rule 1 | **Enforced** |
| Sharing a project does not distribute audio | MP#15, MP#29 | ADR-0003 | — | **Decided** |
| Plugins sandboxed; a crash never stops playback | MP#23 | ADR-0005 | — | **Decided** |
| Stem separation on-device by default | MP#26, MP#29 | ADR-0004 | — | **Decided** |
| Colour never carries meaning alone | MP#8, MP#16 | `design/tokens/tokens.json` | every semantic colour is paired in its component | **Decided** |
| Motion degrades under reduced motion | MP#8, MP#16 | `MotionToken::reducedMotion` | generated and compiled | **Verified** |
| Entitlements never reach the engines | MP#29 | module index | no engine crate depends on licensing | **Enforced** |
| Waveform tiles at several resolutions | MS#003 | `prv-waveform` | `every_level_is_exact_rather_than_derived_from_the_one_below` | **Verified** |
| Never upscale coarse waveform data | MS#003 | `prv-waveform::level_for` | `level_selection_never_upscales` | **Verified** |
| Waveform generation is resumable | MS#003, MP#7 | `prv-waveform::WaveformBuilder` | `chunking_does_not_change_the_result` | **Verified** |
| Waveform invalidation by generation version | MP#20, MS#003 | `prv-waveform::GenerationVersion` | `versions_are_compared_exactly` | **Verified** |
| The renderer never processes invisible regions | MS#003 | `prv-waveform::render` | `rendering_writes_exactly_the_requested_columns` | **Verified** |
| Rendering allocates nothing per frame | MP#4, MS#003 | `prv-waveform::render` | caller-supplied buffer; no allocation in the call | **Verified** |
| Clipping detected and located | MP#20 | `prv-waveform::Tile::is_clipped` | `clipping_is_detected_at_full_scale` | **Verified** |
| Library, timeline, cloud, UI | MS#001, MP#21, MP#24, MP#17 | — | — | Not started |

## On the specification corpus itself

The Master Prompts and Module Specifications are referenced throughout the code
and documentation by number. They are not reproduced verbatim in the repository.

That is a deliberate choice, and it is worth stating rather than leaving as an
omission. Copying thirty-five documents in would create a second copy of the
requirements that must be kept in step with the first, and a copy that drifts is
worse than a reference. What the repository needs from the specification is not
its prose but its *bindings* — which requirement produced which decision, which
code satisfies it, and which test proves it. That is this document, and it is
mechanically checkable in a way a transcript is not.

If the corpus is to live in the repository as well, the right form is a read-only
`docs/specifications/` directory whose contents are never edited, with this
matrix remaining the working index. Say the word and it will be added.
