# Implementation Status

Statuses are those defined by MP#31. Two additional qualifiers are used because
the build environment makes the distinction material:

- **Verified** — compiled and tested in continuous integration.
- **Authored** — written against specification, not yet compiled on a machine
  with the required SDK.

Code that is authored is never reported as working. This is a direct requirement
of MP#13 (*no placeholder implementations*) and MP#27 (*no feature is complete
without verification*).

_Last updated: Sprint 34._

## Sprint 0 — Foundation

| Item | Status | Qualifier | Notes |
|------|--------|-----------|-------|
| ADR-0001 platform & language strategy | Completed | — | |
| ADR-0002 realtime audio core | Completed | — | |
| ADR-0003 project document & persistence | Completed | — | |
| ADR-0004 stem separation | Completed | — | Model evaluation deferred to Phase 3 by design |
| ADR-0005 plugin isolation | Completed | — | |
| ADR-0006 AI decision architecture | Completed | — | |
| Master Technical Specification v0.1 | Completed | — | Sections filled as modules enter design |
| Repository structure | Completed | Verified | Builds from clean checkout |
| `prv-time` musical time & transport clock | Completed | Verified | Unit tested, zero-drift property tested |
| `prv-rt` realtime primitives | Completed | Verified | Allocation test, SPSC queue, triple buffer, parameter smoothing |
| `prv-harmony` harmonic model | Completed | Verified | Camelot wheel, compatibility scoring, property tested |
| Design token source & generator | Completed | Verified | Generates Swift and JSON from one source |
| Continuous integration pipeline | Completed | — | Linux jobs run; macOS jobs declared, not yet exercised |

## Sprint 1 — Core Experience, first slice

| Item | Status | Qualifier | Notes |
|------|--------|-----------|-------|
| `prv-time` multi-segment tempo map | Completed | Verified | Removes the single-segment limitation |
| `prv-time` beat grid and snapping | Completed | Verified | Beat, bar, phrase, division and sample resolutions |
| `prv-transport` playback state machine | Completed | Verified | All 405 state-event-intent combinations defined |
| `prv-transport` loops and slip | Completed | Verified | Phase-correct wrapping, including loops shorter than a block |
| `prv-dsp` processor contract and chain | Completed | Verified | Bounded length, bypass, latency reporting |
| `prv-dsp` gain | Completed | Verified | Ramped, one ramp shared across channels |
| `prv-dsp` three-band equaliser | Completed | Verified | Linkwitz-Riley crossover, true kill, flat at unity |
| `prv-dsp` filter | Completed | Verified | Exponential sweep, stable under fast modulation |
| Allocation gate extended to the signal path | Completed | Verified | A full channel strip under control movement |
| Requirements traceability matrix | Completed | — | Every requirement linked to its code and test |

## Sprint 2 — Waveform

| Item | Status | Qualifier | Notes |
|------|--------|-----------|-------|
| `prv-waveform` tile model | Completed | Verified | Min, max and energy; clipping detected; damaged frames skipped |
| `prv-waveform` resolution ladder and builder | Completed | Verified | Five bands, each built from the audio; chunk-size independent |
| `prv-waveform` viewport rendering | Completed | Verified | Allocation-free into a caller buffer; never upscales |

## Sprint 3 — Project operation log

| Item | Status | Qualifier | Notes |
|------|--------|-----------|-------|
| `prv-project` operation envelope and version vectors | Completed | Verified | Concurrency detected as a fact, not estimated |
| `prv-project` materialisation and undo | Completed | Verified | Undo appends an inverse; the log never shrinks |
| `prv-project` named versions and branching | Completed | Verified | Both are positions in the log |
| `prv-project` incremental sync and merge | Completed | Verified | Convergence proven independent of arrival order |

## Sprint 4 — Music library

| Item | Status | Qualifier | Notes |
|------|--------|-----------|-------|
| `prv-library` track entity and states | Completed | Verified | Missing files keep their ratings, tags and playlists |
| `prv-library` prefix search index | Completed | Verified | Cost proportional to the answer, not the library |
| `prv-library` composable filters and sorting | Completed | Verified | Harmonic filtering uses the planner's own model |
| `prv-library` duplicate detection | Completed | Verified | Four signals, ordered by confidence, never acted on |
| `prv-library` collections | Completed | Verified | Overlapping, and they survive removal |

## Sprint 5 — Audio analysis: rhythm

| Item | Status | Qualifier | Notes |
|------|--------|-----------|-------|
| `prv-analysis` discrete Fourier transform | Completed | Verified | Written rather than depended upon; checked against the direct sum |
| `prv-analysis` analysis windows | Completed | Verified | Two shapes, each chosen by requirement; sidelobe rejection measured |
| `prv-analysis` streaming short-time transform | Completed | Verified | Centred, never materialised; a track costs kilobytes not megabytes |
| `prv-analysis` spectral-flux novelty curve | Completed | Verified | Survives a 20 dB level change; sustained material produces no events |
| `prv-analysis` tempo estimation | Completed | Verified | Autocorrelation through the transform; octave alternatives always exposed |
| `prv-analysis` beat and downbeat tracking | Completed | Verified | Dynamic programme; coasts through a breakdown and recovers |
| `prv-analysis` fitted beat grid | Completed | Verified | Tempo accurate to a hundredth of a beat per minute over 192 beats |
| Central confidence scale | Completed | Verified | One mapping to Master Prompt #25's five labels, used everywhere |

## Sprint 6 — Audio analysis: tone, loudness and structure

| Item | Status | Qualifier | Notes |
|------|--------|-----------|-------|
| `prv-analysis` chroma extraction | Completed | Verified | Spectral peaks only, normalised per frame; drums do not colour the profile |
| `prv-analysis` key detection | Completed | Verified | Krumhansl-Kessler profiles; the relative key is an alternative, not an error |
| `prv-analysis` loudness to BS.1770 | Completed | Verified | Derived coefficients reproduce the standard's table; gated integrated, range |
| `prv-analysis` true peak | Completed | Verified | Polyphase band-limited interpolation, not linear |
| `prv-analysis` structure segmentation | Completed | Verified | Bar-resolution checkerboard novelty; no boundaries on material with none |
| `prv-analysis` track profile | Completed | Verified | Stages versioned independently; staleness propagates to dependants |

