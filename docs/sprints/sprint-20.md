# Sprint 20 — The orchestrator

ADR-0006 has governed every musical module since Sprint 0 without anything
actually standing on the seam it draws. `prv-ai` is that: intent inward, evidence
outward, and a deterministic planner in between that a model never reaches.

| Delivered | Tests |
|-----------|-------|
| `prv-ai` — intent, agents, tasks, runs | 42 new; 794 in total |

All gates green: `cargo test`, `cargo clippy --all-targets -- -D warnings`,
`cargo fmt --check`, `cargo doc`, eight architecture rules, and the design-token
staleness gate.

## Business outcome

**A two-hour set can be planned by someone who has agreed to nothing.** Not as a
degraded mode — the same plan, the same order, the same planner, every step
resolving to a local agent, nothing leaving the device. What such a user loses is
the ability to type a sentence and the ability to read a paragraph of
explanation. What they keep is the product.

**The user is asked for permissions once, in advance.** `missing_agreements`
computes what a plan would need before anything runs, so an interface can ask a
single question at the start rather than interrupting someone four steps in with
one they could have answered at the beginning. The schedule also says, before it
starts, whether anything will leave the device.

**A failure early on never becomes a confident answer late on.** When a step
fails, everything downstream is skipped and says which step caused it. The
failure mode this exists against is not the visible error — it is the plausible
wrong result built on a measurement that never arrived, which is the worst thing
a system like this can produce.

## Architecture review

**No new decision record.** The crate implements ADR-0006. The one thing it adds
that the record does not name — the device-capability model — is described below
and is a refinement rather than a change of direction.

**A task names a capability, not an agent.** The agent is resolved at scheduling
time from what the user has agreed to and what the machine can do. That ordering
is the entire reason the offline path works: one plan, and the only difference
between a fully consenting user and a fully private one is which agent each step
resolves to.

**Availability is two questions, not one.** Has the user agreed to this, and can
this machine do it. Conflating them would tell someone on a modest laptop that
they had withheld a permission they never withheld — an error that reads as an
accusation.

**Scheduling is deterministic, and that is a requirement.** Among tasks whose
dependencies are met, the lowest identifier goes first, always. Master Prompt #19
asks for an orchestrator whose behaviour can be explained, and an order that
varied between runs would make every support conversation start with "and did it
do them in this order that time".

**A cycle is refused rather than broken.** Dropping an edge and carrying on
produces a plan that runs and is wrong, and the wrongness surfaces later,
somewhere else, as a task that read a value before it was written.

Nineteen crates, acyclic. `prv-ai` names `prv-mix`, `prv-time` and
`prv-security`, and the reason it names the first is worth stating: an
orchestrator that could not refer to the planner's own goal would have to
describe it in strings, which throws away exactly the type safety the planner was
built to have.

## The two properties this sprint exists for

**No musical decision is made on a server.** `decides_musically` and `location`
are checked against each other over every agent, so one added later that both
decided musically and ran remotely fails the build. The reason is not privacy,
though it is good for privacy: a musical decision has to be reproducible,
explainable and the same this evening as it was this afternoon. A request to a
model is none of those three.

**Losing the cloud costs fluency, never capability.** Checked twice — once over
the capability table, and once end to end by scheduling a whole set-planning plan
with `Consents::none()`. "Graceful degradation" is a phrase that means nothing
until something measures it.

## Findings raised on my own work

**Three tests failed because I had assumed stem separation needs the cloud, and
the tests were right.** ADR-0004 puts a separation model on the device. The
tempting fix was to change the tests; the correct one was to notice that the
tests had found a *missing model* — a local agent can be unavailable for a reason
that has nothing to do with consent. `Device` and `DeviceFeature` came out of
that, and they make three previously unreachable paths real: `NoAgent`,
`missing_agreements` returning anything, and `anything_leaves_the_device` being
true. Without them those three were defensive structure serving no case, which is
close enough to a placeholder to be worth the redesign.

