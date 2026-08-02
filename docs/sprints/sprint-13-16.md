# Sprints 13–16 — From the document to the door

Four sprints reviewed together. They form one arc: the document learned to hold
everything an edit produces (13), the system learned who it is working for (14),
the work learned how to leave (15), and the business rules learned where they are
allowed to live (16).

| Sprint | Delivered | Tests, cumulative |
|--------|-----------|-------------------|
| 13 | A trimmed front survives a reload | 572 |
| 14 | The personal profile | 590 |
| 15 | Delivery — compliance, dither, provenance | 612 |
| 16 | What a licence may and may not withhold | 623 |

All gates green at each commit: `cargo test`,
`cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`, `cargo doc`,
the architecture rules — six of them until Sprint 16 added the seventh — and the
design-token staleness gate.

## Business outcome

**Sprint 13 — the audio stops sliding.** A clip carried a source offset and the
document did not, so trimming the front of a clip moved its boundary in the
timeline and the audio underneath slid back to the beginning on the next open.
The document now records the offset, and the round-trip test checks it alongside
position, length, lane and media reference.

**Sprint 14 — the system has a user rather than a default user.** The objective
weights had been the same constant for everyone since Sprint 7, which ADR-0006
never intended: it says the weights come from the scenario *and* from the user's
learned profile, and a constant could do neither. They are a value now, carried
on the goal, and `prv-learning` infers a scaling from what the user actually did.

**Sprint 15 — an export can be judged before it is rendered.** A forty-minute set
takes minutes to render; telling someone afterwards that it peaks two decibels
too high has wasted both the time and the file. The compliance report is computed
from the analysis, before a single sample is written, and the manifest records
what went into the mix for licensing, reproducibility and integrity at once.

**Sprint 16 — the free tier is a real tier.** Playing your own music, seeing your
own library, opening your own projects and getting your finished work out are
available at every tier, after any expiry, permanently. That is Master Prompt
#29's promise, and this sprint made it a structure rather than a sentence.

## Architecture review

**No new decision records were needed.** Sprint 13 is ADR-0003 applied; Sprint 14
is ADR-0006 applied; Sprints 15 and 16 sit above the engines and introduce no new
seam. ADR-0007, written in Sprint 9, absorbed Sprint 13's question — where does a
source offset live? — without argument, which is the sign it drew the line in the
right place.

**A new operation, not a redefined one.** `SetPlacementSource` could have been a
field on `PlaceTrack`. ADR-0003 forbids that, because a project written by an
earlier build must keep its meaning, and a log that predates the operation means
an offset of zero — which is exactly what it meant when it was written. The rule
paid for itself here: the migration is *nothing*.

**Weights became a value and the type system did the rest.** `Weights::scaled`
clamps to a floor and a ceiling expressed as multiples of the default, so the
bound is stated once and cannot be exceeded by composing two profiles.

**`prv-entitlements` is deliberately alone.** It depends on nothing and nothing
in the engines depends on it. That isolation is not tidiness — it is the
mechanism. A promise not to artificially restrict essential functionality decays
by exactly one mechanism: somebody adds one tier check inside the mixer because
it was the convenient place, and a year later nobody can say what the free tier
does without reading the DSP. Architecture rule 7 makes that a build failure. The
consequence worth stating is that the audio graph, the planner and the analysis
pipeline behave identically at every tier *because they cannot tell which tier
they are running under.*

**I tested the rule, not just the code.** Rule 7 was verified by temporarily
adding `prv-entitlements` to `prv-dsp`'s manifest and confirming the check exits
non-zero with the right message, then restoring the manifest. A guard nobody has
seen fail is a guard nobody has seen work.

The layering held throughout. Fifteen crates, acyclic, no runtime dependencies.

## Audio quality review

**Dither is derived, not configured.** It is the kind of setting that is usually
a checkbox and usually wrong. Dithering into a lossy encoder adds noise the
encoder then spends bits describing, which is worse than the truncation it was
meant to mask; dithering a floating-point file adds noise to a file that lost
nothing. It follows from the depth and the format, and is tested in all four
combinations.

**"Needs gain" and "would clip" are different verdicts on purpose.** The remedies
differ and the user has to choose: limit the master, or deliver quieter than the
target. Picking one on their behalf would be making a mastering decision they did
not ask for and hiding it in a file they will hand to someone else.

**Every normalising target has a true-peak ceiling below full scale.** A platform
re-encodes to a lossy format and a lossy encoder overshoots its input, so a file
peaking at full scale arrives at the listener clipping on some players and not
others — the kind of defect that is reported from the field and cannot be
reproduced at the desk.

**An archive is not normalised at all.** Its ceiling of 0 dBTP is not a target but
a statement that the file must not already be clipping. Normalising an archive
destroys the information that would let you normalise it differently later.

## Machine learning review

**It learns from behaviour, not from a preferences screen.** People are poor at
introspecting about taste and good at exercising it: a DJ who would tell you they
never cut between records cuts between records all evening when the harmony is
wrong.

**It learns from what distinguished the choices, not from what was seen most.**
This is the decision the sprint turns on. A user who only ever saw harmonically
excellent suggestions and kept them all has said *nothing* about harmony — every
score was high, so the high scores do not separate the kept from the rejected.
Pearson correlation captures that and reads it as no signal; averaging would read
it as a strong preference and then over-weight the one thing the user had no say
in. There is a test for exactly this.

**It cannot reach the safety rules.** Learning moves how much a component counts,
within fixed bounds, and never switches one off. The hard constraints are not
weighted at all, so no amount of observation reaches them. A guarantee that a
sufficiently unusual user could train away is not a guarantee.

**An ignored suggestion teaches nothing, deliberately.** A suggestion scrolled
past says the user did not choose it, which is not the same as not wanting it —
they may never have seen it. Counting silence as rejection is how a recommender
talks itself into a narrower and narrower corner.