## Sprint 7 — The mix planner

| Item | Status | Qualifier | Notes |
|------|--------|-----------|-------|
| `prv-mix` goal model | Completed | Verified | Five energy shapes, three creativity settings; the seam ADR-0006 draws |
| `prv-mix` candidate projection | Completed | Verified | Missing facts represented, never defaulted |
| `prv-mix` constraint model | Completed | Verified | A violating move is not generated, at any creativity setting |
| `prv-mix` objective and evidence | Completed | Verified | Six weighted components, each retained with the decision |
| `prv-mix` beam search | Completed | Verified | Deterministic; follows the energy shape; no track repeats |
| `prv-mix` distinct alternatives | Completed | Verified | Version A/B/C differ by construction, not by sampling |

## Sprint 8 — The timeline

| Item | Status | Qualifier | Notes |
|------|--------|-----------|-------|
| `prv-timeline` parameter addressing | Completed | Verified | Closes the limitation open since Sprint 1; survives storage and reordering |
| `prv-timeline` parameter descriptors | Completed | Verified | Range, unit and curve; normalisation round-trips on every curve |
| `prv-timeline` automation lanes | Completed | Verified | Binary-search evaluation, allocation-free; no shape overshoots |
| `prv-timeline` clips and lanes | Completed | Verified | Trimming the front does not slide the audio; splitting changes no sound |
| `prv-timeline` editing with snapping | Completed | Verified | Opt-out per edit rather than a global mode; overlap refused, not resolved |

## Sprint 9 — Automation joins the document

| Item | Status | Qualifier | Notes |
|------|--------|-----------|-------|
| ADR-0007 the project crate is the document model | Completed | — | Refines ADR-0003's scope; resolves a dependency cycle structurally |
| Parameter identity moved to `prv-project` | Completed | Verified | Identity is persisted, description is not; the line is persistence |
| Automation operations in the log | Completed | Verified | Undo, versions, branching and sync inherited rather than reimplemented |
| Automation in the state fold | Completed | Verified | Values clamped at the fold, once, where a foreign log arrives |
| `Timeline::from_project` | Completed | Verified | Unholdable clips reported, never clamped into a lane the user did not choose |

## Sprint 10 — A plan becomes edits

| Item | Status | Qualifier | Notes |
|------|--------|-----------|-------|
| `prv-mix::render` transition techniques | Completed | Verified | Derived from the planner's evidence, not chosen; ordering survives recalibration |
| `prv-mix::render` operation emission | Completed | Verified | An AI mix is an ordinary edit: undoable, branchable, editable, with no code for it |
| Overlap length from evidence | Completed | Verified | Quantised to whole bars; better matches given more room |
| Transition automation | Completed | Verified | Every technique expressed as automation, so a generated transition is editable |

## Sprint 11 — Transitions land where the music invites them

| Item | Status | Qualifier | Notes |
|------|--------|-----------|-------|
| Tempo operations in the log | Completed | Verified | Keyed by frames, because a tick position depends on the map itself |
| Tempo in the state fold, with inverses | Completed | Verified | Undoes to what was there, or to its absence |
| Transitions placed at the analysed exit | Completed | Verified | Falls back to the naive placement; a stale exit point is clamped |
| The set records the tempo it runs at | Completed | Verified | Changed at the end of a transition, not the start |

## Sprint 12 — Every edit is an operation

| Item | Status | Qualifier | Notes |
|------|--------|-----------|-------|
| `prv-timeline::Edit` | Completed | Verified | A gesture is a list of operations, not one payload per gesture |
| Clips carry their media reference | Completed | Verified | Makes ADR-0003's "the project refers to media" structural |
| Edits emit operations | Completed | Verified | The cache and the fold agree, tested end to end |
| Deleting a clip records its automation going with it | Completed | Verified | Undoing a deletion restores both |

## Sprint 13 — A trimmed front survives a reload

| Item | Status | Qualifier | Notes |
|------|--------|-----------|-------|
| `SetPlacementSource` operation | Completed | Verified | A new operation rather than a redefined one, as ADR-0003 requires |
| Source offset in the fold, with an inverse | Completed | Verified | A log that predates it means zero, which is what it meant when written |
| Trim and split emit it | Completed | Verified | The audio under a clip no longer slides on reload |

## Sprint 14 — The personal profile

| Item | Status | Qualifier | Notes |
|------|--------|-----------|-------|
| Objective weights are a value, not a constant | Completed | Verified | ADR-0006 says they come from the scenario and the profile; a constant could do neither |
| `prv-learning` observation model | Completed | Verified | Behaviour rather than a preferences screen; an ignored suggestion teaches nothing |
| `prv-learning` inference | Completed | Verified | Correlation, not average — a component with no contrast is not learned from |
| Confidence gating | Completed | Verified | Influence arrives gradually; no observation changes the system's character |
| Bounds that learning cannot cross | Completed | Verified | No profile can switch a component off or reach a hard constraint |
| Explain, correct and delete | Completed | Verified | The user owns it; a cleared profile is indistinguishable from a new one |

## Sprint 15 — Delivery

| Item | Status | Qualifier | Notes |
|------|--------|-----------|-------|
| `prv-export` delivery targets | Completed | Verified | Published figures in one place, each with the reason it is what it is |
| `prv-export` compliance report | Completed | Verified | Computed before the render; "needs gain" and "would clip" are different verdicts |
| Dither derived rather than configured | Completed | Verified | Right in all four combinations of depth and format |
| `prv-export` provenance manifest | Completed | Verified | Licensing, reproducibility and integrity from one structure |

## Sprint 16 — What a licence may and may not withhold

