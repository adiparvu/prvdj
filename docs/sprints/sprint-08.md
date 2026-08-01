# Sprint 8 — The timeline, and the debt it closes

Sprint 7 produced a set. This one produces the surface a user edits it on —
and, in doing so, closes the one limitation that has been recorded in every
review since Sprint 1.

| Delivered | Tests |
|-----------|-------|
| Parameter addressing and descriptors | 9 |
| Automation lanes and interpolation | 14 |
| Clips, lanes and editing with snapping | 15 |
| Numeric conversions | 2 |

540 tests in total, up from 499. All gates green: `cargo test`,
`cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`, `cargo doc`,
all six architecture rules, and the design-token staleness gate.

## Business outcome

**A user can edit the mix the planner produced.** Move a clip, trim it, split
it, draw a filter sweep across four bars — with everything landing on the grid
the analysis engine worked so hard to find.

**Automation is a feature the product can now have.** It has been blocked since
Sprint 1 on a question nobody had answered: how do you *name* a parameter?

**A plugin can expose its own controls.** Master Prompt #23's parameter model
needs the same addressing scheme, and it is now there, with the validation a
value crossing a trust boundary requires.

## Architecture review

**No new decision records were needed.** ADR-0002 governed the evaluation path
and ADR-0003 governed the relationship to the operation log; both were specific
enough that this was implementation.

**The debt is closed, and the way it was closed is the point.** From Sprint 1
onward every review recorded that processors had no parameter-addressing scheme
and that automation and plugin parameters both needed one. It was deliberately
not improvised around a single processor's needs, and this sprint shows why that
restraint paid: three consumers needed the same answer, and each of them alone
would have produced something the other two could not use.

- Automation needs an address it can *store*, so a pointer is out.
- The operation log needs one that still means the same thing after the effect
  slots have been reordered, so an index is out.
- Plugins need one the host has not compiled anything about, so a closed enum
  alone is out.

The answer is a typed owner plus a typed key. Stability then follows from
something already guaranteed — a placement identifier is stable because
ADR-0003 says so — rather than from a new promise this module would have to
keep.

**The timeline is a shape, not a second source of truth.** Nothing here
persists anything or holds history. Undo, versions and branching belong to the
log; two mechanisms for going back in time is how a project ends up able to
reach a state neither of them believes in.

The layering held. `tools/check-architecture.sh` passes unchanged.

## Audio quality review

**No interpolation shape overshoots.** This is the decision that rules out the
spline a graphics library would reach for. A Catmull-Rom through three points
overshoots on the way to a peak; an overshoot on a gain lane is a value above
unity, and on a filter lane a frequency past Nyquist. Every shape here satisfies
`f(0) = 0`, `f(1) = 1`, stays inside those bounds, and is *monotone* — tested by
walking each shape across its whole domain rather than at a few sample points.

Monotonicity is the stronger claim and the one a user feels: dragging a point up
must not make any part of the segment go down.

**`Smooth` is a raised cosine**, whose derivative is zero at both ends, so
joining several of them produces a curve with no corners. A corner in a gain
envelope is audible as a click.

**A non-number can never reach the signal path.** Values are clamped at
construction. Infinities are clamped rather than zeroed, because they are
ordered and therefore have a defensible position on the scale; only a value that
is not a number has none, and zero is the safer end to send it to.

## Performance review

`AutomationLane::value_at` is the function the audio thread calls, once per
control block per automated parameter. It does a binary search plus one
interpolation over a slice: no allocation, no locking, and work logarithmic in
the number of points.

The linear alternative would have been correct and would have stalled. A
four-hour set with a point per bar is thousands of points on one lane, and a
scan per parameter per block is the shape of a dropout. Tested at a
thousand-point scale, checking the search agrees with the answer.

The timeline holds clips in a `BTreeMap` rather than a hash map, so iteration
order is identifier order on every platform and every run. ADR-0006 requires
that of anything a decision is derived from and ADR-0003 requires it of anything
synchronised — and it is what makes a rendered export match a preview.

## Security review

One new trust boundary, and it is handled at the boundary rather than at each
use. `PluginParameterId` validates on construction: non-empty, at most 64 bytes,
and restricted to lowercase letters, digits, underscore and dot.

The character restriction is not fastidiousness. These values are used as
storage keys and appear in log output, so a value that can contain a newline or
a path separator is a value that can be used to forge either. Validating once,
where the value enters, is what makes every later read safe; validating on use
means the one place that forgot is the vulnerability. Tested against `../../etc/passwd`
and an embedded newline explicitly.

