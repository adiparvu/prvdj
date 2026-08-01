# ADR-0005: Plugin isolation versus realtime latency

- Status:        Accepted
- Date:          2026-08-01
- Review date:   2027-02-01
- Deciders:      Platform Engineering Lead, Lead Audio DSP, Security Engineering
- Related modules: plugin-manager, dsp-graph, effects, marketplace
- Source requirements: MP#18 (Audio Thread), MP#23 (Plugin SDK — *sandboxed*),
  MP#26 (Plugin Security), MP#29 (Marketplace)

## Context

MP#23 requires plugins to be sandboxed, permission-based, digitally signed and
revocable, and states that a crashing plugin must never stop playback. It also
requires audio plugins that support realtime processing and declare their
latency. MP#18 forbids allocation, locking, blocking and syscalls on the audio
thread.

These two requirements are in direct tension. Genuine isolation normally means
running untrusted code in a separate process, and crossing a process boundary
inside a 2.7 millisecond audio budget introduces exactly the synchronisation and
scheduling risk MP#18 forbids.

## Problem

How can third-party audio code be untrusted and sandboxed, and still participate
in the realtime signal path?

## Alternatives considered

### A. Everything out of process

*Rejected for realtime audio.* Round-trip through shared memory with
cross-process wakeups adds latency and, worse, adds a scheduling dependency on a
process we do not control. Correct for non-realtime plugins; unacceptable for the
signal path.

### B. Everything in process as native code

*Rejected.* A native plugin sharing the address space can corrupt engine state,
allocate on the audio thread, block, or crash the host. This is the failure mode
professional audio users have suffered for thirty years, and MP#23 explicitly
forbids it.

### C. Tiered isolation by plugin class (chosen)

Match the isolation mechanism to what the plugin actually needs to touch.

## Decision

Three tiers, assigned by capability rather than by vendor.

### Tier 1 — Non-realtime plugins, out of process

Analysis engines, visualisation data producers, importers, exporters, metadata
providers, AI providers, cloud providers, workflow extensions.

These run in a separate, sandboxed child process with only the permissions their
manifest declares. They cannot see the filesystem, the library database or any
data outside what the host passes them. A crash affects only that process; the
supervisor restarts or disables it, records diagnostics and notifies the user
where it matters. Latency is irrelevant here because none of it is on the audio
path.

### Tier 2 — Realtime audio plugins, WebAssembly sandbox in process

Third-party effects and processors from the marketplace.

They are compiled to WebAssembly and executed in process, on the audio thread,
by a runtime configured for realtime use: modules are compiled ahead of time and
pre-instantiated before they become reachable from the callback, linear memory is
preallocated from a pool, and no host functions are importable except pure
mathematical helpers. WebAssembly gives what is needed and nothing more —
memory safety by construction, no syscalls, no ambient authority, and
deterministic execution.

Two protections cover the remaining failure modes. Execution is metered, so a
plugin that fails to return within its declared budget is interrupted rather than
allowed to overrun the block. A per-block watchdog bypasses any plugin that
exceeds its budget repeatedly, replaces it with a pass-through, and reports the
bypass — the music continues, exactly as MP#23 requires.

The cost is a modest execution overhead relative to native code, which is
accepted: MP#15 ranks reliability above performance, and a plugin that cannot
crash the host during a live set is worth more than one that runs marginally
faster.

### Tier 3 — First-party and certified native processors

The built-in effect set and, later, processors that pass a certification
programme, compiled natively and bound by the same contract as the core itself
(ADR-0002). Certification means the code is reviewed against the audio-thread
contract and covered by allocation and timing tests. This tier exists because
some processing genuinely warrants native performance; it is deliberately narrow
and is not open by default.

### Common rules

Every plugin, in every tier, declares its permissions, its processing latency and
its resource expectations in a signed manifest. Latency is compensated by the
graph. Permissions are approved by the user and revocable. Unsigned code is never
loaded without explicit user approval, per MP#26.

## Consequences

### Positive

- Untrusted audio code cannot corrupt engine state, allocate on the audio thread,
  or take the process down.
- The marketplace of MP#29 can accept third-party effects without each submission
  being a stability risk to live performers.
- Tier 1 covers the majority of plugin categories with the strongest isolation
  available, at no latency cost.

### Negative

- Plugin authors targeting the realtime tier must compile to WebAssembly, which
  constrains their language and library choices.
- Three tiers is more machinery than one, and the tier boundaries must be
  documented clearly enough that authors know where they land.

### Risks and mitigations

| Risk | Probability | Impact | Mitigation |
|------|-------------|--------|------------|
| WebAssembly runtime allocates during execution | Medium | Critical | Pooled preallocated instances; ahead-of-time compilation; the same allocation test that gates the core covers a hosted plugin |
| Metering overhead measurably raises DSP load | Medium | Medium | Budget measured per release; plugin count per chain bounded by measured headroom, surfaced in diagnostics |
| Authors reject the WebAssembly constraint | Medium | Medium | Certified native tier exists as a path for serious partners; software development kit ships with a working example and toolchain |

## Success criteria

1. A plugin that deliberately loops forever is interrupted and bypassed without
   an audible dropout.
2. A plugin that deliberately crashes does not stop playback.
3. A plugin cannot read any file, socket or library record not granted by its
   manifest, verified by an isolation test suite.
4. Declared plugin latency is compensated exactly, verified by a null test.
