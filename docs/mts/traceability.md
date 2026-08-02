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
| Confidence calibrated centrally | MP#25 | `prv-analysis::Confidence` | `the_scale_is_continuous_and_monotonic`, `every_label_is_reachable`, `actionability_agrees_with_the_label` | **Verified** |
| A derived result is never more certain than its inputs | MP#25, ADR-0006 | `prv-analysis::Confidence::and_then` | `a_chain_is_no_stronger_than_its_weakest_stage` | **Verified** |
| Works with cloud AI disabled | MP#26 | ADR-0006 | — | **Decided** |
| Tempo detection | MP#20 | `prv-analysis::tempo` | `a_click_track_is_measured_to_within_half_a_beat_per_minute`, `a_tempo_that_is_not_a_whole_number_of_frames_is_not_read_as_half_time` | **Verified** |
| Half-time and double-time offered, not hidden | MP#20 | `prv-analysis::tempo::OctaveRelation` | `the_half_time_reading_is_offered_rather_than_hidden`, `a_fast_track_is_reported_in_the_range_a_dj_expects` | **Verified** |
| Beat positions accurate enough for a professional grid | MP#18, MP#20 | `prv-analysis::beats` | `beats_land_on_the_clicks`, `the_grid_does_not_drift_over_a_long_track`, `the_fitted_tempo_is_accurate_to_a_hundredth_of_a_beat_per_minute` | **Verified** |
| The grid survives a passage with no onsets | MP#20 | `prv-analysis::beats` | `the_tracker_coasts_through_a_gap_and_recovers` | **Verified** |
| Downbeat detection | MP#20 | `prv-analysis::beats::downbeat_phase` | `the_downbeat_follows_the_low_end_rather_than_the_onsets` | **Verified** |
| Onsets found where an envelope detector would fail | MP#20 | `prv-analysis::NoveltyCurve` | `the_curve_survives_a_twenty_decibel_level_change`, `a_sustained_tone_has_no_onsets_in_its_interior` | **Verified** |
| Analysis produces the same result on every platform | ADR-0006 | `prv-analysis::fft` | transform written in-crate, checked against the direct sum; `estimation_is_reproducible`, `tracking_is_reproducible` | **Verified** |
| Analysis hands over to exact integer time | MP#18, MS#002 | `prv-analysis::BeatEstimate::to_beat_grid` | `a_beat_grid_is_anchored_on_the_first_downbeat` | **Verified** |
| Key detection | MP#20 | `prv-analysis::key` | `a_c_major_scale_is_detected_as_c_major_or_its_relative`, `transposing_a_profile_transposes_the_key` | **Verified** |
| Key ambiguity surfaced rather than hidden | MP#20, MP#25 | `prv-analysis::KeyEstimate::relative` | `the_relative_is_always_offered`, `a_flat_profile_is_not_given_a_confident_key` | **Verified** |
| Loudness to a published standard | MP#3A, MP#20 | `prv-analysis::loudness` | `a_full_scale_kilohertz_tone_reads_the_standard_value`, `the_derived_coefficients_reproduce_the_standard_table` | **Verified** |
| Loudness usable for matching two tracks | MP#3A | `prv-analysis::Loudness::gain_to_reach` | `silence_before_a_track_does_not_make_it_quieter`, `the_gain_to_a_target_is_the_difference` | **Verified** |
| Inter-sample peaks detected | MP#3A, MP#3C | `prv-analysis::Loudness::true_peak_dbfs` | `the_true_peak_catches_what_the_sample_peak_misses` | **Verified** |
| Structure detection | MP#20, MP#21 | `prv-analysis::structure` | `boundaries_land_on_the_arrangement_changes`, `a_track_with_no_arrangement_reports_no_boundaries` | **Verified** |
| Sections usable as transition points | MP#3B, MP#21 | `prv-analysis::Structure::transition_points` | `the_quiet_sections_are_recognised_as_places_to_mix` | **Verified** |
| Each analysis stage versioned independently | MP#20 | `prv-analysis::Stage::version` | `staleness_propagates_to_everything_downstream`, `the_dependency_order_has_no_cycles_and_matches_the_run_order` | **Verified** |
| A stage that finds nothing says so | MP#25 | `prv-analysis::TrackProfile` | `a_stage_that_finds_nothing_is_absent_rather_than_uncertain`, `a_partial_analysis_is_a_success_not_a_failure` | **Verified** |
| Transition scoring at import | MP#20 | — | — | Not started |
| Mix planner | MP#3B, ADR-0006 | `prv-mix::plan` | `a_plan_reaches_the_requested_length_without_repeating_a_track`, `tracks_are_laid_end_to_end_without_gaps` | **Verified** |
| Safety rules are guarantees, not tendencies | MP#3B | `prv-mix::transition::Rejection` | `a_clashing_key_is_not_a_low_score_but_no_candidate_at_all`, `no_plan_contains_a_move_that_violates_a_hard_constraint` | **Verified** |
| Creativity widens soft limits only | MP#3B | `prv-mix::Creativity` | `creativity_unlocks_risky_harmony_and_nothing_beyond_it` | **Verified** |
| The set follows the requested energy shape | MP#3B, MP#12 | `prv-mix::EnergyShape` | `the_set_follows_the_energy_shape_it_was_asked_for` | **Verified** |
| Versions A/B/C are genuinely different | MP#3B | `prv-mix::plan::distinct` | `the_alternatives_are_genuinely_different_sets` | **Verified** |
| The same inputs produce the same set | MP#27, ADR-0006 | `prv-mix::plan` | `planning_is_reproducible` | **Verified** |
| Explanations render the actual arithmetic | ADR-0006, MP#25 | `prv-mix::TransitionScore` | `every_move_carries_the_evidence_that_produced_it` | **Verified** |
| An unanswerable request explains what was missing | MP#10, MP#25 | `prv-mix::PlanError` | `a_library_outside_the_tempo_range_says_so`, `a_short_set_is_returned_rather_than_refused` | **Verified** |
| Full musical capability offline | MP#1, MP#26, ADR-0006 | `prv-mix::Goal` | the planner has no network dependency, enforced by architecture rule 1 | **Enforced** |
| The system learns one user's taste | MP#5 | `prv-learning::Profile` | `a_preference_is_learned_from_what_distinguishes_choices` | **Verified** |
| Learning cannot reach the safety rules | MP#3B, MP#5 | `prv-mix::Weights::scaled` | `no_amount_of_observation_can_switch_a_component_off` | **Verified** |
| Influence arrives gradually, never as a jump | MP#5, MP#25 | `prv-learning::Inference::strength` | `influence_arrives_gradually_rather_than_switching_on`, `a_handful_of_observations_barely_moves_anything` | **Verified** |
| The profile is explainable and correctable | MP#5 | `prv-learning::Profile::explain` | `the_user_can_correct_one_inference_and_delete_all_of_them` | **Verified** |
| The profile holds nothing that identifies anyone | MP#26 | `prv-learning::Observation` | structural — an observation holds six scores and an outcome | **Enforced** |
| Scenario and profile weights combine | ADR-0006 | `prv-learning::Profile::weights_from` | `learning_starts_from_the_scenario_rather_than_replacing_it` | **Verified** |
| Agent registry and orchestration | MP#6, MP#19 | — | — | Not started |

