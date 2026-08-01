# Sprint 10 — A plan becomes edits

Sprint 7 produced a tracklist. Sprint 8 built the surface a mix is edited on.
Sprint 9 put automation into the document. This sprint joins them: a plan is now
turned into the actual edits that make the mix.

| Delivered | Tests |
|-----------|-------|
| Transition techniques, chosen from evidence | 6 |
| Overlap length and bar quantisation | 3 |
| Operation emission and the undo round trip | 6 |
| Summary and reporting helpers | 3 |

563 tests in total, up from 545. All gates green: `cargo test`,
`cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`, `cargo doc`,
all six architecture rules, and the design-token staleness gate.

## Business outcome

**The product's central promise now works end to end in the core.** Describe a
set, and the system produces analysed facts, a planned ordering, and the edits
that realise it — clips overlapping on two decks, with the level, equaliser and
filter moves each transition needs.

**An AI-built mix is an ordinary edit.** It can be undone, compared against what
was there before, branched, synchronised, and pulled apart clip by clip. None of
that is implemented here; all of it arrives because the renderer emits
operations rather than a finished timeline.

**The system can now say how it will mix, not only what.** "A blend here, a
short cut there, because these two keys are only loosely compatible" — with the
reason naming the same number that appears beside the track in the list.

## Architecture review

**No new decision records were needed**, and one prior decision paid for the
sprint.

**The renderer emits operations, not a timeline.** That is the decision the
module is arranged around. Handing back a finished timeline would make an
AI-generated mix a *different kind of object* from a hand-made one: every
feature the log provides — undo, versions, branching, merge reporting,
incremental sync — would need a second implementation for the generated case,
and the first thing a user would discover is that they cannot undo it.

Master Prompt #3B requires the user to be able to edit everything the system
decides. Emitting operations makes that true by construction rather than by a
promise to add editing later, and the test that proves it applies the render to
a state, then applies every inverse in reverse, and asserts the project is empty
again.

**The technique is derived, not configured.** How two tracks are joined comes
from the evidence the planner already produced, so the reason for a short cut is
the same number that explains the choice of track. A configuration setting would
have been easier and would have severed the explanation from the decision.

**Identity allocation stays with the caller.** `PlacementIds` is handed in
rather than invented, because the log's identity guarantees are what everything
else rests on and two devices rendering concurrently must not both claim
placement 7.

The layering held. `prv-mix` → `prv-timeline` → `prv-project` → `prv-time`, still
acyclic, still no runtime dependencies.

## Audio quality review

Each technique encodes something a working DJ does, and the reasoning is written
where the automation is:

**Blend** hands the bass over in one step rather than crossfading it, so there
is never a moment with two basslines at full level — the muddiest sound a mix can
make. Levels move on a raised cosine; the low end moves on a hold.

**BassSwap** hands the low end over at the *very start* rather than part way,
because the reason for choosing it is that the two basslines do not agree. Doing
it late would leave the clash audible for exactly as long as the technique
exists to avoid.

**FilterFade** sweeps the outgoing record upward while its level stays put, so
the change reads as it *leaving* rather than as someone turning it down. Chosen
when the records suit each other but the arrangement offers nowhere quiet.

**Cut** is short and square, and deliberately not a fade with a small number:
the point is that neither record is heard at half level, because that is the part
that sounds wrong when two records do not fit.

**Every overlap is a whole number of bars.** A transition that begins or ends
mid-bar sounds late whatever the arithmetic says, so the length is quantised —
tested across the whole range of every technique rather than at a few points.

## AI behaviour review

**Better evidence never produces a more cautious technique.** The thresholds are
provisional and will be recalibrated in Phase 3; the *ordering* they induce is
not, and it is tested by walking the whole range and asserting monotonicity. A
recalibration that broke that would be caught.

**The reason is carried, not reconstructed.** `TechniqueChoice` holds both the
technique and the component that decided it, so an explanation renders what
happened instead of re-deriving something plausible.

