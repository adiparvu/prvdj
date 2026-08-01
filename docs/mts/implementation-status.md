# Implementation Status

Statuses are those defined by MP#31. Two additional qualifiers are used because
the build environment makes the distinction material:

- **Verified** — compiled and tested in continuous integration.
- **Authored** — written against specification, not yet compiled on a machine
  with the required SDK.

Code that is authored is never reported as working. This is a direct requirement
of MP#13 (*no placeholder implementations*) and MP#27 (*no feature is complete
without verification*).

_Last updated: Sprint 12._

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

## Not started

Every module in the [module index](03-module-index.md) not listed above is *Not
Started*. They are sequenced by the phase plan in [18-roadmap.md](18-roadmap.md).

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
