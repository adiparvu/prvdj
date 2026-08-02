# Sprint 31 — The planner reaches the host

The boundary could start an engine and render a track. It could not do the one
thing the product is named for.

| Delivered | Tests |
|-----------|-------|
| `prv-ffi::planning` — the library, the plan, and applying it | 11 |
| Planner entry points and the extended header | (covered by the above) |
| `PRVCore.Planner` | 9 (Swift) |
| — | **991 Rust, 24 Swift** |

## Business outcome

**"A two-hour set that builds" now goes in and a tracklist comes out.** A host
describes its library one track at a time, asks for a set, reads it back, and
applies it. That is the product's promise, and until this sprint it stopped at
the language boundary.

**A generated mix is an edit, not a result.** `prv_planner_apply` appends
ordinary operations to the log — the same ones a hand-made edit produces. Master
Prompt #3B requires the user to be able to edit everything the system decides,
and after this call there is nothing in the document that says which placements a
person made and which the planner did. It is undoable, branchable and
synchronisable because it is not special.

**"Nothing you own fits" is an answer.** Planning an empty or incompatible
library returns `PRV_REFUSED` rather than an empty plan. An empty list is
indistinguishable from success and teaches a user to distrust the feature.

## Architecture review

**Candidates are arguments, not a struct.** A `#[repr(C)]` candidate would be a
permanent layout promise, and the first field anybody wants to add — danceability,
a stem count, whatever Phase 3 measures — breaks every host compiled against it.
One call per track costs a few milliseconds across a ten-thousand-track library,
once, on the thread that was reading the library anyway. What it buys is that
next year's addition is a new function rather than a new major version.

**A plan is held, not returned.** Returning a variable-length result across C
means either allocating something the host frees — a second ownership rule for
every host language to get right — or a two-pass "ask the size, then ask again"
dance that races if anything changes between passes. Holding it means nothing is
allocated on the host's behalf, nothing has to be freed, and the count cannot
change under a caller, because making a new plan is a different call.

**The planner is a separate handle from the engine.** A library and a plan are
not a project. A host may plan with nothing open and may keep a project open
while replanning; tying them together would make the first impossible and the
second awkward, and they share no invariant that would justify it.

**The ABI is 1.1 rather than 2.0.** Calls were added and nothing existing moved,
which is exactly what the minor field promises: a host built against 1.0 keeps
working and simply does not plan.

## What the tests assert that matters

**Applying a plan twice does not reuse a placement identity.** ADR-0003 says an
identity is never handed out twice. Reuse would make a later merge drop the
second set as "already present", and the user would see a successful edit with
half the work missing — the same failure mode as the branch collision fixed in
Sprint 27, reached by a different route.

**Records that hand over early make a shorter set, and Swift can see it.** The
pacing rule from Sprint 28 is now asserted from both sides of the boundary. If
the planner and the renderer ever disagree about time again, it will be across
two languages, where it is much harder to notice — so it is pinned in both.

**Unknown is not the same as no.** A track nobody has analysed for vocals is not
a track known to be instrumental, and the two score differently. The Swift
wrapper maps an absent key to zero confidence rather than to a separate call the
caller could forget, and a test says so.

## Known limitations

- Analysis is not across the boundary. A host must supply tempo, key, energy and
  loudness from somewhere; `prv-analysis` computes all of them in Rust and is not
  yet reachable from C. A minor addition, not a redesign.
- The library index, consent, entitlements and notifications are likewise
  reachable in Rust only.
- `PRVKit`'s framework adapters and `PRVUI` remain unwritten. The boundary they
  sit on is now complete enough to write them against.