| Item | Status | Qualifier | Notes |
|------|--------|-----------|-------|
| `prv-entitlements` tiers and features | Completed | Verified | Ordered tiers; a feature is gated by "at least this tier", never by a list |
| Essential features | Completed | Verified | Playing, browsing, editing and exporting your own work, at every tier, checked over the whole matrix |
| Expiry is the free tier, not a locked door | Completed | Verified | Essential features survive; the user's own privacy choices survive with them |
| Denials that explain | Completed | Verified | A tier denial names the tier; a user's own choice is not answered with a sales prompt |
| Architecture rule 7 — no engine depends on entitlements | Completed | Verified | Enforced by `tools/check-architecture.sh`; the rule was tested against a deliberate violation |

## Sprint 17 — Security policy

| Item | Status | Qualifier | Notes |
|------|--------|-----------|-------|
| `prv-security` authorisation | Completed | Verified | One decision point; roles cumulative; six capabilities no manifest can ever hold |
| `prv-security` consent | Completed | Verified | Nothing granted by default; training reachable from nothing else; every purpose withdrawable |
| `prv-security` redaction | Completed | Verified | A log field for a secret takes no value, so a token cannot be put in a diagnostic |
| `prv-security` secret handling | Completed | Verified | Redacted in every rendering; compared without an early return; zeroised on drop, best effort |
| `prv-security` audit | Completed | Verified | Four closed vocabularies and no personal data; discards are counted, never silent |
| Architecture rule 8 — no credential material committed | Completed | Verified | Scans tracked and staged files; exercised against a planted key |
| Architecture rule 2 no longer matches prose | Completed | Verified | Comments stripped before matching; still fails on real unsafe code |

## Sprint 18 — Preferences

| Item | Status | Qualifier | Notes |
|------|--------|-----------|-------|
| `prv-settings` experience modes | Completed | Verified | A mode changes what is offered, never what is possible; assistance decreases monotonically |
| `prv-settings` setting vocabulary | Completed | Verified | Only assistance settings may follow the mode, checked over every setting in both directions |
| `prv-settings` document | Completed | Verified | Only choices are stored; a withdrawn choice returns to following the mode |
| Unrecognised settings preserved | Completed | Verified | An older build no longer deletes a newer build's preferences on launch |
| `prv-settings` accessibility | Completed | Verified | The platform can add an accommodation and nothing here can withdraw one |

## Sprint 19 — Plugins

| Item | Status | Qualifier | Notes |
|------|--------|-----------|-------|
| `prv-plugin` isolation tiers | Completed | Verified | A tier on the audio thread is either interruptible or certified; never neither |
| `prv-plugin` manifest | Completed | Verified | A claim, not a fact; forbidden authority narrowed rather than refused |
| `prv-plugin` lifecycle | Completed | Verified | Every state processes or passes through; there is no event whose outcome is silence |
| `prv-plugin` watchdog | Completed | Verified | Two rules; allocation-free, total, safe to call from the callback |
| `prv-plugin` registry | Completed | Verified | Granted is not requested; a bypass does not move the music in time |
| Revocation | Completed | Verified | Applies from any state, is terminal, and takes effect on the next question |

## Sprint 20 — The orchestrator

| Item | Status | Qualifier | Notes |
|------|--------|-----------|-------|
| `prv-ai` intent boundary | Completed | Verified | Every value bounded once, at the edge; adversarial sweep over durations and tempos |
| `prv-ai` agent registry | Completed | Verified | No musical decision runs on a server; every cloud agent names its agreement |
| Device capability model | Completed | Verified | "Agreed to" and "can be done here" are separate questions |
| `prv-ai` task graph | Completed | Verified | Deterministic order; a cycle is refused rather than broken |
| Agreements known before anything runs | Completed | Verified | The user is asked once, up front, for exactly what is needed |
| `prv-ai` run record | Completed | Verified | A failed step skips its dependents transitively, each naming its cause |

## Sprint 21 — A request becomes work

| Item | Status | Qualifier | Notes |
|------|--------|-----------|-------|
| `prv-ai::plan_for` | Completed | Verified | The situation is a parameter, never a field; every composed plan schedules |
| Empty answers are honest | Completed | Verified | An analysed library asked to analyse itself produces nothing, not a no-op |
| A request about an empty set is refused | Completed | Verified | More useful than an empty answer, which reads as "nothing to say" |
| Urgency, and its propagation | Completed | Verified | Anything a requested task needs is itself requested |
| Live playback defers background work | Completed | Verified | Deferred, never dropped; the deferred set is closed under dependency |

## Sprint 22 — Telemetry

| Item | Status | Qualifier | Notes |
|------|--------|-----------|-------|
| `prv-telemetry` event vocabulary | Completed | Verified | Closed list, no payload; faults and usage are separable |
| Nothing recorded without agreement | Completed | Verified | Not recorded-and-not-sent — no buffer exists for a mistake to release |
| Counts, never a trace | Completed | Verified | Reordering the same events produces an identical record |
| Withdrawal discards what was held | Completed | Verified | A cleared record is indistinguishable from one that never counted |
| `prv-telemetry` diagnostics | Completed | Verified | A credential-bearing report is refused at every setting, checked first |
| What the user is shown is what would be sent | Completed | Verified | One structure, rendered through the redaction module |

## Sprint 23 — Synchronisation and notices

| Item | Status | Qualifier | Notes |
|------|--------|-----------|-------|
| `prv-sync` state machine | Completed | Verified | Editing is allowed in every state; a pause is lifted only by the user |
| `prv-sync` outbox | Completed | Verified | Refuses when full rather than discarding — the opposite of the audit log, for a stated reason |
| Idempotent acknowledgement | Completed | Verified | Replaying a log on startup duplicates nothing; a lost reply is boring |
| `prv-notify` interruption rule | Completed | Verified | Exactly three notices may reach someone on stage, named individually |
| Notices are held, never dropped | Completed | Verified | What waits during a set is released afterwards, coalesced with a count |
| A question is never merged away | Completed | Verified | Two permission requests are two decisions |

## Sprint 24 — The master limiter

| Item | Status | Qualifier | Notes |
|------|--------|-----------|-------|
| `prv-dsp::Limiter` | Completed | Verified | Nothing leaves above the ceiling, to within a few units in the last place |
| True-peak detection on the signal path | Completed | Verified | Acts on a signal whose samples are under the ceiling and whose reconstruction is not |
| Straight-line gain, never a step | Completed | Verified | Bounded by the steepest line to silence over the delay |
| Delay reported for compensation | Completed | Verified | Look-ahead plus the detector's own delay, both counted |
| Allocation gate extended | Completed | Verified | The limiter renders 175 000 blocks in a chain with zero allocations |

