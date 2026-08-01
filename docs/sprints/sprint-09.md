# Sprint 9 — Automation joins the document

Sprint 8 built automation and recorded that it was not yet undoable. Closing
that turned out to require an architectural decision rather than a feature, and
this sprint is mostly that decision and its consequences.

| Delivered | Tests |
|-----------|-------|
| ADR-0007, and the move it describes | — |
| Automation operations in the log, with inverses | 3 |
| Automation in the state fold, with validation | included above |
| `Timeline::from_project` | 2 |

545 tests in total, up from 540. All gates green: `cargo test`,
`cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`, `cargo doc`,
all six architecture rules, and the design-token staleness gate.

## Business outcome

**A filter sweep is now a first-class part of a project.** It can be undone,
redone, named as a version, compared against another version, branched,
synchronised incrementally to another device, and reported as a conflict if two
people edit it at once — none of which was reimplemented. It inherits all of it
from the log.

That is the return on ADR-0003's original decision, and it is worth naming: the
sprint that added automation to the document added *one enum variant family and
its inverses*, and six features arrived with it.

## Architecture review

**One new decision record, and it was needed.** ADR-0003 said what the log *is*
and never said which crate owns the vocabulary the log records. Sprint 8 put
parameter addressing in `prv-timeline` because that is where automation lives;
Sprint 9 needed the log to record an automation edit. That is a cycle: the log
needs the timeline's vocabulary, and the timeline needs the log's identities,
because a clip *is* a placement.

ADR-0007 resolves it by drawing the line at **persistence rather than subject
matter**, which cuts through the middle of what looked like one concept:

- `ParameterAddress` — *which* parameter — is stored, synchronised, and must
  mean the same thing in ten years. It is document vocabulary.
- `ParameterDescriptor` — *what values it takes* — is declared at load time by
  the engine or a plugin and never stored. It is runtime description.

The rejected alternatives are recorded because both were tempting. Duplicating
the address in both crates breaks the cycle and is where two definitions drift —
and the symptom of that drift is a project opening with automation pointing
somewhere it did not point when it was saved, which is exactly the failure the
addressing scheme existed to prevent. A third shared-vocabulary crate is the
textbook answer and would have been a crate whose entire contents are "things
the document stores", which is the document crate with a different name.

**The rule is mechanical, which is the point.** "Does a project file contain
it?" has one answer, so the next such decision is a lookup rather than a debate.

## Findings raised on my own work

**The layering error was mine, from one sprint earlier.** Sprint 8 put parameter
addressing in `prv-timeline` and the sprint review recorded, as limitation 1,
that the timeline was "not yet wired to the operation log" — describing it as
remaining work rather than as the symptom of a boundary in the wrong place. It
was only when the wiring was attempted that the cycle appeared.

Worth recording honestly: the limitation was written accurately and read
optimistically. "Not yet wired" and "cannot be wired as currently structured"
look identical from the outside, and the only reliable way to tell them apart
was to try.

**Two type-level claims had to be given up, and both were lies to begin with.**
`OperationPayload` and `Target` lost `Eq` and `Copy`. An automation point carries
an `f32`, and floating point has no total equality; an address owns a boxed chain
and, for a plugin parameter, a string. This codebase refused exactly this claim
once before — `prv-time::TimeError` does not derive `Eq` for the same reason —
and it was right both times.

**No wildcard arm survives in the fold or in `inverse_of`.** `non_exhaustive`
binds other crates, not the defining one, so within `prv-project` the compiler
still demands every variant. That makes "a variant added without a fold arm" a
build failure rather than an edit that silently vanishes when a project is
reopened. Both wildcards were written first and both were removed once the
compiler pointed out they were unreachable — a case where the lint was making a
design point, not a style one.

## Security review

**One validation point, deliberately placed.** An automation value reaches the
audio thread, and a log can arrive from another device or an older build. The
value is clamped in `ProjectState::apply` — at the fold, where a foreign log
enters — rather than at each read. Tested against a non-number and against
values outside the range in both directions.

The same reasoning as Sprint 8's plugin identifier validation: check where the
value crosses the boundary, because the place that forgot to check on use is the
vulnerability.

**A project that cannot be held is reported, never clamped.**
`Timeline::from_project` skips clips outside its limits and returns how many it
skipped. A user unable to open their own project is a worse outcome than one
told that three clips could not be shown — but silently moving their work to a
lane they did not choose is worse than either.

## Performance review

Unchanged. `from_project` runs when a project opens and after a merge, never on
the audio thread and never per frame. The state holds automation in a
`BTreeMap`, so iteration order is address order on every platform and every run,
which is what makes a rendered export match a preview.

## Accessibility review

No user-facing surface, and no new decisions.

## Are the project's principles still respected?

| Principle | Verdict |
|-----------|---------|
| Continuously refactor when necessary (MP#1) | Held, and exercised. A boundary in the wrong place was moved rather than worked around. |
| Never the easy solution when a premium one exists (MP#1) | Held. The easy solutions were a duplicate type and a third crate; both are recorded with why they were refused. |
| Nothing is lost (MP#9) | Held and extended. Automation is now inside the mechanism that guarantees it. |
| Modular, dependencies inward (MP#4, MP#7) | Held, and now acyclic where it was about to stop being. |
| Decisions are recorded (MP#31) | Held. ADR-0007 exists because a future reader will otherwise find parameter addressing in "the log crate" and assume it was an accident. |
| Quality is not a phase (MP#27) | Held. 545 tests; the refactor was made under a green suite the whole way. |

## Known limitations

1. **Transitions are still an ordering, not an operation.** `prv-mix` says which
   track follows which; `prv-timeline` can express *how*; nothing yet converts
   one into the other. This is now the largest gap between what the system knows
   and what a user would see.
2. **Ripple editing is not implemented**, unchanged from Sprint 8.
3. **Effect chains are addressed but not modelled**, unchanged. That belongs with
   the plugin manager.
4. **The timeline does not yet emit operations.** It can be *built* from the
   document; edits made through `Timeline` do not yet produce log entries. The
   two halves exist and the seam between them is the next piece of work.
5. **Lane numbers remain positional**, unchanged from Sprint 8.

## Definition of done

| Criterion | Status |
|-----------|--------|
| Architecture reviewed | Yes — one new record, ADR-0007 |
| Implemented, no placeholders | Yes, enforced |
| Tests passing | Yes — 545 |
| Performance validated | No change to any hot path; `from_project` runs on open |
| Documentation updated | Yes, in the same commit |
| Accessibility verified | Not applicable; no new decisions |
| Security reviewed | Yes — one validation point, at the fold |
| No critical technical debt introduced | Five recorded limitations; one of Sprint 8's was closed |
