# Sprints 2 to 4 — Waveform, project log, library

Three sprints reviewed together, because they share one shape: each takes a
requirement that appears in several specifications and finds the single mechanism
that satisfies all of them.

| Sprint | Delivered | Tests added |
|--------|-----------|-------------|
| 2 | Waveform tiles, resolution ladder, viewport rendering | 38 |
| 3 | Project operation log, undo, versions, branching, merge | 49 |
| 4 | Library index, search, filters, duplicates, collections | 46 |

360 tests in total, up from 227. All gates green.

## Business outcome

Nothing user-facing yet, and three things that will be very hard to add later if
they are not here now.

**The waveform can be drawn at any zoom without lying.** Precomputed tiles at five
resolutions, never upscaled, aggregated so that a transient inside a column
survives rather than being averaged away.

**Nothing a user does can be lost.** Undo, redo, named versions, branching,
comparison, crash recovery and incremental synchronisation are consequences of
one append-only log rather than seven separate features.

**A hundred thousand tracks stay searchable.** Search costs the size of the
answer rather than the size of the library, so the third keystroke is faster than
the second.

## Architecture review

**No new decision records were needed**, which is the useful signal. Every choice
in these three sprints followed from ADR-0001 and ADR-0003 rather than requiring
a new one. The layering held: `tools/check-architecture.sh` passes unchanged, the
core still performs no input or output, and unsafe code is still confined to one
crate.

**One design question was genuinely new** and is recorded where it was answered.
Module Specification #001 requires the library to filter by tempo, key and
energy; Master Prompt #20 makes the analysis engine the owner of all three. A
filter that consulted the analysis engine per track would turn a 100 000-track
query into a 100 000-call fan-out.

The answer is a projection: the library keeps the few values browsing needs,
refreshed when analysis completes, with a single writer and an obvious rebuild
path. It sits *beside* the track record rather than inside it, so that its status
as a cache is visible in the type rather than only in a comment.

## Audio quality review

The waveform is display, not signal path, so the audio contract is unchanged from
Sprint 1 and its gates still pass. Two properties are worth naming because they
are the ones a rectified or averaged implementation would lose:

- **Extremes rather than magnitude.** Asymmetry carries real information — heavy
  limiting, direct-current offset, the lopsided shape of a kick — and a rectified
  display hides all of it.
- **Energy alongside peaks.** A lone transient in a quiet passage has a tall peak
  and almost no energy. A display driven by peaks alone shows a breakdown as
  though it were full, and a DJ reading it misjudges the section.

## Security review

Nothing new. No input or output, no secrets, no network, no new dependencies. The
core still has zero runtime dependencies across nine crates.

Two decisions have a privacy dimension and were made accordingly. The library
holds an *opaque handle* to media rather than a path, so a library file carries
no filesystem layout. The project holds a *reference* to a track rather than a
copy, which is what lets ADR-0003's rule — sharing a project shares the document,
not the audio — be structural rather than a policy someone has to remember.

## Performance review

The two claims that would be expensive to retrofit are measured:

- Search on a ten-thousand-track library returns a specific result immediately,
  and the cost is proportional to the answer. The full hundred-thousand-track
  case named by Module Specification #001 is a Phase 7 benchmark; the structure
  that makes it achievable is here and tested at a tenth of the scale.
- Waveform rendering allocates nothing per frame, because the caller supplies the
  buffer. A timeline scrolling at 120 frames a second would otherwise allocate
  hundreds of times a second on the interaction path.

The project log's load time under compaction remains an open risk (R-06) and is
still marked pending rather than assumed.

## Accessibility review

No user-facing surface yet. One decision was taken with accessibility in mind and
is worth recording: the waveform exposes *energy* per column, not only peaks. The
structural navigation model for the timeline described in section 11 of the
specification depends on being able to say "a quiet breakdown here, a loud
section there", and that needs energy rather than amplitude.

## AI behaviour review

`prv-harmony` gained a second consumer this sprint, and that is the review point:
the library's harmonic filter calls the same `compatibility` function the planner
will call. Two notions of "compatible" that disagreed at the edges would be worse
than one imperfect notion, because a user would filter to a set of tracks the
planner then refused to use.

The calibration item from Sprint 0 remains open (R-03, R-04).

## Findings raised on my own work

**Three test expectations were wrong rather than the code**, across these
sprints, and each is worth naming because the pattern is the same: the test
asserted an example where it should have asserted a property.

1. A waveform column count assumed chunks of a fixed size when the loop clips
   them at a boundary. Now the test accumulates what was actually rendered and
   asserts the invariant.
2. A library search asserted an exact count where prefix matching is
   deliberately broad — "album 42" also finds track 4200. Now it asserts
   completeness, which is the guarantee that actually matters for
   search-as-you-type.
3. A slip-mode test made the same fixed-size assumption as the first.

None of these were defects in shipped code, but all three were tests that would
have passed for the wrong reason under a slightly different implementation. They
are fixed, and the pattern is now called out in the testing section of the
specification.

**One genuine omission is recorded rather than papered over.** Processors still
have no parameter-addressing scheme; automation and plugin parameters both need
one. It is limitation 3 and Phase 2 work, deliberately not improvised around a
single processor's needs.

## Are the project's principles still respected?

| Principle | Verdict |
|-----------|---------|
| Never the easy solution when a premium one exists (MP#1) | Held. Version vectors rather than a Lamport counter, because a Lamport clock cannot tell "after" from "elsewhere" and that distinction is the whole of conflict detection. |
| Production-ready, never placeholder (MP#13) | Held and enforced. |
| Modular, dependencies inward (MP#4, MP#7) | Held. Four new crates, no new exceptions. |
| Nothing is lost (MP#9, MP#24) | Held and tested: soft delete with restore, undo that grows the log, conflicts reported rather than resolved. |
| The user owns their work (MP#29) | Held structurally: the project refers to media, the library holds an opaque handle. |
| Quality is not a phase (MP#27) | Held. 360 tests; three test defects found and fixed during the sprints. |

## Known risks

Unchanged. **R-01** — no macOS runner — remains the one that shapes what comes
next, and now more sharply: everything the core needs for a first audible result
exists, and what is missing is the platform layer that cannot be verified here.

## Future work

The remaining Phase 1 items are the audio host, the import pipeline that feeds
the library, and the first interface surfaces. All three are platform work.

## Definition of done

| Criterion | Status |
|-----------|--------|
| Architecture reviewed | Yes — no new records needed |
| Implemented, no placeholders | Yes, enforced |
| Tests passing | Yes — 360 |
| Performance validated | Search and rendering measured; log compaction still pending with a phase |
| Documentation updated | Yes, in the same commits |
| Accessibility verified | Not applicable yet; one enabling decision recorded |
| Security reviewed | Yes |
| No critical technical debt introduced | One recorded limitation, not critical |