## Sprint 25 — Key lock, and two defects it uncovered

| Item | Status | Qualifier | Notes |
|------|--------|-----------|-------|
| `prv-dsp::TimeStretch` | Completed | Verified | Waveform-similarity overlap-add; pitch held within 8 Hz across the ratio range |
| Stereo spliced once, not twice | Completed | Verified | The search runs on the sum; identical channels stay identical to 1e-6 |
| `prv-dsp::Resampler` | Completed | Verified | Cutoff follows the rate; a tone above the new Nyquist does not fold back |
| `prv-dsp::PitchShift` | Completed | Verified | Four semitones moves 440 Hz to 554 Hz and leaves the length alone |
| Allocation gate for the streaming path | Completed | Verified | Caught a `to_vec` in both `read` paths before it shipped |
| `Tile::fold` replaces a chained merge | Completed | Verified | Rendered energy no longer depends on the order of the span |
| Crossovers validated at the boundary | Completed | Verified | A non-finite crossover no longer poisons the equaliser permanently |

## Sprint 26 — The live meter, and four defects an adversarial review found

| Item | Status | Qualifier | Notes |
|------|--------|-----------|-------|
| `prv-dsp::LoudnessMeter` | Completed | Verified | Momentary and short-term; a full-scale sine reads −3.01 LUFS at three rates |
| One K-weighting derivation, two users | Completed | Verified | `prv-analysis` now takes the filter from `prv-dsp`; its published-table tests still pass |
| `OperationLog::author_all` | Completed | Verified | A run of operations gets a run of identities; the loop was minting collisions |
| `ProjectState::inverses_of` | Completed | Verified | An inverse may need more than one operation; undoing a removal restores the source offset |
| `Timeline::add` records the source offset | Completed | Verified | A clip added with one no longer reloads at zero |
| An undo that would revert another device is refused | Completed | Verified | `Undo::Superseded` names who, rather than discarding their work silently |

## Sprint 27 — The render path, and the branch collision

| Item | Status | Qualifier | Notes |
|------|--------|-----------|-------|
| `prv-render::Source` port | Completed | Verified | The core decides which audio; the platform reads it |
| `prv-render::Renderer` | Completed | Verified | Identical output at block sizes from 1 to 1024 |
| Automated level, per sample | Completed | Verified | A level held across a block is a click at the block rate |
| Short reads reported | Completed | Verified | Named once per placement, not once per block |
| A branch no longer reuses the trunk's identities | Completed | Verified | The high-water mark of what a log has *issued*, not only what it has seen |
| A name collision is a conflict, not a duplicate | Completed | Verified | `ConflictKind::SameNameDifferentWork`; a retried delivery stays boring |

## Sprint 28 — Restore points, and two clocks that disagreed

| Item | Status | Qualifier | Notes |
|------|--------|-----------|-------|
| `prv-sync::backup` retention | Completed | Verified | Thinned by age band, not truncated; a named point is never discarded |
| `prv-mix::pacing` | Completed | Verified | One rule for how far a set advances, called by both the planner and the renderer |
| A plan's length is the length it renders at | Completed | Verified | It was not: a forty-minute plan rendered as thirteen minutes and reported no error |
| `Goal` carries its sample rate | Completed | Verified | A duration in frames is not a duration until something says how long a frame is |
| A short set is reported as short | Completed | Verified | `duration_error` is also what the search sorts by, so the wrong number was choosing plans |
| The ordering of the default transition weights | Completed | Verified | Reversing them once passed the whole suite; the ordering is now asserted, and asserted to matter |

### The defect, stated plainly

The planner laid tracks end to end and the renderer started each track at the
outgoing track's *exit point* — where the analysis says a record wants to be
left. Two models of the same clock, in two files, neither aware of the other.

For material with no exit points the two nearly agree, which is why every
existing test passed: they used tracks with no analysis attached. For analysed
material — the normal case, and the case the product exists for — eight
five-minute records whose exit points sit a quarter of the way in produced a
**forty-minute plan and a thirteen-minute mix**, with `duration_error` reporting
the set as a perfect match for what the user asked for.

The second half is what made it serious rather than merely wrong. A planner that
comes up short can say so, and the user can accept it or ask for more. A planner
that comes up short and reports success cannot be caught — and `duration_error`
is also the number the beam search sorts by, so the wrong clock was choosing
between plans as well as describing them.

The fix is one function, in `prv-mix::pacing`, called from both sides. It was
found by a probe left behind by the Sprint 26 adversarial review, read rather
than deleted.


### A second defect, and this one was mine

While Sprint 28 was being staged, `prv-mix::Weights::DEFAULT` — the six numbers
that decide what the planner values in a transition — was **reversed** in the
commit. Harmonic compatibility fell from the most important thing about a move
(0.30) to the least (0.05); vocal collision rose from least to most.

It was not an edit anybody made on purpose. A mutation probe from the Sprint 26
adversarial review was sitting in the working tree, and `git add` on the file
swept it into the commit alongside two legitimate changes to the same file. It
was caught afterwards by `git log -L` on the constant, and repaired in the
following commit.

Two things are worth recording rather than quietly fixing.

**The whole suite passed with the weights reversed.** Seventy-one tests, and not
one of them said what the constant was *for*. A planner that would rather clash
two keys than overlap two vocals was indistinguishable, to the test suite, from
the one the product is supposed to be. That is a coverage gap that existed since
Sprint 14 and had nothing to do with the mutation; the mutation only revealed it.
`the_ordering_of_the_default_weights_is_not_an_accident` now pins the ordering —
deliberately not the values, which Phase 3 calibration will move — and
`a_reversed_weighting_produces_a_different_answer` asserts that the ordering
changes what the planner decides, so the first test is not theatre.

**Staging a whole file is not the same as staging your own work.** Every file in
a commit needs to be diffed, not just listed, when anything else has write access
to the tree. The review workflow has since been stopped.

