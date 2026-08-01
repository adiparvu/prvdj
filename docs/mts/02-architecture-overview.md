# 2. Architecture Overview

## Layering

The system follows the dependency rule of MP#4: dependencies point inward only,
and business logic never depends on the interface.

```
┌─────────────────────────────────────────────────────────────────────┐
│ PRESENTATION            SwiftUI views, component library, tokens    │
│                         holds no business rules                     │
├─────────────────────────────────────────────────────────────────────┤
│ INFRASTRUCTURE          file access, CoreAudio host, decoders,      │
│ (PRVKit, Swift)         keychain, network, sync transport, FFI      │
├══════════════════════ language / ports boundary ═══════════════════┤
│ APPLICATION             use cases, orchestration, scheduling        │
│ (prv-core, Rust)        pure — no I/O, no OS calls                  │
├─────────────────────────────────────────────────────────────────────┤
│ DOMAIN                  musical model, DSP graph, analysis,         │
│ (prv-core, Rust)        planner, project document, invariants       │
└─────────────────────────────────────────────────────────────────────┘
```

The language boundary sits exactly on the ports seam. Infrastructure implements
traits the application declares; the application never names a concrete adapter.
This is why the core builds and tests on Linux with no Apple SDK present, and why
the Phase-2 platforms of MP#8 need new adapters rather than new logic.

Rationale, alternatives and trade-offs: [ADR-0001](../adr/0001-platform-and-language-strategy.md).

## Execution domains

Four domains with different rules. Confusing them is the most common way audio
software fails, so they are named explicitly and enforced.

| Domain | Owns | Rules |
|--------|------|-------|
| **Realtime** | audio callback, transport clock, DSP graph | no allocation, no locks, no syscalls, no panics, bounded time (ADR-0002) |
| **Interactive** | user interface, gestures, immediate feedback | never blocks; must reach the next frame; reads published snapshots |
| **Background** | analysis, waveform generation, import, export, sync, AI | preemptible, resumable, suspended during live performance (MP#19) |
| **Isolated** | out-of-process plugins, untrusted code | no ambient authority; failure contained (ADR-0005) |

Data crosses between domains through three mechanisms and no others: a wait-free
command queue into the realtime domain, a triple-buffered snapshot out of it, and
durable job queues for background work.

## Communication

**Inside the core**, modules communicate through explicit interfaces and a domain
event stream. The event names of MP#7 and MP#10 — `TrackImported`,
`TrackAnalysisCompleted`, `WaveformGenerated`, `MixGenerated`, `ExportCompleted`,
`CloudSyncCompleted` — are the contract. Every event carries an identifier, a
timestamp, a producer, a payload, a schema version and a **correlation
identifier**, so a single user action can be traced across a dozen subsystems
(MP#10).

**Across the language boundary**, calls are coarse-grained and versioned. Audio
and waveform data crosses as pointers into shared preallocated buffers, never
copied per frame. Swift bindings are generated from the Rust definitions; they
are never hand-written, because hand-written bindings drift.

**To the cloud**, the same service contracts are exposed over the network. One
set of contracts, two transports — in-process calls for the local path, network
calls for sync and collaboration. This resolves the tension between MP#10's
service independence and MP#18's latency requirements: services are independent
in contract, not in address space.

## The project document

A project is an append-only log of immutable operations. State is a fold over the
log. Snapshots make loading fast; named versions are labels on log positions;
branches are forks.

This is the mechanism behind non-destructive editing (MP#3C), version history at
every stage (MP#7), immutable snapshots with branch and merge (MP#9), timeline
history (MP#21) and incremental offline-first sync (MP#24). One mechanism, six
requirements. See [ADR-0003](../adr/0003-project-document-and-persistence.md).

Because the fold is pure, live preview and offline export execute the same
materialised graph. They cannot disagree — which is why what the user hears while
editing is what the export contains.

## Intelligence

Musical decisions are computed by a deterministic planner under hard constraints;
a language model translates intent inward and evidence outward. The specialised
agents of MP#6 are orchestration roles over that core, registered with declared
capabilities, latencies and confidence characteristics.

Consequences that matter architecturally: generation is reproducible, so it can
be regression-tested (MP#27); explanations render the actual decision arithmetic,
so they are true (MP#3B); the safety rules are constraints rather than
preferences, so they cannot be violated; and the whole path runs on-device, so
disabling cloud AI costs fluency and not capability (MP#26).

See [ADR-0006](../adr/0006-ai-decision-architecture.md).

## Analysis is precomputed

MP#20 scores transition regions at import time rather than at mix time. This
turns mix planning from a search over every pair of tracks at every possible
point into a search over a small precomputed index. It is the single decision
that makes a sixty-minute set computable in seconds and lets recommendations
stream progressively as MP#12 requires.

Every analysis stage is versioned independently, so improving key detection
re-runs key detection alone across the library — not the whole pipeline.

## What enforces this

Architecture is enforced by the build, not by review:

- `prv-core` cannot import a user-interface framework, because none exists in
  its language.
- Crate boundaries within the core encode the layer rule; a dependency that
  points outward fails to compile.
- Allocation on the realtime path fails a test.
- Public interface changes fail a contract test unless versioned.
- Undocumented public items fail the documentation gate.

MP#28 requires architecture rules to be validated in continuous integration. The
above is how that requirement is met concretely.
