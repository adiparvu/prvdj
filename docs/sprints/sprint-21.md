# Sprint 21 — A request becomes work

Two of Sprint 20's recorded limitations, closed. `prv-ai` had a vocabulary and no
way to get from a request to a plan, and Master Prompt #19's rule about live
playback was named in the specification and implemented nowhere.

| Delivered | Tests |
|-----------|-------|
| `prv-ai::compose`, urgency and deferral | 13 new; 807 in total |

All gates green: `cargo test`, `cargo clippy --all-targets -- -D warnings`,
`cargo fmt --check`, `cargo doc`, eight architecture rules, and the design-token
staleness gate.

## Business outcome

**Asking for something now produces the work that thing needs, and nothing
else.** Analysis appears in a set-planning plan only when something is
unanalysed. Asking an already-analysed library to analyse itself produces an
empty plan — not a plan with a step that does nothing, because a caller that has
to distinguish those two will eventually get it wrong, and because the honest
answer is *nothing*.

**A performance is not interrupted by housekeeping.** Master Prompt #19 requires
non-critical work to be suspended during live playback, and this is where that
becomes real. Library analysis started as background work waits for the room to
empty. The same analysis, needed for a set the DJ just asked for, runs — because
they asked, and refusing what somebody just requested is worse than doing it.

**Nothing is silently dropped.** A deferred task stays in the schedule's deferred
list. A plan that quietly did less during a set would leave a user wondering why
their library never finishes analysing, which is the kind of defect nobody ever
files because nobody can describe it.

## Architecture review

**Urgency is a property of why a task is in the plan, not of what it does.** This
is the sprint's central decision. Analysing a track is background work on a
Tuesday afternoon and the most urgent thing in the building when a DJ has just
asked for the next hour. A model that put "deferrable" on the *capability* would
have got both cases wrong in opposite directions.

**Urgency travels backwards along dependencies.** Anything a requested task needs
is itself requested, computed to a fixed point before scheduling. That gives the
invariant the whole deferral rule rests on: the deferred set is closed under
dependency, so no step in a schedule ever waits for something that is not in it.
There is a test for the invariant itself rather than only for its consequences.

**The default urgency is `Requested`.** Work whose urgency nobody thought about
is work somebody is waiting for, so forgetting to mark a task never makes the
product feel like it stopped. The failure direction matters more than the
default.

**The situation is a parameter, never a field.** `plan_for` is handed the count
of unanalysed tracks, whether the set has anything in it, and what the machine
can do. Holding any of that here would produce a copy that is stale the moment a
track finishes importing, and the plans built from it would be confidently wrong
in the hardest direction to notice: a set planned over music the system believed
was analysed and was not.

**A cycle among deferred tasks is still an error.** A plan that would only be
wrong later is wrong now.

Nineteen crates, unchanged, acyclic.

## Findings raised on my own work

**I wrote the exact pattern I named as a smell in Sprint 12, and caught it on
re-reading.** The first `plan_for` discarded the result of every `add` with
`let _ =`, under a comment explaining that none of them could fail — while the
same comment claimed the code "keeps the compiler's attention on it", which it
demonstrably did not. That is a comment arguing for an incomplete design.
`ComposeError` now carries `CouldNotAssemble(TaskError)` and every `add` uses
`?`. The variant is unreachable from anything this module composes, and it says
so in its own documentation; what it buys is that a change which *could* fail
would be caught rather than producing a plan quietly missing a step.

**"A composed plan always schedules" is the test that justifies the module.**
Thirty combinations of situation, intent, device and activity, each composed and
then scheduled. If the composer ever built a plan the scheduler refuses, a user
would meet it as "it just does nothing" — the least diagnosable failure
available. It is cheap and it covers the seam between two things written a sprint
apart.

**A request about an empty set is refused rather than answered emptily.**
Explaining a choice that was never made is not a failure of the system; it is a
request about something that does not exist. "There is nothing in the set" is
more useful than an empty answer, which reads as "the system had nothing to say".

## Performance review

Urgency propagation is a fixed-point loop bounded by the number of tasks, over a
plan capped at 1024 and realistically under ten. Nothing on a hot path; nothing
reachable from the audio thread.

## Security review

Nothing new. `Situation` holds three numbers and no identity — no track
references, no paths, no names — so a composed plan cannot carry anything about a
user's library beyond how much of it is unanalysed.

## Accessibility review

No surface. One decision in its favour: a schedule reports deferred work rather
than omitting it, so an interface can say "waiting until the set finishes" — a
state, not an absence. An absence is what a screen reader cannot announce.

## Are the project's principles still respected?

| Principle | Verdict |
|-----------|---------|
| Audio performance first, always (MP#19) | Held, and now implemented rather than intended. |
| Nothing is lost (MP#9) | Held. Deferred is a state, not a deletion. |
| Never the easy solution when a premium one exists (MP#1) | Held. Urgency on the reason rather than the capability; propagation rather than a hand-maintained list. |
| Production-ready, never placeholder (MP#13) | Held — after catching myself writing the pattern I had already named once. |
| Errors explain (MP#10) | Held. "Nothing in the set" rather than an empty result. |
| Quality is not a phase (MP#27) | Held. 807 tests; the review in the same commit, third sprint running. |

## Known limitations

1. **Still no agent implementations.** Unchanged from Sprint 20, and the largest
   remaining gap in this crate.
2. **`Situation` is narrow on purpose and will grow.** Three fields today. Each
   addition should be a value the composer genuinely branches on, not a general
   picture of the application, or it becomes the stale copy this design avoids.
3. **Deferred work is not resumed by anything.** The schedule says what waits;
   scheduling again when the performance ends is the caller's move. That is the
   right split — the core does not know when a set finishes — but nothing yet
   does it.
4. **Conflict resolution between agents** remains unmodelled, for the same reason
   as last sprint: nothing yet produces competing proposals.