## Sprints 29–30 — The application layer begins

Twenty-three crates of musical judgement, and no way for a host to reach any of
it. That is why the application did not exist yet, and it is what these two
sprints are about.

| Item | Status | Qualifier | Notes |
|------|--------|-----------|-------|
| `prv-ffi` — the C ABI | Completed | Verified | One opaque handle; every entry point guarded against a panic reaching C |
| Panic containment at the boundary | Completed | Verified | Unwinding into C is undefined behaviour; `PRV_PANICKED` is a defect report, not a condition |
| Audio as a host callback | Completed | Verified | The renderer reads what it needs in the order it needs it; a push model cannot see a seek coming |
| `bridgegen` — generated header and module map | Completed | Verified | Each declaration paired with a typed reference to the real function, so a changed signature is a compile error |
| A C host drives the boundary | Completed | Verified | Compiled with `-Werror` against the committed header, linked to the real archive, rendering through a C callback |
| Architecture rule 9 — bindings are current | Completed | Enforced | Verified against a deliberately drifted header |
| `PRVCore` — safe Swift over the boundary | Completed | **Verified on Linux** | 15 tests, real Swift 6.1, real static library |
| `PRVKit` framework adapters | Not started | — | CoreAudio, AVFoundation, keychain — the genuinely unverifiable part |
| `PRVUI` | Not started | — | SwiftUI |

## Sprint 31 — The planner reaches the host

The headline feature, across the boundary. A host describes its library, asks
for a set, reads the tracklist back, and applies it to the project — where it
becomes ordinary operations on the log, indistinguishable from an edit somebody
made by hand.

| Item | Status | Qualifier | Notes |
|------|--------|-----------|-------|
| `prv-ffi::planning` | Completed | Verified | Candidates built by call, not by struct: a `repr(C)` layout is a permanent promise |
| A plan is held, not returned | Completed | Verified | Nothing allocated on the host's behalf, nothing to free, no size-then-read race |
| `prv_planner_apply` | Completed | Verified | MP#3B by construction: nothing marks a placement as generated |
| Applying twice does not reuse an identity | Completed | Verified | ADR-0003; reuse would make a merge drop the second set as "already present" |
| ABI minor version 1.1 | Completed | Verified | Calls added, nothing existing moved — what the minor field is for |
| `PRVCore.Planner` | Completed | **Verified on Linux** | 9 further Swift tests, including the whole loop end to end |

The boundary now carries the product's actual promise: *"a two-hour set that
builds"* goes in, a tracklist comes out, and the audio plays.

## Sprint 32 — Analysis reaches the host, and the application closes

The last thing that was reachable in Rust only. Before this a host had to tell
the planner a track's tempo, key, energy and loudness — facts the core computes
and the host has no way to work out. The application would have had to ask a user
to type in a BPM, which is not a product.

| Item | Status | Qualifier | Notes |
|------|--------|-----------|-------|
| `prv-ffi::analysis` | Completed | Verified | Tempo, key, loudness, true peak, energy, transition points |
| Absent is not zero | Completed | Verified | A track with no pulse has no tempo — not a tempo of zero, and not a guess |
| `PRVCore.Analysis` | Completed | **Verified on Linux** | Optionals all the way out, so "we could not tell" survives to the interface |
| Analyse → plan → place → play | Completed | **Verified on Linux** | Nothing typed in by hand |
| ABI minor version 1.2 | Completed | Verified | Calls added, nothing existing moved |

### What building the end-to-end test found

The stages have different appetites, and nothing had said so. Tempo comes from a
novelty curve and is available after a few seconds. **Structure needs roughly
thirty seconds** before it finds sections, and without sections there is no
energy figure — so a twelve-second loop analyses successfully, reports a tempo,
and still cannot be planned with.

That is the correct behaviour and a confusing one. It is now written down in
`prv-ffi::analysis`, and the test that found it says why its fixture is the
length it is.

### The application, stated plainly

There is now one test, in Swift, that does what the product does: decode audio,
analyse it, hand the findings to the planner, ask for a set, apply the set to the
project, press play, and hear it. Everything in that sentence is verified on
every commit.

## Sprints 33–34 — The Apple layer, and the two questions asked before anything

| Item | Status | Qualifier | Notes |
|------|--------|-----------|-------|
| `PRVKit` ports, session, decoders, project store | Completed | **Verified on Linux** | The workflow: import, analyse, plan, adopt, play |
| `PRVUI` models for six spaces | Completed | **Verified on Linux** | Every threshold and format; the views decide nothing |
| `RenderHandle` | Completed | Verified | The audio thread cannot reach `placeTrack`, because it is not on the type it holds |
| `AVFoundationDecoder`, `CoreAudioOutput`, `KeychainStore`, `Views.swift` | Completed | *Authored* | The four things a macOS runner must check |
| `prv-ffi::policy` — consent and entitlement | Completed | Verified | Nothing agreed to by default; essential features at every tier |
| `PRVCore.Policy` | Completed | **Verified on Linux** | Both privacy questions exposed, not one |
| ABI minor version 1.3 | Completed | Verified | Calls added, nothing existing moved |

### Sprint 35 — the collection

| Item | Status | Qualifier | Notes |
|------|--------|-----------|-------|
| `prv-ffi::collection` | Completed | Verified | Search returns a count; the host reads identities and fields by index |
| Strings copy into caller buffers | Completed | Verified | Always terminated, even when empty; the size is reported whether or not it fitted |
| `prv_collection_restore` | Completed | Verified | Added because a test showed the boundary had a delete with no undo |
| `PRVCore.Collection` | Completed | **Verified on Linux** | Two-call read path; non-ASCII round-trips byte for byte |
| ABI minor version 1.4 | Completed | Verified | Calls added, nothing existing moved |

A test written to assert that removing a track twice fails found that it does
not — removal is a soft delete and `restore` brings the track back with its
rating, tags and play count. The core was right; the *boundary* was wrong,
because it exposed the removal and not the restore. Every host would have had a
destructive action with no way back, which is the opposite of Master Prompt #9
whatever it was called.

