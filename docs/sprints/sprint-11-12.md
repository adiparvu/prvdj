# Sprints 11 and 12 — Closing the loop

Two sprints reviewed together because they close the same loop from opposite
ends. Sprint 11 made the renderer produce a mix that lands where the music
invites it and records the tempo it runs at. Sprint 12 made the user's own edits
produce operations, so a hand edit and a generated one are the same kind of
thing.

| Sprint | Delivered | Tests added |
|--------|-----------|-------------|
| 11 | Tempo in the document; transitions on the analysed exit | 5 |
| 12 | Every timeline edit emits its operations | 3 |

571 tests in total, up from 563. All gates green: `cargo test`,
`cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`, `cargo doc`,
all six architecture rules, and the design-token staleness gate.

## Business outcome

**The set has a grid.** A DJ set runs at one tempo at a time, and the document
now records it. Everything downstream that snaps — the timeline, automation
editing, the transport — had nothing to snap to before this.

**Transitions land on the music.** The naive placement overlapped the last few
bars of whatever was playing, which on a produced record is often a fade, a drum
outro, or nothing at all. The analysis has known where a record wants to be left
since Sprint 6; the renderer now uses it.

**A generated edit and a hand edit are the same object.** Dragging a clip
produces exactly the operations the renderer produces. Both go into the same
log, both undo the same way, and neither has a code path the other does not.

## Architecture review

**No new decision records were needed.** Both sprints are ADR-0003 and ADR-0007
being applied.

**Tempo is keyed by frames, not ticks.** `prv-time` works in ticks and it would
have been natural to store them — but a tick position is *computed from the
tempo map*, so a map keyed by ticks defines itself in terms of itself. Frames
are the one position that means the same thing regardless of tempo, which is why
every other position in the document is one.

**An edit returns operations rather than appending them.** Appending is the
caller's decision, because an edit made mid-drag is provisional and only the
gesture's end should enter the history. A timeline that appended on every
intermediate position would give a user four hundred undo steps for one drag.

**An edit is a *list* of operations.** Trimming the front of a clip moves it and
shortens it; a split trims one clip and places another. The alternative —
a payload variant per gesture — would grow the log a vocabulary shaped by the
interface rather than by the document, and every new interface affordance would
become a new thing a ten-year-old project file has to understand.

**The timeline is a cache of the fold, and the test says so.** `Edit` exists so
the two can be kept in step cheaply; the test that matters applies three edits to
both the timeline and a project state, rebuilds a timeline from that state, and
asserts every clip agrees on position, length, lane and media. If they ever
disagreed, a user would see one thing and their project would contain another.

The layering held. Twelve crates, acyclic, no runtime dependencies.

## Audio quality review

**The tempo change belongs at the end of a transition, not the start.** During
the overlap the two records are matched and it is the *outgoing* one still
setting the pulse; moving the grid at the start would put it a few per cent off
the record the listener is currently hearing, for the whole length of the blend.

**An exit point is clamped rather than trusted.** One from a stale analysis, or
from a track that has since been trimmed, must not push the incoming record into
silence. The fallback when a track has no identified exit is the naive placement,
and that is not a compromise: a track with no outro genuinely offers no better
answer than "near the end".

## Findings raised on my own work

**A first attempt at emitting operations was quietly lossy, and the comment I
wrote for it was the tell.** `trim_start` emitted only a `MovePlacement`, and I
wrote a comment explaining that the length change would be "recovered on reload
because a clip's offset is derived from where it sits". That was not true, and
the fact that it needed three sentences of explanation was the signal. An edit
now carries a list of operations, `trim_start` emits both, and the round-trip
test would have failed if it did not.

Worth naming as a pattern: a comment that argues for why something incomplete is
acceptable is usually arguing against a design that has not been found yet.

**`Clip` did not know which track it played.** It carried a placement
identifier, which is identity, and the media reference lived only in the
document. That made `split` unable to record its second half — there was
nothing to put in the `PlaceTrack` operation. Adding `TrackRef` to `Clip` was
the missing model rather than a convenience: a clip on a timeline *refers to
media*, and expressing that in the type is also where ADR-0003's rule that
sharing a project shares the document and not the audio becomes structural.

**Sprint 11 shipped without its review document.** The standing instruction is a
review after every sprint, and I updated the status and traceability documents
but not the narrative one. Recorded here rather than quietly backfilled: this
document covers both sprints, and the omission is the sort that compounds if it
is not noticed.

## Security review

Nothing new in either sprint. No input or output, no secrets, no network.

One property is worth naming: removing a clip now records the removal of its
automation as separate operations, so undoing a deletion brings the automation
back *with* the clip. The alternative — dropping the automation silently and
restoring a bare placement on undo — would have been a quiet loss of a user's
work, which Master Prompt #9 forbids.

## Performance review

Unchanged on every hot path. Edits are constant-time apart from the overlap
check, which is linear in the number of clips on a lane; `from_project` runs when
a project opens.

## Accessibility review

No user-facing surface, no new decisions.

## Are the project's principles still respected?

| Principle | Verdict |
|-----------|---------|
| Never the easy solution when a premium one exists (MP#1) | Held. A list of operations rather than a payload per gesture; frames rather than ticks. |
| Production-ready, never placeholder (MP#13) | Held — and one near-miss caught, where a comment was arguing for an incomplete design. |
| Nothing is lost (MP#9) | Held and extended: deleting a clip records its automation going with it. |
| The log is the source of truth (ADR-0003) | Held, and now tested end to end: the cache and the fold agree. |
| Modular, dependencies inward (MP#4, MP#7) | Held. Twelve crates, acyclic. |
| Quality is not a phase (MP#27) | Held. 571 tests; three findings raised and fixed within the sprints. |

## Known limitations

1. **The renderer does not adjust playback rate.** The document now records what
   tempo the set runs at; nothing yet stretches a track to reach it. That needs a
   time-stretching processor, which is Phase 2 audio work.
2. **The incoming track enters at its start, not at its intro.** Sprint 11 used
   the outgoing track's *exit* point; the incoming track's *entry* point is
   carried on `Candidate` and not yet used. Doing it needs a source offset on the
   placement, which the log does not yet record.
3. **A placement has no source offset in the document.** `Clip` carries one and
   the log does not, so a trimmed front survives in the timeline and not on
   reload. This is the most concrete of the remaining gaps.
4. **Ripple editing, effect-chain modelling and positional lane numbers** are
   unchanged from Sprint 8.
5. **Every calibration item remains open** — harmonic weights, objective weights,
   confidence thresholds, technique thresholds — all scheduled for Phase 3 by
   ADR-0006.

## Definition of done

| Criterion | Status |
|-----------|--------|
| Architecture reviewed | Yes — no new records needed |
| Implemented, no placeholders | Yes, enforced; one near-miss caught and fixed |
| Tests passing | Yes — 571 |
| Performance validated | No hot path changed |
| Documentation updated | Yes, including the review Sprint 11 did not get |
| Accessibility verified | Not applicable; no new decisions |
| Security reviewed | Yes |
| No critical technical debt introduced | Five recorded limitations; the source offset is the most concrete |