## Product and platform

| Requirement | Source | Where | Verification | Status |
|---|---|---|---|---|
| Non-destructive editing, version history | MP#3C, MP#7, MP#9 | `prv-project::OperationLog` | `undo_appends_an_inverse_rather_than_shortening_the_log`, `materialising_a_position_gives_the_project_as_it_was` | **Verified** |
| Restore, compare, duplicate, branch, merge | MP#9 | `prv-project` | `branching_forks_history_without_recording_the_fork`, `a_branch_can_be_merged_back` | **Verified** |
| Undo, redo, named versions | MP#3C, MP#21 | `prv-project` | `undo_can_itself_be_undone`, `named_versions_are_positions` | **Verified** |
| Incremental synchronisation | MP#24 | `prv-project::operations_since` | `synchronisation_sends_only_what_the_other_side_lacks` | **Verified** |
| Conflicts explained, never silently discarded | MP#24 | `prv-project::MergeReport` | `concurrent_edits_to_the_same_thing_are_reported`, `a_sequential_edit_is_not_a_conflict` | **Verified** |
| Offline devices converge | MP#24 | `prv-project` | `two_devices_converge_on_the_same_state`, `convergence_holds_whatever_order_operations_arrive_in` | **Verified** |
| The project refers to media, never contains it | MP#29, ADR-0003 | `prv-project::TrackRef` | structural — a placement holds a reference | **Verified** |
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
| Library stores, indexes, organises and nothing else | MS#001 | `prv-library` | non-responsibilities enforced by the crate's dependencies | **Verified** |
| Search feels instant at library scale | MS#001 | `prv-library` prefix index | `search_stays_fast_on_a_large_library` | **Verified** |
| Search updates as the user types | MS#001 | `prv-library::TextIndex` | `typing_more_narrows_rather_than_widens` | **Verified** |
| Composable filters, ten sort keys | MS#001 | `prv-library::Query` | `filters_compose_by_conjunction`, `a_limit_truncates_after_sorting_not_before` | **Verified** |
| Duplicates suggested, never deleted | MS#001 | `prv-library::find_duplicates` | `an_edit_and_an_extended_mix_are_reported_but_distinguished`, `the_same_pair_is_never_reported_twice` | **Verified** |
| Missing and moved files lose no organisation | MS#001 | `prv-library::TrackStatus` | `removing_hides_a_track_without_losing_what_the_user_built`, `a_missing_track_still_appears_so_the_user_can_find_it` | **Verified** |
| Overlapping collections | MS#001 | `prv-library` | `collections_overlap_freely` | **Verified** |
| Nothing permanently deleted by default | MP#9 | `prv-library::remove` | `a_removed_track_can_be_restored_with_everything_intact` | **Verified** |
| A parameter can be named, stored and re-found | MP#21, MP#23, ADR-0003 | `prv-timeline::ParameterAddress` | `an_address_survives_being_written_down_and_read_back`, `reordering_effects_re_addresses_only_what_moved` | **Verified** |
| Parameter identifiers from plugins are validated | MP#23, MP#26 | `prv-timeline::PluginParameterId` | `a_plugin_identifier_is_validated_before_it_is_stored` | **Verified** |
| Automation evaluates on the audio thread | MP#21, ADR-0002 | `prv-timeline::AutomationLane::value_at` | binary search, no allocation; `evaluation_is_correct_across_a_large_lane` | **Verified** |
| No automation curve overshoots its points | MP#3A, MP#21 | `prv-timeline::Interpolation` | `no_shape_ever_leaves_the_range_its_points_defined`, `every_shape_is_monotone` | **Verified** |
| Editing snaps to the musical grid | MP#21 | `prv-timeline::Snap` | `an_added_clip_snaps_to_the_grid` | **Verified** |
| Editing is non-destructive | MP#3C, MP#9, MP#21 | `prv-timeline::Timeline` | `trimming_the_front_does_not_slide_the_audio`, `splitting_is_structural_and_changes_no_sound` | **Verified** |
| A destructive edit is refused, not resolved | MP#9, MP#21 | `prv-timeline::EditError::Overlap` | `overlapping_clips_on_one_lane_are_refused_rather_than_truncated` | **Verified** |
| Automation is undoable like any other edit | MP#9, MP#21 | `prv-project::OperationPayload` | `an_automation_edit_is_undoable_like_any_other`, `disabling_a_lane_keeps_its_points_and_undoes_cleanly` | **Verified** |
| Values from a foreign log are validated at the fold | MP#24, MP#26 | `prv-project::ProjectState::apply` | `an_automation_value_from_the_log_is_clamped_before_it_is_stored` | **Verified** |
| The timeline is built from the document | ADR-0003, ADR-0007 | `prv-timeline::Timeline::from_project` | `a_timeline_is_built_from_the_materialised_document` | **Verified** |
| An unopenable project is never the outcome | MP#9, MP#24 | `prv-timeline::Timeline::from_project` | `a_document_the_timeline_cannot_hold_is_reported_rather_than_clamped` | **Verified** |
| A generated mix is editable like a hand-made one | MP#3B, MP#9 | `prv-mix::render` | `applying_the_operations_produces_the_set_and_undoing_them_removes_it` | **Verified** |
| Transitions overlap rather than abut | MP#3B, MP#21 | `prv-mix::render` | `tracks_overlap_rather_than_abutting` | **Verified** |
| The technique is explained by the same evidence as the choice | ADR-0006 | `prv-mix::render::choose_technique` | `the_technique_follows_the_evidence`, `better_evidence_never_produces_a_more_cautious_technique` | **Verified** |
| Generated transitions are editable | MP#3B, MP#21 | `prv-mix::render::automation_for` | `every_transition_produces_automation_on_both_lanes` | **Verified** |
| Identity allocation stays with the caller | MP#24 | `prv-mix::render::PlacementIds` | `identifiers_come_from_the_caller_and_never_repeat` | **Verified** |
| The set carries the tempo it runs at | MP#3B, MP#21 | `prv-project::OperationPayload::SetTempo` | `a_tempo_change_is_a_document_value_like_any_other`, `the_set_records_the_tempo_it_runs_at` | **Verified** |
| Transitions land on the analysed structure | MP#3B, MP#20 | `prv-mix::render::transition_start` | `a_transition_lands_on_the_outgoing_tracks_exit_point`, `an_exit_point_too_late_to_use_is_clamped_rather_than_trusted` | **Verified** |
| A hand edit and a generated edit are the same object | MP#3B, ADR-0003 | `prv-timeline::Edit` | `an_edit_applied_to_the_log_rebuilds_the_same_timeline`, `a_split_records_both_halves` | **Verified** |
| Deleting a clip does not silently lose its automation | MP#9 | `prv-timeline::Timeline::remove` | `removing_a_clip_records_its_automation_going_too` | **Verified** |
| A trimmed clip does not slide on reload | MP#3C, MP#21 | `prv-project::OperationPayload::SetPlacementSource` | `a_source_offset_is_recorded_without_redefining_an_older_operation`, `an_edit_applied_to_the_log_rebuilds_the_same_timeline` | **Verified** |
| Cloud and UI | MP#24, MP#17 | — | — | Not started |

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