### Sprint 36 — delivery

| Item | Status | Qualifier | Notes |
|------|--------|-----------|-------|
| `prv-ffi::delivery` | Completed | Verified | What must happen to a master before it goes where it is going |
| The verdict is read in one call | Completed | Verified | A gain without its resulting peak is half a decision |
| `PRVCore.Analysis.judge(for:)` | Completed | **Verified on Linux** | Every target, format and depth |
| ABI minor version 1.5 | Completed | Verified | Calls added, nothing existing moved |

The core still writes no files. It answers the question an encoder needs
answered first, and the host applies the gain and encodes — which is what lets
one rule serve a WAV on a laptop, a stream upload and a broadcast delivery.

A master far below its target reports a gain of *zero*. That is not an
oversight: it is almost always a mistake upstream — a muted lane, the wrong
project — and twenty decibels of gain produces a loud version of the wrong
thing.

### Sprint 37 — how the application behaves

| Item | Status | Qualifier | Notes |
|------|--------|-----------|-------|
| `prv-ffi::experience` | Completed | Verified | Settings and notifications together, because they are one decision |
| A performance is not interrupted by what can wait | Completed | Verified | Held, not dropped; released with a count when the user is back at the desk |
| `PRVCore.Experience` | Completed | **Verified on Linux** | And `noticeCount`, so a host never hard-codes the range |
| ABI minor version 1.6 | Completed | Verified | Calls added, nothing existing moved |

**The boundary now reaches every context the core decides in.** Musical time,
transport, the project, the timeline, rendering, planning, analysis, the
collection, consent, entitlement, delivery, settings and notifications: all
callable from C, all wrapped in Swift, all tested on Linux on every commit.

What is left out of the boundary is `prv-plugin`, `prv-learning` and
`prv-telemetry` — three contexts whose hosts do not exist yet. A boundary for a
plugin loader with no plugin host to load into would be a guess about a shape
nobody has built against, and ADR-0005 is explicit that the isolation tier
decides the interface. They are named here rather than quietly omitted.

### Sprint 38 — the application

| Item | Status | Qualifier | Notes |
|------|--------|-----------|-------|
| `PRVStudio` executable | Completed | **Verified on Linux** | Starts, brings the stack up, reports what it found, exits zero |
| `Studio` — the wiring object | Completed | **Verified on Linux** | Owns session, policy and experience; decides nothing itself |
| `Info.plist` and entitlements | Completed | Verified | Structure validated; the network key's absence is a build rule |
| Architecture rule 10 — the sandbox has no network | Completed | **Enforced** | Verified against a deliberate grant |
| The macOS CI job | Completed | Enabled | Was `if: false`, which reports nothing and looks like passing |

**A library that compiles and a binary that starts are different claims.** Every
other test here exercises a module; `swift run PRVStudio` exercises the product,
and it now runs in continuous integration.

The entitlements file has no `com.apple.security.network.client`, and its absence
is deliberate rather than an omission. Master Prompt #26 promises nothing leaves
the device without agreement; our code honours that, and code can have bugs. An
entitlement cannot. Without that key the operating system makes an outbound
connection *impossible*, so the promise is enforced below us. Rule 10 keeps it
absent, and cloud sync will add it in the same commit as the consent screen it
depends on — deliberately, not quietly.

### Sprint 39 — the wire format, and a defect it exposed

| Item | Status | Qualifier | Notes |
|------|--------|-----------|-------|
| `prv-project::wire` codec | Completed | Verified | Thirteen payload kinds, each pinned to a number that can never change |
| A version vector as a message of its own | Completed | Verified | The half of the protocol that goes first; makes sync incremental |
| Forward compatibility at three levels | Completed | Verified | Header, entry and payload each framed, so a newer build can add to any of them |
| An unreadable operation is carried, not dropped | Completed | Verified | Byte-exact relay; an older install is not a hole in a fleet |
| Bounded allocation on untrusted bytes | Completed | Verified | Every count checked against the bytes present before anything is reserved |
| `prv-ffi` synchronisation calls | Completed | Verified | Four calls; the core produces bytes and the host owns the socket |
| `prv_engine_set_device` | Completed | Verified | Device identity is a fact about the machine, so the host declares it |
| `PRVCore.Engine` sync surface | Completed | **Verified on Linux** | Eight tests, including both directions converging |
| Placement identities namespaced by device | Completed | Verified | A defect the bidirectional test found; see below |
| ABI minor version 1.7 | Completed | Verified | Calls added, nothing existing moved |

**Why the codec lives in `prv-project` and not `prv-sync`.** The encoder must
handle every variant of `OperationPayload`, and only the crate that defines that
enum can have the compiler say so. One crate away the enum is `non_exhaustive`,
the match needs a wildcard, and a new kind of edit would ship as an operation
that silently refuses to travel. ADR-0003's rule — a variant is never redefined
and never removed — is a rule about that file, so that is where it is kept.

**Understand all of it, or none of it.** A reader that meets something it does
not know keeps the entry's bytes exactly as they arrived, reports it, and passes
it on unchanged. It does not apply half of it: an entry carrying a payload this
build knows *plus* a field it does not is treated as not understood, because
storing an edit stripped of the meaning its author gave it, and then relaying the
stripped version as though it were theirs, is worse than carrying it whole.

The practical rule for anyone extending the format: prefer a new variant over a
new field on an existing one. Both are safe; the second costs old builds the
ability to apply that variant at all.

**What the codec does not claim.** There is no checksum. Master Prompt #26
requires the transport to be encrypted and authenticated, which detects tampering
far better than a checksum, and a checksum beside real authentication mostly
gives a reader's confidence somewhere to come from when it should not. A test
sweeps every byte of a message through five values and asserts what is actually
true — no panic, no allocation the bytes did not pay for, and anything the reader
accepts it can write back out and read again identically. It does *not* assert
that damage is detected, and the comment says why.

### The defect the bidirectional test found

Writing "both sides end up with the same project" failed, and the reason was not
in the new code.

