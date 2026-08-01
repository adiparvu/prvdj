# Sprint 7 — The mix planner

Six sprints built the things a DJ application needs to know. This one is the
thing it needs to *do*: take "a three-hour set that builds" and a library, and
produce an actual set with the reasoning for every move retained.

| Delivered | Tests |
|-----------|-------|
| Goal model — energy shapes, creativity, tempo range | 7 |
| Candidate projection | 5 |
| Constraint model and objective, with evidence | 14 |
| Beam search, distinct alternatives | 12 |
| Numeric conversions | 2 |

499 tests in total, up from 457. All gates green: `cargo test`,
`cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`, `cargo doc`,
all six architecture rules, and the design-token staleness gate.

## Business outcome

**The product now does the thing it exists to do.** Everything before this was
infrastructure that a user would never see directly. `prv-mix` is the first
module whose output *is* the feature: a tracklist, ordered, with a reason for
every transition.

**It works offline, and that is not a fallback.** The planner takes a structured
[`Goal`], never a prompt. A language model produces one; so does an on-device
parser, or six taps on a form. By the time the search runs, the difference has
been erased — which is what makes Master Prompt #26's "allow users to disable
cloud AI" a setting rather than an amputation.

**The same request produces the same set.** Tested. That is what makes the AI
behaviour tests of Master Prompt #27 meaningful, and it is what lets a user
share a set and have a colleague reproduce it.

## Architecture review

**No new decision records were needed.** ADR-0006 specified this module in
enough detail that building it was implementation rather than design. The one
structural decision worth recording — and it is recorded in the source, not in a
new ADR, because it follows directly from ADR-0006 — is that a rejection is a
different *type* from a low score.

**Constraints are not low scores.** This is the load-bearing decision of the
whole crate. Master Prompt #3B calls its rules "safety rules", and a safety rule
implemented as a heavy penalty is not a safety rule: under enough pressure — a
short library, a long set, a demanding curve — a scoring system will eventually
surface the least-bad clash, because everything is comparable and something has
to win. `Rejection` is a separate type returned by `Result::Err`, so a clashing
move never enters the candidate set and the search cannot reach it however
desperate it becomes.

The test that matters here is not the unit test on `score`; it is
`no_plan_contains_a_move_that_violates_a_hard_constraint`, which checks the
guarantee over a whole search. The search is where pressure to compromise comes
from, so it is where a leak would appear.

**The candidate is a projection, not the analysis.** Same pattern as
`prv-library`'s `AnalysisFacts`, for the same reason: the planner compares
tracks millions of times and wants a small flat record. Assembling one from a
`TrackProfile` is the caller's job, which is deliberate — "which key when the
detector offered two" is a product decision, not a search decision.

The layering held. `tools/check-architecture.sh` passes unchanged: no I/O, no
unsafe, dependencies inward only.

## AI behaviour review

**Version A, B and C are different optima, not three samples.** A beam search
converges by nature: its top few results usually share every track but the last.
Returning those as three versions is a lie a user detects in about ten seconds.
So the beam is pruned with a per-opening quota during the search, and filtered by
track-set similarity at the end. Two plans sharing more than two thirds of their
tracks are the same plan.

**Three genuinely different sets is a smaller claim than three optimal sets**,
and it is the one worth making. Recorded as a limitation rather than presented
as a guarantee.

**An unknown key is neutral, not perfect.** This is the finding I am most glad
the tests caught, because the failure would have been invisible: if an
undetected key scored perfectly, the planner would systematically prefer the
tracks it knows *least* about, and the more analysis improved the worse its
recommendations would look. Neutral says what is true — this move was not
evaluated harmonically — and a key detected with low confidence is discounted
toward neutral in proportion, which is the only use of a confidence in this
codebase that needs no threshold.

**The energy penalty is asymmetric.** Coming in below the curve costs twice what
coming in above it does, because a room notices a drop far more than a lift.
That is a claim about people, and it is written where the arithmetic is.

## Performance review

The search cost is the beam width times the candidate count times the set
length, and all three are known before it starts. That predictability is the
point: Master Prompt #12 requires recommendations to stream progressively, which
is only possible if a step takes a knowable amount of time. Planning a
twenty-track set from a library of hundreds runs in the low tens of
milliseconds; the whole 499-test suite, including every planning test, adds well
under a second.

