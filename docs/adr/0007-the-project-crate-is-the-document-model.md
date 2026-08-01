# ADR-0007: The project crate is the document model

- Status:        Accepted
- Date:          2026-08-01
- Review date:   2027-02-01
- Deciders:      Chief Architect, Timeline Engineering, Cloud Engineering
- Related modules: project, timeline, plugin-manager, audio-graph
- Source requirements: MP#4 (Modularity), MP#9 (Nothing is lost), MP#21 (Timeline
  and automation), MP#23 (Plugin parameters), MP#24 (Synchronisation)
- Supersedes:    nothing. Refines the scope of
  [ADR-0003](0003-project-document-and-persistence.md).

## Context

ADR-0003 established that a project is an append-only operation log and that its
state is a pure fold over that log. It said what the log *is*; it did not say
which crate owns the **vocabulary** the log records.

Sprint 8 made that gap concrete. `prv-timeline` introduced parameter addressing
so that automation could name what it drives, and put it there because that is
where automation lives. Sprint 9 then needed the log to record an automation
edit — without which every sweep a user draws sits outside undo, outside
versions and outside synchronisation, which ADR-0003 forbids.

That produces a cycle:

- The log needs the timeline's vocabulary, to record what an edit addressed.
- The timeline needs the log's identities, because a clip *is* a placement.

A cycle between two domain crates cannot be built and should not be worked
around with a duplicate type, because two definitions of an address is two
things that can disagree about what a project file means.

## Options

### A. Duplicate the address in both crates, converting at the boundary

Breaks the cycle and keeps each crate self-contained.

Rejected. The conversion is where the two definitions drift, and the symptom of
drift is a project that opens with automation pointing somewhere it did not
point when it was saved. That is precisely the failure the addressing scheme was
designed to make impossible.

### B. A third crate holding shared vocabulary

Also breaks the cycle, and is the textbook answer.

Rejected for this case. The vocabulary in question — addresses, interpolation
shapes, marker kinds, track references — is not shared between peers; it is
*the document's*, and every one of these values exists because the document
stores it. A crate whose contents are all "things the document stores" is the
document crate with a different name, and Master Prompt #4 warns specifically
against fragmenting for tidiness.

### C. The project crate owns everything the document persists (chosen)

`prv-project` is not "the log". It is the **document model**: the log, the fold,
and every value either of them holds.

## Decision

**A value that a project file contains is defined in `prv-project`.** A value
that only exists at runtime is defined by whichever crate computes it.

The line is *persistence*, not subject matter, and it cuts through the middle of
what looked like one concept:

| Concept | Home | Why |
|---------|------|-----|
| `ParameterAddress` — *which* parameter | `prv-project` | Stored in the log, synchronised, must mean the same thing in ten years |
| `ParameterDescriptor` — *what values it takes* | `prv-timeline` | Declared at load time by the engine or a plugin; never stored |
| `Interpolation` — the shape of a curve segment | `prv-project` | Stored with every automation point |
| `AutomationLane` — an evaluable curve | `prv-timeline` | Built from the document, optimised for the audio thread, never stored |
| `Clip`, snapping, editing | `prv-timeline` | A shape the fold is read in, and the rules for changing it |

Dependencies run one way: `prv-timeline` → `prv-project` → `prv-time`.

`prv-timeline` re-exports the identity half, so a caller building a timeline
does not have to know which crate owns which piece.

## Consequences

### Positive

- **The cycle is gone**, and gone structurally rather than by convention.
- **There is one definition of what a project file means.** A value that
  round-trips through storage has exactly one type, so there is nothing for two
  definitions to disagree about.
- **The rule is mechanical.** "Does a project file contain it?" is a question
  with one answer, which makes the next such decision a lookup rather than a
  debate.
- **Automation joins the log**, and therefore inherits undo, versions,
  branching, merge-conflict reporting and incremental synchronisation without
  any of them being reimplemented.
- **Values arriving from a log are validated once, at the fold.** A log can come
  from another device or an older build, and an automation value reaches the
  audio thread; clamping in `apply` is the single place that has to be right.

### Negative

- **`prv-project` grows.** It now holds parameter addressing, which is not
  obviously "the log" to a reader who has not read this record. Mitigated by the
  crate documentation stating the rule and by this record existing.
- **`Target` and `OperationPayload` are no longer `Copy` or `Eq`.** An address
  owns a boxed owner chain and, for a plugin parameter, a string; an automation
  point carries an `f32`, and floating point has no total equality. Both are
  honest: claiming `Eq` for a type containing a float would be a lie about its
  semantics, and this codebase already refused that once, in `prv-time::TimeError`.
- **A future value that is *nearly* persisted will be a judgement call.** The
  rule handles the clear cases; the unclear ones will still need thought.

### Neutral

- The move is source-compatible for callers, because `prv-timeline` re-exports
  what moved.

## What this does not change

ADR-0003 stands unchanged: the log is still the source of truth, state is still
a pure fold, undo is still an appended inverse, and conflicts are still detected
as fact rather than guessed. This record only says where the vocabulary that
those mechanisms operate on is defined.