Every engine allocated placement identities from a counter starting at one. Two
machines editing the same project offline therefore both called their first clip
`placement:1`. Merging produced two `PlaceTrack` operations claiming the same
placement — reported as conflicts rather than lost, so nothing was silently
destroyed, but a user would have met one conflict per clip either of them added,
which is the same as having no conflict detection at all.

The lesson was already in the codebase and had not been applied twice.
`OperationId` is a device plus a number that device allocates itself, precisely
so two offline machines cannot collide. Placement identities now inherit the same
idea: the high half of the number names the device, the low half counts, and a
device that exhausts its half is refused rather than spilling into its
neighbour's.

The residual limit is stated rather than hidden: the device half is a 32-bit
fingerprint, so two devices collide only if their fingerprints do. Sixty-four
bits of exactness would need both halves whole, and a placement identity is a
`uint64_t` at a boundary whose major version forbids changing a call that already
exists. The risk is bounded by the devices sharing *one* project — a handful, not
a population — and it fails the way the old scheme failed rather than a new way.

### What synchronisation still does not do

**A transport.** There is none, and that is ADR-0001 working rather than a gap:
the core produces bytes and reads bytes. The same four calls serve a cloud
service, a local network, a memory stick and a file attached to an email. The
entitlement that would let the application open a socket is still absent, and
architecture rule 10 keeps it absent until the consent screen it depends on ships
in the same commit.

### Sprint 40 — carrying what this build cannot read

| Item | Status | Qualifier | Notes |
|------|--------|-----------|-------|
| `CarriedOperation` in the document model | Completed | Verified | A project is its log, and this is part of the log |
| `OperationLog::carry` | Completed | Verified | Bounded separately, refuses rather than discarding |
| `OperationLog::carried_since` | Completed | Verified | The counterpart of `operations_since`; what makes a relay work |
| `OperationLog::promote_carried` | Completed | Verified | An upgrade turns carried work into the project it always was |
| `wire::encode_all` | Completed | Verified | Both halves interleaved into one total order |
| `prv_engine_carried_count`, `prv_engine_promote_carried` | Completed | Verified | Driven from C against the committed header |
| `PRVCore` carrying surface | Completed | **Verified on Linux** | Two more tests, including the three-install relay |
| ABI minor version 1.8 | Completed | Verified | Calls added, nothing existing moved |

**The limitation Sprint 39 recorded is closed.** Operations from a newer build
are now written into the log, so they survive being closed and reopened, and they
are re-emitted in every message the device sends afterwards. A device running an
older build is a relay rather than a hole in a fleet, and stays one across a
restart — which is what "relay" has to mean to be worth anything.

**Why the log and not the sync layer.** ADR-0007 draws the boundary by one
question: does a project file contain it? An operation somebody made is part of
the project whether or not this build can fold it into a timeline. Holding these
in the synchronisation layer would mean carrying a colleague's work until the
application quit and then quietly stopping — the same as losing it, later and
less visibly.

**Carrying commits the log to something, and the commitment is honest.** The
identity is recorded as seen, which stops peers resending it. That is only
truthful because the bytes are kept: a device that recorded an operation as seen
without holding it would end the relay at itself while every peer believed the
work had arrived. The two halves of that promise are tested together.

**What carrying cannot do**, stated because it is easy to assume otherwise: a
carried operation takes no part in conflict detection. This build does not know
what it touches, so it cannot tell whether it disagrees with a local edit. That
decision moves to whichever build understands both sides — the honest place for
it, since it is the only place it can be made correctly.

**Promotion is the part worth the most.** After an upgrade,
`promote_carried` re-reads the stored bytes under the new build's rules —
including "understand all of it or none of it" — and the ones it now understands
become ordinary operations. The user sees work appear that was theirs all along,
reconstructed from nothing, because the bytes the author wrote were never
rewritten by anyone in between.

### Sprint 41 — what goes with this?

| Item | Status | Qualifier | Notes |
|------|--------|-----------|-------|
| `prv-mix::affinity` | Completed | Verified | The pair, judged with no set in mind |
| Direction is part of the question | Completed | Verified | `what_follows` and `what_precedes` are different lists |
| Clashing keys are absent, not ranked last | Completed | Verified | A musical fact rather than a preference |
| A stable ranking | Completed | Verified | Ties broken by identity, so the same question gives the same answer |
| `prv_planner_neighbours`, `prv_planner_neighbour` | Completed | Verified | Driven from C against the committed header |
| `PrvComponent` in the generated header | Completed | Verified | Numbered from one, so zero is never a component |
| `PRVCore.Neighbour`, `ScoreComponent` | Completed | **Verified on Linux** | Three more tests |
| ABI minor version 1.9 | Completed | Verified | Calls added, nothing existing moved |

**The question the planner could not answer.** `transition::score` needs a
`Goal`, because half of what makes a move good is where the evening is going —
the same two records are a great move at the peak and a poor one at the start.
Master Prompt #20 asks for something else: what goes with this record, asked at
import, before there is a set to ask it of.

That question is about the *pair*, and it has an answer without a journey.
Every component is judged by the same functions the planner uses, except energy:
with no set, the only thing energy continuity can mean is continuity, so that is
what it means. The planner scores energy against what the set wants next, which
is often deliberately not continuity — a set that means to lift wants the next
record higher, and the planner is right to prefer one.

So affinity is the pair and the plan is the journey. A record can be an excellent
neighbour and the wrong record for this moment, and both are true at once. Saying
that plainly is what stops the two disagreeing in a way that teaches people to
ignore one of them.

**Tempo is judged at the widest tolerance any setting allows**, which makes that
component an upper bound: no plan at any creativity setting scores a pair's tempo
higher, because a tighter allowance can only cost more. There is a test. A
shortlist that promised more than a set could deliver would be the worse mistake.

**Clashing keys are not neighbours at all.** `prv-harmony` separates clashing
from merely risky precisely so this distinction can be made without a setting,
and a list that ranked unlistenable moves at the bottom would be a list nobody
could trust the top of. An *unknown* key is not a clash — that would bury every
record whose analysis has not finished, which is most of a library on the day
somebody imports it.

**Neighbours live on the planner handle**, not on the collection, because they
need exactly the facts a plan needs and the planner's library already holds them.
A second path would mean a second place for a host to describe its records, and
the two would drift.