**The adversarial sweep in `intent` found nothing, and it was still the right
test to write.** It walks nine durations against eighty-one tempo combinations —
NaN, both infinities, negatives, a million — and asserts every one produces
either a goal inside the planner's limits or an error naming a field. It passed
first time. It is the test that will fail the day somebody adds a field to
`PlanSet` and forgets to bound it, and that is the day it earns its place.

**Half a tempo range is dropped rather than completed.** The planner takes a
range or nothing. Completing a half-specified range would require inventing the
other end — a number nobody chose, arriving from a boundary whose entire purpose
is to stop invented numbers.

**A cascade keeps the first reason rather than the last.** When two independent
branches fail, the step that depended on both keeps the explanation from the
first failure. The `or_insert` that does this was a one-word decision and it
answers a real question: "why did this not run" has one useful answer, not the
most recent one.

## Security and privacy review

**Nothing here holds user content.** An intent is a duration, a shape and two
tempo bounds. A task is an identifier and a capability. A run is a set of
outcomes. The material a cloud agent would send never enters this crate.

**Every cloud agent names the agreement it needs**, checked as a pairing: an
agent that reached a server without naming a purpose would be one nobody could
withdraw consent for.

**Reading a sentence and hearing a record are separate agreements**, following
`prv-security`'s split. The material is different in kind — one is a recording
the user owns, the other is something they wrote, which may mention anything.

## Performance review

Nothing on a hot path. Scheduling is quadratic in the number of tasks in the
worst case and the plan is bounded at 1024; a realistic plan is under ten steps.
The cascade after a failure is one forward pass over a schedule already in
dependency order.

## Accessibility review

No surface. One decision in its favour: every intent, agent, capability, outcome
and failure carries a stable key, and a failure says whether trying again could
help — so an interface never offers a button that does nothing, which costs a
screen-reader user more than it costs anyone else.

## Are the project's principles still respected?

| Principle | Verdict |
|-----------|---------|
| Musical decisions are computed, not generated (ADR-0006) | Held, and now checkable over every agent. |
| The offline path is real, not degraded (ADR-0006, MP#26) | Held, and measured end to end. |
| Errors explain (MP#10) | Held. Every refusal names a field, a task or an agreement. |
| Never the easy solution when a premium one exists (MP#1) | Held. The device model came out of refusing to edit a failing test into agreement. |
| Production-ready, never placeholder (MP#13) | Held — and one near-miss caught, where three code paths existed for a case nothing could reach. |
| Quality is not a phase (MP#27) | Held. 794 tests; four findings within the sprint; review in the same commit. |

## Known limitations

1. **No agent is implemented.** This crate says what the agents are, where they
   run and in what order; it runs none of them. Each needs either a network, a
   model, or a crate that already exists and has to be wired to it.
2. **A plan is built by the caller.** There is no function from an `Intent` to a
   `TaskPlan` yet. Writing one requires knowing what is already analysed and what
   is not, which is a library question, and answering it here would put a stale
   copy of the library's state in the orchestrator.
3. **Suspending non-critical work during live playback is not modelled.** Master
   Prompt #19 requires it. It needs a notion of "now playing", which is transport
   state, and the seam between the two is not yet drawn.
4. **Conflict resolution between agents is not modelled**, because with no agent
   producing a proposal there is nothing yet to conflict. The task graph is where
   it will go.
5. **Cancellation is an outcome, not a mechanism.** `Failure::Cancelled` records
   that it happened; stopping something in flight belongs to whatever is running
   it.

## Definition of done

| Criterion | Status |
|-----------|--------|
| Architecture reviewed | Yes — ADR-0006 implemented; one refinement documented |
| Implemented, no placeholders | Yes; a near-miss found and closed by the device model |
| Tests passing | Yes — 794 |
| Performance validated | Nothing on a hot path; the plan is bounded |
| Documentation updated | Yes, in the same commit as the code |
| Accessibility verified | No surface; stable keys and an honest retry signal |
| Security reviewed | Yes — no user content passes through this crate |
| No critical technical debt introduced | Five recorded limitations; intent-to-plan is the next one to close |
