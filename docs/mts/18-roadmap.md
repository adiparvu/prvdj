# 18. Future Roadmap

The phase plan is Master Prompt #30. This section records where the work
actually stands and what each phase must prove before the next begins.

Three horizons are maintained, as Master Prompt #15 requires: current release,
next release, long-term vision. Future capability is prepared with extension
points, not with features built early.

| Phase | Goal | Exit criterion | Status |
|-------|------|----------------|--------|
| 0 | Foundation | Project builds; pipeline operational; architecture reviewed | **Complete** |
| 1 | Core Experience | A user can import music and create a simple mix | **In progress** |
| 2 | Professional Workflow | Professional editing workflow is stable | Not started |
| 3 | AI Foundation | Recommendations are transparent, explainable and optional | Not started |
| 4 | Live Performance | Reliable live workflow | Not started |
| 5 | Cloud Platform | Projects synchronise without disrupting local work | Not started |
| 6 | Plugin Platform | Third-party extensions integrate safely | Not started |
| 7 | Optimisation | Performance budgets consistently met | Not started |
| 8 | Commercial Release | Version 1.0 ready | Not started |

## Phase 1 — the immediate next slice

Ordered so that each item is verifiable when it lands, and so that nothing
depends on a module that does not yet exist.

1. ~~**Transport state machine.**~~ Done — nine states, 405 transitions defined,
   loops with phase-correct wrapping, slip mode.
2. ~~**DSP graph and the first processors.**~~ Done — processor contract, chain,
   gain, Linkwitz-Riley three-band equaliser with true kill, filter sweep. All
   under the allocation gate.
3. ~~**Beat grid and multi-segment tempo map.**~~ Done.
4. **Waveform tile generation.** Tile model, resolutions, cache invalidation by
   generation version (Module Specification #003).
5. **Library index and import pipeline.** Restartable stages with durable state,
   as Master Prompt #7 requires.
6. **Project operation log.** The mechanism from ADR-0003 on which undo,
   branching and synchronisation all rest.
7. **Apple audio host.** CoreAudio render host and decoder adapters. Requires a
   macOS runner to be verifiable; until then it is authored, not verified.
8. **First interface surfaces.** Token adapter, foundation components, Library
   space.

Phase 1 is complete when a person can import a folder of music and hear two
tracks mixed — with nothing in the path having been reported as working before
it was measured.