### The fixture that was lying, and the test that was comparing the wrong things

Two failures in this sprint were both in tests rather than in code, and both were
worth the time.

The first asserted that a badly-matched tempo made tempo the weakest component.
It did not: the fixtures had no mix points, so *structure* was weakest — truly,
since nothing was known about where to mix. The fixture was fixed rather than the
assertion, because a library of records with no analysed mix points is not the
library the module is for.

The second compared affinity's tempo component against one computed from ideal
beats per minute. A tempo is stored as microseconds per beat, so 132 comes back
as very nearly 132, and the test was measuring that rounding rather than the
property it meant to check. Now both sides read the tempo from the same place.

### Sprint 42 — the cloud, as a person sees it

| Item | Status | Qualifier | Notes |
|------|--------|-----------|-------|
| `prv-ffi::sync` | Completed | Verified | The state machine and the outbox, at the boundary |
| `PrvSyncState`, `PrvSyncEvent` | Completed | Verified | Generated from the core's own vocabulary |
| `PRVCore.Sync`, `SyncSnapshot` | Completed | **Verified on Linux** | One consistent reading rather than three reads that can straddle a change |
| `PRVUI::ConsentModel` | Completed | **Verified on Linux** | Two questions, because a purpose can leave the device carrying nothing the user made |
| `PRVUI::SyncModel` | Completed | **Verified on Linux** | Offline is a state, not an error |
| ABI minor version 1.10 | Completed | Verified | Calls added, nothing existing moved |

**Why the state machine crosses at all**, when the bytes already do: "connected"
is the smallest part of what synchronisation state means. The machine holds rules
with teeth — editing is permitted in every state, permanently; a conflict stops
the transfer and nothing else; a pause is lifted only by the user and never by a
network event. A host that modelled synchronisation itself would reimplement
those, differently, eventually. So the host reports events and the core decides
what they mean.

**The consent screen carries two questions rather than one.** A purpose can leave
the device without carrying anything the user made — crash reporting is the plain
example — and a screen that collapses those misleads whichever way it collapses
them. Built on "does anything leave the device" alone, it tells a user their
recordings are being sent when they are not. Built on "does this send content"
alone, it hides an upload. This is the distinction a test found in Sprint 38, now
built into the surface that has to say it out loud.

**Offline is a state, not an error.** It is normal, editing works, and the only
thing worth saying is that some work has not travelled yet — a count, not a
warning. The one thing that *is* a warning arrives long before anything breaks: a
backlog near its bound means a very long session offline or a server refusing
everything, and a user told at the refusal is told too late.

**One badge at a time, most pressing first.** A status area that stacks four
notices is a status area people stop reading, so the model picks one and the
quieter facts stay readable underneath it.

**A wildcard the compiler forced, and the test that replaces it.** `SyncState`
and `SyncEvent` are `non_exhaustive`, so a mapping in another crate cannot be
exhaustive the way the wire format's payload mapping is. The arms map an unknown
variant to zero rather than to a plausible-looking state, and a test walks
`SyncState::ALL` and `SyncEvent::ALL` asserting every one has a non-zero code.
An unmapped variant is a failing test rather than a silent lie on somebody's
screen. `SyncEvent::ALL` was added for exactly that purpose.

### The privacy distinction the tests found

A test was written asserting that a purpose which sends no content also does not
leave the device. The core disagreed, and the core was right: `CrashDiagnostics`
transmits and carries none of the user's music.

Those are two different questions and a consent screen needs both. A screen built
on "does anything leave the device" alone claims the user's recordings are being
sent when they are not; one built on "does this send content" alone hides an
upload entirely. The boundary now exposes both, and the test asserts the true
relationship — sending content implies leaving the device, and not the reverse.

### The limitation that shrank

R-01 used to read "no Apple code has been compiled". It now reads "no
Apple-*framework* code has been compiled", and the difference is the point.

Swift 6.1 runs on Linux. `PRVCore` — the layer holding pointer lifetimes, the
audio callback and every error code, where a mistake is unrecoverable and silent
— imports nothing but Foundation, so it builds and its tests run on every
commit against the real library. Keeping it free of framework imports is what
buys that, and it is why `PRVKit` is a separate target: so the untestable half
cannot swallow the testable half.

What remains genuinely unverified here is CoreAudio, AVFoundation, the keychain
and SwiftUI. That is a real gap and a much smaller one than "the Apple layer".

## Where the core stands

Every one of the eighteen bounded contexts in the [module
index](03-module-index.md) now has a crate in `core/`, built and tested here.
Twenty-four crates, acyclic, with no third-party runtime dependency.

What that does **not** mean, and the distinction is the point of this document:
the core *decides*; it does not *act*. There is no code here that opens a file, a
socket, a keychain, an audio device or a plugin, because ADR-0001 puts all of
that outside — and the layer that does those things is Swift against Apple
frameworks, which this environment cannot compile.

So the honest summary is:

- **Every decision the product makes is written, tested and reviewable today.**
  What tempo a track is, which record follows which, whether a mix meets its
  target, who may do what, what leaves the device, what a licence withholds, when
  a plugin is passed over, what a notification interrupts.
- **Nothing yet performs any of it.** Each of those decisions is waiting on an
  adapter in the platform layer.

The remaining core-side gaps are recorded per sprint under *Known limitations*
and summarised in [17-known-limitations.md](17-known-limitations.md). The largest
are time-stretching with key lock (Phase 2 audio work) and a realtime loudness
meter.

## Environment limitation

The available continuous-integration environment is Linux. It compiles and tests
Rust and pure Swift. It cannot compile SwiftUI, AVAudioEngine or CoreAudio, which
require the Apple SDKs.

Consequences, stated plainly:

- Everything in `core/` is verified here.
- Pure-Swift packages are verified here.
- Apple-framework code will be marked *Authored* until a macOS runner compiles
  and tests it. The CI workflow defines those jobs; they are inert until such a
  runner is available.

This is recorded again in [17-known-limitations.md](17-known-limitations.md) so
that it is impossible to miss.