## Security and privacy review

**The profile contains no personal data because it contains no data about
people.** Six scores and an outcome per observation: no track name, no artist, no
path, no device, no clock reading. The cheapest way to keep a profile from
becoming personal data is for it never to contain any.

**No clock, and that is a feature twice over.** ADR-0001 keeps the core free of
operating-system calls, which rules out reading the time; and recency measured in
*decisions made* is the better metric anyway. A user who has not opened the
application for a month has not changed their taste.

**A manifest refers to media and never contains it.** Whether a particular
manifest is fit to leave the device is the caller's decision, not this crate's —
title and artist are optional and absent by default.

**A privacy choice is not answered with a sales prompt.** `Denial` distinguishes
"your tier does not include this" from "you turned this off". Master Prompt #26
lets a user disable cloud features; responding to that with an upgrade offer
would be treating their decision as a mistake.

**An expired licence keeps the user's disabled list.** Someone who turned cloud
analysis off has not changed their mind by failing to renew.

## Findings raised on my own work

**Sprints 13, 14 and 15 shipped without their review documents.** The standing
instruction is a review after every sprint. The status and traceability documents
were updated each time; the narrative one was not. This is the *second* time —
Sprint 11 had the same gap, and the Sprint 11–12 review named it as the sort of
omission that compounds if it is not noticed. It compounded. Recorded here rather
than quietly backfilled, and the lesson taken is that "write the review" belongs
in the same commit as the code rather than after it.

**Two test premises in Sprint 15 were wrong, and fixing them improved the
tests.** A full-scale sine reads −3.01 LUFS, so −14 LUFS is an amplitude of
0.282, not 0.2. And a loud mix cannot clip on the way to a quieter target — it
gets turned down; the signal that clips is quiet with tall peaks, which is now
what the test builds. Both were cases of the test asserting my arithmetic rather
than the requirement.

**`prv-entitlements` has an empty dependency section, and that is the point.**
It would have been natural to give it `prv-project` for a `TrackRef` or two. It
has none, so the isolation is visible in the manifest and not only in a shell
script.

**`required_tier` returns `Option`, not `Tier::Free`.** They are equivalent
today. They are not equivalent to a future reader: `Some(Tier::Free)` is a floor
someone can raise in a one-line change, and `None` says the feature is not tiered
at all. The essential-feature arms are also written out explicitly rather than
caught by a wildcard, so adding a variant without deciding its tier is a compile
error.

## Performance review

Nothing on a hot path changed. Inference is linear in the number of observations
and bounded at 4096; compliance reporting is arithmetic on an already-computed
loudness measurement; an entitlement check is a comparison and a small linear
scan of the user's own disabled list.

## Accessibility review

No user-facing surface yet, but two decisions are accessibility decisions in
advance. Every tier, feature, outcome and target carries a stable key rather than
a display string, so the interface layer localises without the core knowing any
language. And a denial names the tier that would grant the feature, so an
interface can say what to do rather than only that something is unavailable —
Master Prompt #10 requires errors to explain, and "upgrade" with no destination
is not an explanation.

## Are the project's principles still respected?

| Principle | Verdict |
|-----------|---------|
| Never the easy solution when a premium one exists (MP#1) | Held. Correlation rather than averaging; a new operation rather than a redefined one; derived dither rather than a checkbox. |
| The user owns their work (MP#9, MP#29) | Held, and now structural: export is essential, expiry is the free tier, a profile can be deleted. |
| Privacy by design (MP#26) | Held. The profile cannot become personal data because it never holds any. |
| Production-ready, never placeholder (MP#13) | Held. |
| The log is the source of truth (ADR-0003) | Held. Sprint 13's gap was in the log, and it was closed in the log. |
| Modular, dependencies inward (MP#4, MP#7) | Held, and strengthened: rule 7 makes one more layering promise mechanical. |
| Quality is not a phase (MP#27) | Held on code — 623 tests, and the new gate was itself tested. Not held on documentation: three reviews were late. |

## Known limitations

1. **The renderer still does not adjust playback rate.** Unchanged since
   Sprint 11; it needs a time-stretching processor, which is Phase 2 audio work.
2. **The incoming track still enters at its start, not at its intro.** The log
   can now record a source offset, so the obstacle Sprint 12 recorded is gone —
   the renderer simply does not yet use it.
3. **The learned profile is never persisted.** `prv-learning` holds observations
   in memory and the project document has no place for them. Where a profile
   lives is a real question — it is not part of any one project — and it is
   deliberately left until there is a settings store to answer it.
4. **No export actually happens.** Sprint 15 built the part that can be decided
   before rendering. Writing a file needs an encoder and a filesystem, both of
   which are outside the core by ADR-0001.
5. **Nothing consumes entitlements yet.** The crate is complete and unreferenced,
   which is the correct state for it: the feature boundary that will consult it
   is in the application layer, which the Linux environment cannot compile.
6. **Every calibration item remains open** — harmonic weights, default objective
   weights, confidence thresholds, technique thresholds, the learning bound —
   all scheduled for Phase 3 by ADR-0006.

## Definition of done

| Criterion | Status |
|-----------|--------|
| Architecture reviewed | Yes — no new records needed; one new build-enforced rule |
| Implemented, no placeholders | Yes, enforced by rule 3 |
| Tests passing | Yes — 623 |
| Performance validated | No hot path changed |
| Documentation updated | Yes, and three late reviews recorded rather than backdated |
| Accessibility verified | No surface yet; two decisions made in its favour |
| Security reviewed | Yes — privacy is the substance of Sprints 14 and 16 |
| No critical technical debt introduced | Six recorded limitations; profile persistence is the most concrete |