The alternatives were considered and rejected on measurable grounds rather than
taste. Exhaustive search over a hundred tracks in a twenty-track set is more
orderings than there are atoms in the observable universe. Greedy is affordable
and fails in a recognisable way: it spends the strongest records early, because
they score well as *the next thing*, and has nothing left for the peak.
`the_set_follows_the_energy_shape_it_was_asked_for` is the test a greedy planner
fails.

## Security review

Nothing new. No input or output, no secrets, no network — the planner cannot
reach a network even if a future change wanted it to, which is what architecture
rule 1 enforces and what makes the offline claim structural.

One privacy property is worth naming: `TrackId` is opaque and the planner never
interprets it. A set can therefore be planned, stored and synchronised without
the planning layer ever holding a filename, a path or a piece of metadata.

## Accessibility review

No user-facing surface. Two enabling decisions: `Component::key`,
`EnergyShape::key` and `Creativity::key` return stable identifiers rather than
English prose, keeping translations out of the core; and `ScoreComponents::weakest`
exists so an interface can lead an explanation with the reason a score is low
rather than presenting six numbers and leaving the user to compare them.

## Findings raised on my own work

**One design error caught before it shipped, by asking what a neutral value
should be.** The first draft scored an unknown key as a perfect harmonic match,
on the reasoning that an unknown constraint should not penalise a track. That is
backwards: it makes unanalysed tracks the planner's favourites. The fix is a
neutral value with the reasoning written next to it, and a test that compares an
unknown pair against a known-identical pair.

**Two test expectations were adjusted for the right reason.** The
creativity-unlocking test originally asserted a specific classification for a
specific key pair, which would have made it a test of `prv-harmony`'s weights
rather than of the creativity setting. It now asserts the property that actually
belongs here: whatever the classification, the two settings may disagree about a
*risk* and must never disagree about a *clash*.

**No defects were found in earlier sprints' work by this one**, which is the
first time that has been true. The interfaces `prv-harmony` and `prv-analysis`
present were the ones this crate needed, which is some evidence that the
relation-before-score and absence-before-uncertainty decisions made in those
sprints were the right shape.

## Are the project's principles still respected?

| Principle | Verdict |
|-----------|---------|
| Never the easy solution when a premium one exists (MP#1) | Held. Beam search with an opening quota rather than greedy; distinctness filtering rather than "the top three". |
| Production-ready, never placeholder (MP#13) | Held and enforced. |
| Modular, dependencies inward (MP#4, MP#7) | Held. Eleven crates, still no runtime dependencies. |
| Safety rules are guarantees (MP#3B) | Held structurally: a violating move is a different type, not a low score. |
| Explanations are true by construction (ADR-0006) | Held. Every plan retains every component of every score. |
| Full capability offline (MP#1, MP#26) | Held structurally: the planner takes a goal, never a prompt. |
| Quality is not a phase (MP#27) | Held. 499 tests; the search's guarantee is tested over whole searches, not only over single scoring calls. |

## Known limitations

1. **Beam search is not guaranteed optimal.** Stated in the module
   documentation rather than implied away. The beam width is chosen so that the
   greedy failure mode cannot occur; a globally better set may still exist.
2. **Transitions are planned as an ordering, not yet as an operation.** The
   planner says which track follows which and why; it does not yet say *how* —
   the length of the blend, the equaliser moves, the filter sweep. That is the
   next module and it needs the timeline (Master Prompt #21).
3. **The learned profile of Master Prompt #5 is not yet an input.** The weights
   are the same for every user. The structure that will take a learned profile
   is in place — `ScoreComponents::WEIGHTS` is a named constant consulted in one
   function — but nothing populates it per user yet.
4. **The objective weights are provisional**, as ADR-0006 schedules, pending
   Phase 3 calibration. Risk R-03.
5. **Tempo is treated as a single number per track.** A track that changes tempo
   is planned as though it did not. `prv-time`'s tempo map supports the
   alternative; the analysis does not yet produce one.

## Definition of done

| Criterion | Status |
|-----------|--------|
| Architecture reviewed | Yes — no new records needed |
| Implemented, no placeholders | Yes, enforced |
| Tests passing | Yes — 499 |
| Performance validated | Cost is bounded and known before the search starts; measured in the suite |
| Documentation updated | Yes, in the same commit |
| Accessibility verified | Not applicable yet; two enabling decisions recorded |
| Security reviewed | Yes — no I/O, opaque identifiers |
| No critical technical debt introduced | Five recorded limitations, none critical |
