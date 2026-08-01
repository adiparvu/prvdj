# ADR-0003: Project document, event sourcing and persistence

- Status:        Accepted
- Date:          2026-08-01
- Review date:   2027-02-01
- Deciders:      Chief Architect, Database Architect, Cloud Platform Lead
- Related modules: project-management, timeline, music-library, cloud-sync, export
- Source requirements: MP#3C (non-destructive editing), MP#7 (version history at every stage),
  MP#9 (Versioning — *projects are immutable snapshots*), MP#21 (History, branching),
  MP#24 (offline-first, conflict resolution), MP#29 (content ownership)

## Context

Four separate specifications converge on the same requirement from different
directions. MP#3C requires every AI action to be reversible and forbids
overwriting originals. MP#7 requires version history at every stage of the
project pipeline. MP#9 states that projects are immutable snapshots supporting
restore, compare, duplicate, branch and merge, and that records are never
permanently deleted by default. MP#21 requires branching history on the timeline.
MP#24 requires offline editing with incremental synchronisation, conflict
detection and no silent discarding of user work.

A mutable document with an undo stack bolted on cannot satisfy these. Undo stacks
are process-local, do not survive a crash, cannot branch, cannot be compared, and
cannot be synchronised incrementally.

## Problem

What is the project's data model, and how does it persist locally and
synchronise to the cloud without ever losing user work?

## Alternatives considered

### A. Mutable document plus undo stack

*Rejected.* Fails branching, comparison, crash recovery and incremental sync.
Retrofitting any of those later is a rewrite of every edit path.

### B. Append-only operation log with materialised state (chosen)

Every edit is an immutable, timestamped domain operation appended to a log. The
in-memory project state is a fold over that log. Snapshots are periodic
materialisations that make loading fast.

### C. Conflict-free replicated data type throughout

Model the entire project as a CRDT so that concurrent edits always merge
automatically.

*Rejected as a blanket strategy.* CRDTs guarantee that concurrent edits converge
to *a* valid state, not to a *musically sensible* one. Two DJs independently
adjusting the same transition would silently converge on a blend of both
intentions, producing a transition neither person designed. MP#24 explicitly
requires conflicts to be explained and forbids silently discarding work, so
automatic convergence is the wrong default here. CRDT techniques are still used
for the specific structures where commutativity is genuinely correct — the marker
set, tag sets, comment threads — but not for the timeline as a whole.

## Decision

Adopt **alternative B**.

### The log

The project is an append-only sequence of operations. Each operation carries an
identifier, the logical clock of the device that produced it, the causal
predecessor it was based on, an author, a timestamp and its payload. Operations
are pure data and are versioned; an operation type is never redefined, only
superseded by a new type.

State is derived by folding the log. Because the fold is a pure function, the
same log produces the same project on every platform and in every version that
understands its operation types — which is what makes export deterministic and
makes preview and render agree.

### Snapshots, versions and branches

A snapshot is a materialised state at a log position, stored for speed. A named
version is a user-visible label on a log position. A branch is a fork of the log.
Restore, compare, duplicate, branch and merge from MP#9 are all operations on log
positions rather than separate features, so they cost almost nothing once the log
exists.

### Undo and redo

Undo moves the materialisation point backwards and appends a compensating
operation, so undo itself is part of the history and survives a restart. Nothing
is ever removed from the log.

### Synchronisation

Because the log is append-only and each operation is small, synchronisation ships
only operations the other side has not seen. A project's entire editing history
is typically kilobytes, so it synchronises in seconds on a poor connection in a
venue — while the audio it references, which is gigabytes, transfers selectively
in the background. This asymmetry is what makes MP#24's cross-platform sessions
and future real-time collaboration practical.

Operations that commute merge automatically. Operations that do not are surfaced
for review with both intentions shown, per MP#24.

### What is stored, and where

- **Operation log, library index, analysis references, preferences**: an embedded
  transactional database on device.
- **Media files**: referenced by stable identity, never copied into the
  application's storage and never modified. MP#29 makes the user the owner of
  their files; the application is a reader.
- **Derived artefacts** (waveform tiles, analysis results, stems, rendered
  previews): a content-addressed cache keyed by source fingerprint plus the
  version of the algorithm that produced them, so an algorithm upgrade
  invalidates exactly what it should and nothing more.

### Why an embedded SQL database rather than the platform's object graph

Core Data and SwiftData are Apple-only. Choosing either would place the schema,
the migrations and the query logic inside the platform layer, contradicting
ADR-0001 and forcing a reimplementation for Windows, Android and Web. An embedded
SQL engine gives the same schema, the same migrations and the same queries on
every platform, runs in the Rust core, and is testable on Linux runners. It also
gives real transactions, which the crash-recovery requirements of MP#7 and MP#22
depend on.

### Sharing a project does not share the audio

When a project is shared, the operation log is shared. The referenced media is
not automatically replicated to another account. A collaborator opens the project
and sees the complete timeline; tracks they do not own appear as missing, with the
option to supply them from their own library. Replicating media on share would
turn synchronisation into a distribution channel, contradicting MP#15's ethics
principle and MP#29's copyright principles.

## Consequences

### Positive

- Undo, redo, snapshots, named versions, branching, comparison, crash recovery
  and incremental sync are all consequences of one mechanism rather than seven
  features.
- Preview and export execute the same materialised graph, so they cannot
  disagree.
- The AI can propose a change as a set of operations that the user previews and
  then accepts or rejects, which is exactly the approval model MP#19 requires.

### Negative

- Every edit must be expressible as an operation; ad-hoc mutation of project
  state is not permitted anywhere in the codebase.
- Logs grow. Compaction is required, and compaction must never discard a named
  version or a reachable branch point.

### Risks and mitigations

| Risk | Probability | Impact | Mitigation |
|------|-------------|--------|------------|
| Log growth degrades load time | High | Medium | Periodic snapshots; load reads the latest snapshot plus the tail; compaction preserves all named versions and branch points |
| Operation schema evolution breaks old projects | Medium | Critical | Operation types are versioned and never redefined; every release carries a migration test that loads fixtures from all prior versions |
| Merge surfaces too many conflicts to be usable | Medium | Medium | Commutativity is analysed per operation type; the common cases (different tracks, different lanes, different time ranges) merge automatically |

## Success criteria

1. Killing the process at any point during editing loses no committed operation.
2. A project with ten thousand operations loads within the budget recorded in
   the Master Technical Specification.
3. Restoring, branching and comparing versions require no code outside the log
   module.
4. A project edited offline on two devices reconciles with automatic merges for
   non-overlapping edits and explicit review for overlapping ones.