**The rules are stated where a reader will look for them.** Each variant's
documentation says what must be true for it to be chosen, and the test walks
every one of those statements.

## Performance review

Rendering is linear in the length of the set and does no searching — the search
already happened in `plan`. It runs once, off any hot path, and produces a few
dozen operations per transition.

One number is worth recording: the whole 563-test suite, including every
planning and rendering test and every signal-processing test on minutes of
synthetic audio, runs in about 25 seconds in debug.

## Security review

Nothing new. No input or output, no secrets, no network. Identity allocation is
the caller's, which keeps the one thing that could collide across devices under
the control of the layer that knows about devices.

## Accessibility review

No user-facing surface. One enabling decision: `Technique::key` returns a stable
identifier rather than English prose, and `TechniqueChoice::reason` returns a
`Component` rather than a sentence — so the interface builds the explanation in
the user's language and at the user's level, which Master Prompt #25 requires.

## Findings raised on my own work

**One API shape was wrong and clippy found it before a reviewer would have.**
`choose_technique` originally took a `&TransitionScore`, which forced the tests
to construct a full score and then mutate its components through a test-only
setter. That setter was the smell: a public mutator existing solely so a test
could reach a private field. Taking `ScoreComponents` directly is both the
narrower dependency and the honest one — the function reads components and
nothing else.

**A hardcoded sample rate slipped in.** The overlap conversion used 44 100
directly. It is now a parameter, which is correct rather than merely tidy: a
96 kHz project would otherwise have had transitions less than half as long as
intended, and nothing would have reported it.

**One function grew past the point of being readable** — the automation writer
handled four techniques in one body. Split into four named functions plus a tiny
writer, so each reads as the description of what a DJ does rather than as a list
of arguments. The lint that caught it was making a design point, not a style one.

## Are the project's principles still respected?

| Principle | Verdict |
|-----------|---------|
| Never the easy solution when a premium one exists (MP#1) | Held. Operations rather than a timeline; derived techniques rather than a setting. |
| Production-ready, never placeholder (MP#13) | Held and enforced. |
| Modular, dependencies inward (MP#4, MP#7) | Held. Twelve crates, acyclic, no runtime dependencies. |
| The user can edit everything the system decides (MP#3B) | Held by construction, and tested by an undo round trip. |
| Explanations render the actual arithmetic (ADR-0006) | Held. The technique carries the component that chose it. |
| Nothing is lost (MP#9) | Held. A generated mix is inside the mechanism that guarantees it. |
| Quality is not a phase (MP#27) | Held. 563 tests; three findings raised and fixed within the sprint. |

## Known limitations

1. **Tempo is not adjusted.** The planner constrains two adjacent tracks to
   within a few per cent, and the renderer lays them out assuming they will be
   matched — but nothing yet records *which* tempo the set runs at during a
   transition, or emits the rate change. That needs a tempo operation in the log
   and is the most audible remaining gap.
2. **The overlap does not use the analysed structure.** A transition is placed
   at the end of the outgoing track rather than at its outro, and the incoming
   track enters at its start rather than at its intro. `Candidate` already
   carries the mix points; using them is a natural next step and would make
   transitions land where the music invites them.
3. **The timeline does not yet emit operations.** It can be built from the
   document and the renderer can produce operations, but a user's own edit
   through `Timeline` still does not become a log entry. Unchanged from Sprint 9.
4. **Ripple editing, effect-chain modelling and positional lane numbers** are
   unchanged from Sprint 8.
5. **The technique thresholds are provisional**, pending Phase 3 calibration.

## Definition of done

| Criterion | Status |
|-----------|--------|
| Architecture reviewed | Yes — no new records needed |
| Implemented, no placeholders | Yes, enforced |
| Tests passing | Yes — 563 |
| Performance validated | Linear, off any hot path; suite time recorded |
| Documentation updated | Yes, in the same commit |
| Accessibility verified | Not applicable yet; one enabling decision recorded |
| Security reviewed | Yes — no new surface |
| No critical technical debt introduced | Five recorded limitations; tempo adjustment is the most material |