Two bounds were added for the same reason Master Prompt #26 requires limits on
anything a document can grow without bound: `MAX_POINTS` on a lane, which is
written to by a gesture that can be held down, and `MAX_CLIPS` and `MAX_LANES`
on a timeline, which can arrive from a file.

## Accessibility review

No user-facing surface. Three enabling decisions: `ParameterKey::key`,
`Interpolation::key` and the error types all return stable identifiers rather
than English prose, keeping translations in the presentation layer where Master
Prompt #8 puts them. And `automation_at` returns addresses ordered by the
address type's own ordering rather than by display text — an order that depended
on display text would change with the user's language.

## Findings raised on my own work

**One test expectation was wrong about my own error handling, and fixing it
improved the code.** The test asserted that positive infinity clamps to one; the
implementation zeroed every non-finite value. Working out which was right
produced a better rule than either: infinities are *ordered* and so have a
defensible position on the scale, while a value that is not a number has none.
The implementation now special-cases only the latter, which is both simpler and
more defensible than what it replaced.

**One test asserted an ordering the type does not promise.** Automation was
expected in alphabetical order by display string; the type orders by its own
`Ord`, which is total and reproducible but follows declaration order. The
guarantee that matters is reproducibility, and it is now asserted directly —
against the address type's ordering rather than against text that will be
localised.

**Two API names were wrong in a way clippy caught and a reviewer would not
have.** `ParameterCurve::from_normalised` took `self`, which reads as a
constructor and is not one. Renamed to `to_linear`, with the reason recorded
where it might otherwise be renamed back.

**No defects were found in earlier sprints' work**, for the second sprint
running. The `PlacementId` that `prv-project` defined in Sprint 3 turned out to
be exactly the stable identity this module's addressing needed, which is some
evidence that giving identity to placements rather than to positions was the
right call at the time.

## Are the project's principles still respected?

| Principle | Verdict |
|-----------|---------|
| Never the easy solution when a premium one exists (MP#1) | Held. A typed address tree rather than a string path; a binary search rather than a scan; monotone shapes rather than a spline. |
| Production-ready, never placeholder (MP#13) | Held and enforced. |
| Modular, dependencies inward (MP#4, MP#7) | Held. Twelve crates, still no runtime dependencies. |
| Nothing is lost (MP#9) | Held. A disabled automation lane keeps its points; a refused edit changes nothing; a trim is not a delete. |
| Realtime contract (ADR-0002) | Held on the one function that touches the audio thread, and documented on the ones that do not. |
| Never guess where a user meant (MP#21) | Held. Snapping is opt-out per edit rather than a global mode, so a gesture always means the same thing. |
| Quality is not a phase (MP#27) | Held. 540 tests; a longstanding limitation closed rather than carried. |

## Known limitations

1. **The timeline is not yet wired to the operation log.** It is the shape the
   fold produces; `prv-project` does not yet carry automation operations, so an
   automation edit is not yet undoable. That is a change to the log's payload
   enum and is the natural next step.
2. **Transitions are still an ordering, not an operation.** `prv-mix` says which
   track follows which; this crate can now express *how* — a filter sweep, a
   level ride — but nothing yet converts one into the other.
3. **Ripple editing is not implemented.** Moving a clip does not move what
   follows it. For a DJ set, where clips are laid end to end, this is a real
   convenience gap rather than a correctness one.
4. **Effect chains are addressed but not modelled.** `ParameterOwner::Effect`
   can name a slot; nothing yet says what is in it. That belongs with the plugin
   manager (Master Prompt #23).
5. **Lane numbers are positional.** Reordering lanes re-addresses their
   automation. Recorded because it is a deliberate choice — automation should
   follow content, and content moves with the lane — but a user who reorders
   lanes expecting automation to follow the *row* would be surprised.

## Definition of done

| Criterion | Status |
|-----------|--------|
| Architecture reviewed | Yes — no new records needed; one longstanding limitation closed |
| Implemented, no placeholders | Yes, enforced |
| Tests passing | Yes — 540 |
| Performance validated | Evaluation is logarithmic and allocation-free, tested at scale |
| Documentation updated | Yes, in the same commit |
| Accessibility verified | Not applicable yet; three enabling decisions recorded |
| Security reviewed | Yes — one new trust boundary, validated at the boundary |
| No critical technical debt introduced | Five recorded limitations, none critical |
