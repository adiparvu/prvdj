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
| No engine knows what tier it is running under | MP#4, MP#29 | `prv-entitlements` isolated | `tools/check-architecture.sh` rule 7, exercised against a deliberate violation | **Enforced** |
| No credential material is ever committed | MP#26 | whole repository | `tools/check-architecture.sh` rule 8, exercised against a planted key | **Enforced** |

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
| Time stretching, key lock | MP#3A | `prv-dsp::TimeStretch` | `stretching_changes_the_length_and_leaves_the_pitch_alone`, `a_ratio_of_one_is_transparent` | **Verified** |
| Key shifting without changing tempo | MP#3A, MP#20 | `prv-dsp::PitchShift` | `shifting_the_key_leaves_the_length_alone` | **Verified** |
| Vari-speed does not alias | MP#3A, MP#15 | `prv-dsp::Resampler` | `speeding_up_removes_what_would_otherwise_fold_back` | **Verified** |
| A stereo splice is one splice | MP#15 | `prv-dsp::TimeStretch` | `both_channels_are_spliced_at_the_same_place` | **Verified** |
| Rendered energy does not depend on the order of a span | MP#2, MP#20 | `prv-waveform::Tile::fold` | `folding_a_span_does_not_depend_on_the_order_of_its_tiles`, `the_energy_of_a_span_is_the_energy_of_its_samples` | **Verified** |
| A control value from outside cannot poison the signal path | MP#15, MP#18 | `prv-dsp::ThreeBandEq::set_crossovers` | `a_non_finite_crossover_never_reaches_the_coefficients` | **Verified** |
| Master limiter, true peak | MP#3A, MP#3C | `prv-dsp::Limiter` | `nothing_leaves_above_the_ceiling`, `it_limits_the_peak_a_converter_would_produce_not_the_sample_peak` | **Verified** |
| The limiter is transparent below its ceiling | MP#15 | `prv-dsp::Limiter` | `a_signal_already_under_the_ceiling_comes_out_unchanged` | **Verified** |
| Gain reduction never steps | MP#18 | `prv-dsp::Limiter` | `the_gain_moves_in_straight_lines_and_never_steps`, `the_gain_is_already_down_when_the_peak_arrives` | **Verified** |
| Master bus loudness metering | MP#3A, MP#3C | `prv-dsp::LoudnessMeter` | `a_full_scale_sine_reads_the_figure_the_scale_is_anchored_to`, `the_weighting_is_the_same_at_every_accepted_rate` | **Verified** |
| The live meter and the export report agree | MP#3C, MP#25 | `prv-dsp::k_weighting` | one derivation; `prv-analysis`'s published-table tests exercise it | **Verified** |
| A gesture of several operations gets several identities | MP#24, ADR-0003 | `prv-project::OperationLog::author_all` | `a_run_of_operations_gets_a_run_of_identities` | **Verified** |
| An inverse may need more than one operation | MP#9, ADR-0003 | `prv-project::ProjectState::inverses_of` | `undoing_a_removal_restores_where_the_clip_began_in_its_source` | **Verified** |
| Undo never silently discards a collaborator's work | MP#24, MP#9 | `prv-project::Undo::Superseded` | `an_undo_that_would_revert_somebody_elses_later_edit_is_refused`, `an_edit_to_something_else_does_not_block_an_undo` | **Verified** |
| A project renders to audio | MP#3A, MP#3C | `prv-render::Renderer` | `the_result_does_not_depend_on_the_block_size`, `rendering_the_same_block_twice_gives_the_same_samples` | **Verified** |
| The render honours where a clip begins in its media | MP#3C, MP#21 | `prv-render::Renderer` | `the_source_offset_says_which_part_of_the_media_is_heard`, `overlapping_placements_sum` | **Verified** |
| An export knows what it could not read | MP#3C | `prv-render::RenderReport` | `a_source_that_falls_short_is_reported_rather_than_silently_silent` | **Verified** |
| Branching never loses work to a name collision | MP#9, MP#24 | `prv-project::OperationLog::branch_at` | `a_branch_never_reuses_a_name_the_trunk_already_gave_out`, `two_different_edits_under_one_name_are_reported_rather_than_dropped` | **Verified** |
| Recording and encoding | MP#3A | — | needs a file and an encoder, both outside the core | Not started |

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
| Mix planner | MP#3B, ADR-0006 | `prv-mix::plan` | `a_plan_reaches_the_requested_length_without_repeating_a_track`, `a_set_moves_forward_by_one_handover_at_a_time` | **Verified** |
| A plan is as long as it renders | MP#3B, MP#12 | `prv-mix::pacing` | `a_rendered_set_is_as_long_as_the_plan_said_it_would_be`, `the_planner_and_the_renderer_agree_about_where_a_record_hands_over` | **Verified** |
| A set that falls short says so | MP#3B, MP#12 | `prv-mix::MixPlan::duration_error` | `a_short_set_is_reported_as_short_rather_than_as_a_perfect_match` | **Verified** |
| A record hands over where the music offers it | MP#3B, MP#21 | `prv-mix::pacing::advance` | `a_record_hands_over_at_its_exit_point`, `an_exit_point_too_late_to_use_is_clamped_rather_than_trusted` | **Verified** |
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
| What the planner values is a decision, not six numbers | MP#3B, ADR-0006 | `prv-mix::Weights::DEFAULT` | `the_ordering_of_the_default_weights_is_not_an_accident`, `a_reversed_weighting_produces_a_different_answer` | **Verified** |
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
| History stays legible rather than being truncated | MP#24, MP#9 | `prv-sync::backup::thin` | `a_month_of_work_stays_legible_rather_than_becoming_a_wall`, `everything_from_the_last_hour_survives` | **Verified** |
| A restore point the user named is never discarded | MP#9, MP#24 | `prv-sync::PointKind::Deliberate` | `a_point_somebody_named_is_never_discarded`, `the_named_points_alone_may_exceed_the_bound` | **Verified** |
| Thinning is safe to run on every save | MP#24 | `prv-sync::backup::thin` | `thinning_twice_changes_nothing` | **Verified** |
| The project refers to media, never contains it | MP#29, ADR-0003 | `prv-project::TrackRef` | structural — a placement holds a reference | **Verified** |
| Offline-first | MP#1, MP#24 | ADR-0001, ADR-0006 | the core has no network dependency, enforced by rule 1 | **Enforced** |
| The language boundary is coarse-grained | MP#4, ADR-0001 | `prv-ffi::Engine` | `a_host_can_start_an_engine_place_a_track_and_hear_it` | **Verified** |
| The language boundary is versioned | MP#4 | `prv-ffi::abi` | `a_host_from_another_major_version_is_turned_away`, the C host's own version check | **Verified** |
| A panic never crosses into C | ADR-0002, MP#15 | `prv-ffi::guard` | `a_panic_becomes_a_status_rather_than_undefined_behaviour`, `a_panic_inside_a_try_body_is_caught_too` | **Verified** |
| Bindings are generated, never hand-written | MP#4, MP#28 | `bridgegen` | architecture rule 9, `the_committed_header_is_what_the_generator_produces` | **Enforced** |
| The header and the library actually agree | MP#27 | `prv-ffi/tests/c_host.rs` | `a_c_host_can_drive_the_boundary_through_the_generated_header` | **Verified** |
| A host cannot hold the boundary wrongly | MP#4 | `PRVCore.Engine` | `the_source_outlives_the_engines_ability_to_call_it`, `a_buffer_too_small_for_the_block_is_caught_before_the_boundary` | **Verified** |
| A host can plan a set | MP#3B, MP#19 | `prv-ffi::planning` | `a_host_can_build_a_library_plan_a_set_and_read_it_back`, `the_whole_product_in_one_test_library_to_plan_to_timeline_to_audio` | **Verified** |
| A generated mix is an ordinary edit | MP#3B, MP#9, ADR-0003 | `prv_planner_apply` | `a_plan_becomes_ordinary_operations_on_the_log`, `applying_a_plan_twice_does_not_reuse_a_placement_identity` | **Verified** |
| An unplannable library is told, not shown an empty list | MP#3B, MP#12 | `prv-ffi::Planner::plan` | `planning_with_an_empty_library_is_refused_rather_than_returning_nothing` | **Verified** |
| A host can analyse a track | MP#20, MP#3A | `prv-ffi::analysis` | `a_track_with_a_pulse_gets_a_tempo_and_a_loudness`, `analysesPulsedAudio` | **Verified** |
| A reading that could not be made is absent, not zero | MP#13, MP#20 | `prv-ffi::Analysis` | `silence_has_no_tempo_and_says_so`, `a_reading_that_could_not_be_made_is_absent_rather_than_zero` | **Verified** |
| A track that cannot be analysed is not planned with defaults | MP#3B, MP#13 | `PRVCore.Analysis.candidate(track:)` | `a_track_the_analysis_could_not_read_is_not_faked_up` | **Verified** |
| The whole product path | MP#3B, MP#19, MP#20 | boundary + `PRVCore` | `the_whole_application_analyse_plan_place_play` | **Verified** |
| Nothing leaves the device unless asked for | MP#26 | `prv-ffi::Policy` | `a_new_policy_sends_nothing_anywhere`, `nothingByDefault` | **Verified** |
| Projects are never used for training without permission | MP#26 | `Purpose::ModelTraining` | `trainingIsOptIn` | **Verified** |
| A consent screen can tell content from a fact about it | MP#26 | `prv_purpose_sends_content` | `sending_content_implies_leaving_the_device_but_not_the_reverse` | **Verified** |
| Essential features are never gated | MP#29 | `prv-ffi::Policy` | `every_essential_feature_is_available_on_the_free_tier`, `expiryIsNotLockout` | **Verified** |
| A purchase is not a consent | MP#26, MP#29 | `prv-ffi::Policy` | `consent_and_licence_are_independent`, `independence` | **Verified** |
| The audio thread cannot reach a non-realtime call | ADR-0002 | `PRVCore.RenderHandle` | enforced by the type; `PRVKit.Session` compiles only through it | **Enforced** |
| The application workflow | MP#3B, MS#001 | `PRVKit.Session` | `wholeWorkflow`, `shortTrackIsKept` | **Verified** |
| Presentation holds no business rules | MP#4 | `PRVUI` models vs views | every threshold tested in `PRVUITests`; views format nothing | **Verified** |
| A host can search the collection | MS#001 | `prv-ffi::collection` | `a_search_finds_what_the_user_typed_and_reads_back_in_order`, `searching` | **Verified** |
| Deleting a track never destroys its metadata | MP#9, MS#001 | `prv_collection_restore` | `removing_a_track_hides_it_without_destroying_it`, `removeAndRestore` | **Verified** |
| A string crossing the boundary is never truncated silently | MP#27 | `prv_collection_text_field` | `a_field_that_did_not_fit_reports_the_length_it_needed`, `longTitle`, `unicode` | **Verified** |
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
| An export comes with a report the user can act on | MP#3C | `prv-export::report` | `a_dynamic_mix_that_would_clip_is_a_different_verdict_from_one_that_needs_gain` | **Verified** |
| Nothing is normalised silently | MP#3A | `prv-export::ExportReport::gain_db` | `silence_is_reported_rather_than_normalised`, `an_archive_is_never_gained_and_only_has_to_not_be_clipping` | **Verified** |
| Inter-sample clipping is caught before delivery | MP#3A, MP#3C | `prv-export::DeliveryTarget::true_peak_ceiling_dbtp` | `every_target_that_normalises_has_a_ceiling_below_full_scale` | **Verified** |
| A mix records what went into it | MP#3C, MP#29 | `prv-export::Manifest` | `a_missing_track_is_recorded_rather_than_omitted`, `reproducing_requires_the_analysis_versions_to_match_too` | **Verified** |
| Essential functionality is never artificially restricted | MP#29 | `prv-entitlements::Feature::is_essential` | `every_tier_can_reach_the_users_own_work`, `getting_work_out_is_never_charged_for` | **Verified** |
| An expired licence is not a locked door | MP#9, MP#29 | `prv-entitlements::Licence::expired` | `an_expired_licence_is_the_free_tier_and_not_a_locked_door` | **Verified** |
| A higher tier never grants less | MP#29 | `prv-entitlements::Licence::check` | `tiers_are_cumulative` | **Verified** |
| A denial says what would grant the feature | MP#10, MP#29 | `prv-entitlements::Denial` | `a_denial_says_what_would_grant_the_feature` | **Verified** |
| A privacy choice is not answered with a sales prompt | MP#26, MP#29 | `prv-entitlements::Denial::DisabledByUser` | `a_privacy_choice_is_not_answered_with_a_sales_prompt` | **Verified** |
| Authorisation rules are centralised | MP#26 | `prv-security::authorise` | `roles_are_cumulative`, `a_viewer_can_look_and_do_nothing_else`, `the_owner_can_do_everything` | **Verified** |
| Sandboxed code cannot widen its own authority | MP#23, MP#26, ADR-0005 | `prv-security::Capability::may_be_delegated_to_a_plugin` | `no_manifest_can_hold_authority_over_secrets_consent_or_licensing`, `a_manifest_from_outside_cannot_smuggle_forbidden_bits_in` | **Verified** |
| Plugin permissions are revocable | MP#23, ADR-0005 | `prv-security::PermissionSet::revoke` | `revoking_a_permission_takes_effect`, `a_plugin_starts_with_nothing` | **Verified** |
| A refusal explains, and never offers an impossible remedy | MP#10, MP#26 | `prv-security::Refusal` | `a_refusal_says_whether_asking_the_user_would_help` | **Verified** |
| Nothing is consented to by default | MP#26 | `prv-security::Consents` | `nothing_is_agreed_to_by_default`, `withdrawing_everything_returns_to_the_starting_state` | **Verified** |
| Projects are never used for training without explicit permission | MP#26 | `prv-security::Purpose::ModelTraining` | `training_on_a_users_work_is_reachable_from_nothing_else` | **Verified** |
| The user is told where their work is processed | MP#26 | `prv-security::Purpose::location`, `sends_content` | `a_purpose_that_sends_the_users_own_material_says_so`, `the_indicator_reflects_only_what_actually_leaves` | **Verified** |
| Every purpose is withdrawable | MP#26 | `prv-security::Consents::withdraw` | `every_purpose_can_be_withdrawn_and_withdrawal_is_immediate` | **Verified** |
| No secret is ever transmitted in a log | MP#26 | `prv-security::Field::secret` | `a_record_renders_public_values_and_withholds_the_rest`, `a_diagnostic_about_a_credential_failure_carries_no_credential` | **Verified** |
| A secret is redacted in every rendering, including derived ones | MP#26 | `prv-security::Secret` | `a_struct_that_derives_debug_and_contains_one_is_safe_to_print` | **Verified** |
| No agreement can make a secret loggable | MP#26 | `prv-security::Sensitivity` | `no_agreement_can_put_a_secret_in_a_diagnostic` | **Verified** |
| An audit trail records decisions, not people | MP#26 | `prv-security::audit::Entry` | `an_entry_cannot_be_rendered_into_anything_but_stable_keys`, `the_actor_is_the_kind_and_never_the_person` | **Verified** |
| An audit log never forgets silently | MP#26, MP#9 | `prv-security::AuditLog::discarded` | `an_overflowing_log_says_how_many_it_dropped` | **Verified** |
| Permission and agreement are separate checks | MP#26 | `prv-security` | `permission_and_agreement_are_two_different_questions` | **Verified** |
| The interface never intervenes uninvited for a professional | MP#8 | `prv-settings::ExperienceMode` | `the_professional_mode_is_quiet_rather_than_reduced`, `assistance_decreases_and_never_reverses` | **Verified** |
| A mode changes what is offered, never what is possible | MP#8, MP#29 | `prv-settings::Category::follows_the_mode` | `only_assistance_settings_follow_the_mode`, `no_mode_changes_anything_but_assistance`, `a_professional_gets_quiet_and_loses_nothing` | **Verified** |
| A mode change never overrules an explicit choice | MP#8, MP#9 | `prv-settings::Settings::set_mode` | `changing_mode_never_discards_a_choice_the_user_made`, `withdrawing_a_choice_returns_to_following_the_mode` | **Verified** |
| A platform accommodation is never withdrawn by a preference | MP#8, MP#16 | `prv-settings::Accessibility` | `the_platform_can_turn_reduced_motion_on_and_the_application_cannot_turn_it_off`, `an_accommodation_survives_every_other_preference` | **Verified** |
| Text scaling is limited by the platform, not by the layout | MP#8, MP#16 | `prv-settings::PlatformAccessibility` | `the_text_scale_limit_comes_from_the_platform_not_from_the_layout` | **Verified** |
| An older build does not delete a newer build's preferences | MP#9, MP#24 | `prv-settings::Settings::keep_unknown` | `a_newer_builds_settings_survive_this_one`, `an_unkeepable_entry_is_reported_rather_than_dropped_quietly` | **Verified** |
| A crashing plugin never stops playback | MP#23, ADR-0005 | `prv-plugin::LifecycleState::audio_behaviour` | `nothing_that_can_happen_to_a_plugin_stops_the_graph`, `a_crash_and_an_overrun_both_land_on_bypassed` | **Verified** |
| A plugin that overruns repeatedly is bypassed | MP#23, ADR-0005 | `prv-plugin::Watchdog` | `three_in_a_row_is_the_plugin`, `a_plugin_that_never_has_three_in_a_row_is_still_caught`, `one_late_block_is_not_a_fault` | **Verified** |
| A bypass does not move the music in time | MP#18, MP#23 | `prv-plugin::Registry::compensated_latency_frames` | `bypassing_a_plugin_does_not_move_the_music_in_time` | **Verified** |
| A plugin is pre-instantiated before it is reachable | MP#18, ADR-0005 | `prv-plugin::LifecycleState::Loaded` | `a_plugin_cannot_reach_the_callback_without_being_approved_and_loaded`, `only_running_processes` | **Verified** |
| Unsigned code is never loaded without explicit approval | MP#26, ADR-0005 | `prv-plugin::Manifest::may_load_without_asking` | `unsigned_code_is_never_loaded_without_asking` | **Verified** |
| A plugin gets what the user approved, never what it asked for | MP#23, MP#26 | `prv-plugin::Registry::approve` | `a_plugin_gets_what_the_user_approved_and_never_more_than_it_asked_for`, `a_plugin_is_installed_holding_nothing` | **Verified** |
| Plugin permissions are revocable and revocation is immediate | MP#23 | `prv-plugin::Registry::withdraw`, `lifecycle::advance` | `withdrawing_a_permission_takes_effect_on_the_next_question`, `revocation_applies_everywhere_and_is_final` | **Verified** |
| Declared plugin latency is bounded and compensated | MP#18, MP#23 | `prv-plugin::Manifest::MAX_LATENCY_FRAMES` | `latency_beyond_what_the_graph_will_compensate_is_refused`, `a_plugin_off_the_audio_path_declares_no_latency_whatever_it_says` | **Verified** |
| The certified tier is not self-assignable | MP#23, ADR-0005 | `prv-plugin::ManifestError::TierNotSelfAssignable` | `the_certified_tier_cannot_be_claimed_by_a_file` | **Verified** |
| Musical decisions are computed, never generated | ADR-0006, MP#6 | `prv-ai::AgentKind::decides_musically` | `no_musical_decision_is_ever_made_on_a_server`, `agreeing_to_the_cloud_changes_how_it_reads_and_writes_and_not_what_it_decides` | **Verified** |
| Losing the cloud costs fluency, never capability | ADR-0006, MP#26 | `prv-ai::Capability::available_agent` | `withdrawing_every_agreement_leaves_every_essential_capability_reachable`, `a_two_hour_set_is_planned_with_nothing_agreed_to_and_nothing_sent` | **Verified** |
| A model's output is bounded before the system acts on it | ADR-0006, MP#25 | `prv-ai::Intent::to_goal` | `nothing_a_model_can_emit_becomes_a_goal_the_planner_would_not_accept` | **Verified** |
| The orchestrator's order can be explained | MP#19 | `prv-ai::TaskPlan::schedule` | `the_order_is_the_same_on_every_run`, `a_dependency_always_comes_first` | **Verified** |
| A cycle in a plan is refused, not broken | MP#19 | `prv-ai::TaskError::Cyclic` | `a_cycle_is_refused_rather_than_broken`, `a_task_that_depends_on_itself_is_a_cycle_like_any_other` | **Verified** |
| A task never runs on an input that was not produced | MP#19, MP#25 | `prv-ai::Run::record` | `a_failure_skips_everything_downstream_of_it_transitively`, `a_failure_early_on_never_produces_a_confident_answer_late_on` | **Verified** |
| The user is asked for agreements once, in advance | MP#26, MP#10 | `prv-ai::TaskPlan::missing_agreements` | `what_a_plan_needs_is_known_before_anything_runs`, `a_schedule_says_in_advance_whether_anything_leaves_the_device` | **Verified** |
| A modest machine is not mistaken for a withheld permission | MP#8, MP#26 | `prv-ai::Device` | `a_modest_machine_falls_back_to_the_cloud_and_only_with_an_agreement` | **Verified** |
| Non-critical work is suspended during live playback | MP#19 | `prv-ai::Activity`, `Urgency` | `during_a_performance_background_work_waits_rather_than_being_dropped`, `housekeeping_yields_to_a_performance_and_a_request_does_not` | **Verified** |
| Suspended work is deferred, never dropped | MP#9, MP#19 | `prv-ai::Schedule::deferred` | `during_a_performance_background_work_waits_rather_than_being_dropped`, `the_deferred_set_is_closed_under_dependency` | **Verified** |
| A step somebody is waiting for is never blocked by postponed work | MP#19 | `prv-ai::TaskPlan::schedule` | `nothing_a_requested_task_needs_is_ever_deferred` | **Verified** |
| A request becomes exactly the work it needs | MP#19 | `prv-ai::plan_for` | `analysis_appears_only_when_something_is_unanalysed`, `a_composed_plan_always_schedules` | **Verified** |
| Nothing to do is said, not simulated | MP#10, MP#13 | `prv-ai::plan_for` | `an_analysed_library_asked_to_analyse_itself_produces_nothing_to_do`, `a_request_about_a_set_that_is_empty_says_so` | **Verified** |
| Nothing is recorded without agreement | MP#26 | `prv-telemetry::Counts::record` | `nothing_is_recorded_without_agreement`, `a_user_who_agreed_to_nothing_leaves_no_trace_anywhere` | **Verified** |
| Telemetry is counts, never a behavioural trace | MP#26 | `prv-telemetry::Counts` | `what_is_kept_is_a_total_and_never_an_order`, `counting_is_bounded_by_the_number_of_kinds_not_by_use` | **Verified** |
| Withdrawing an agreement discards what was held | MP#26, MP#9 | `prv-telemetry::Counts::apply` | `withdrawing_an_agreement_discards_what_was_already_held`, `a_cleared_record_is_indistinguishable_from_one_that_never_counted` | **Verified** |
| Crash reporting and usage counting are separate agreements | MP#26 | `prv-telemetry::Counts::purpose_for` | `agreeing_to_crash_reports_is_not_agreeing_to_being_counted`, `every_event_belongs_to_exactly_one_agreement` | **Verified** |
| A diagnostic carrying a credential is never sent | MP#26 | `prv-telemetry::Diagnostic::may_be_sent` | `a_report_containing_a_credential_never_goes_at_any_setting`, `a_secret_is_refused_before_anything_else_is_considered` | **Verified** |
| What a user is shown is what would be sent | MP#26, MP#10 | `prv-telemetry::Counts::report` | `what_a_user_is_shown_is_what_would_be_sent`, `a_report_can_hold_nothing_a_person_could_be_recognised_by` | **Verified** |
| Editing works with no network | MP#24 | `prv-sync::SyncState::editing_is_allowed` | `there_is_no_state_in_which_the_user_cannot_edit`, `six_hours_offline_then_a_reconnection` | **Verified** |
| Nothing the user did is ever dropped in transit | MP#9, MP#24 | `prv-sync::Outbox` | `a_full_outbox_refuses_rather_than_forgetting`, `edits_from_two_devices_do_not_collide` | **Verified** |
| Re-sending after a lost reply is safe | MP#24 | `prv-sync::Outbox::acknowledge` | `acknowledging_twice_is_a_no_op`, `replaying_a_log_on_startup_does_not_duplicate_anything` | **Verified** |
| A pause is honoured exactly | MP#24, MP#26 | `prv-sync::advance` | `a_pause_is_honoured_exactly_and_only_the_user_lifts_it` | **Verified** |
| A conflict blocks transfer and not the person | MP#24, MP#9 | `prv-sync::SyncState::Conflicted` | `a_conflict_stops_transfer_and_nothing_else`, `a_conflict_pauses_the_transfer_and_not_the_person` | **Verified** |
| Only the sound coming out now interrupts a performance | MP#8, MP#19, MP#24 | `prv-notify::Notice::concerns_the_sound_right_now` | `only_the_sound_coming_out_right_now_reaches_someone_on_stage`, `exactly_three_things_may_interrupt_a_set` | **Verified** |
| A withheld notice is held, never discarded | MP#9, MP#24 | `prv-notify::Notifications::release` | `nothing_withheld_during_a_set_is_thrown_away` | **Verified** |
| Repetition is summarised with a count | MP#10, MP#24 | `prv-notify::Pending::occurrences` | `forty_analysed_tracks_are_one_notice_with_a_count`, `a_question_is_never_merged_away` | **Verified** |
| Cloud transport and UI | MP#24, MP#17 | — | — | Not started |

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
