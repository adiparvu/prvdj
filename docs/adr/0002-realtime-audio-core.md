# ADR-0002: Realtime audio core and the audio thread contract

- Status:        Accepted
- Date:          2026-08-01
- Review date:   2026-11-01
- Deciders:      Lead Audio DSP, Chief Architect
- Related modules: playback-transport, dsp-graph, mixer, effects, master-bus, recording
- Source requirements: MP#3A, MP#15 (*silence should remain silent*), MP#18 (Audio Thread),
  MP#22 (Emergency Mode — *preserve transport clock*), Module Spec #002, Module Spec #003

## Context

MP#18 states the audio-thread rules without exception: never allocate memory
during audio callbacks, never perform network requests, never block, never wait
for locks, never perform heavy inference. Only deterministic audio processing is
allowed. Module Spec #002 adds that state transitions must be explicit with no
hidden transitions, that one authoritative clock exists and no module creates its
own, and that recovery from device loss should happen *without stopping
playback*. MP#22 requires that in Emergency Mode the transport clock survives
whatever else has failed.

These are not stylistic preferences. A single allocation inside a 128-frame
callback at 48 kHz — a window of 2.7 milliseconds — is an audible dropout in
front of an audience.

## Problem

How is the realtime path structured so that (a) the audio thread provably obeys
the contract, (b) the rest of the application can still change what is playing,
and (c) a failure anywhere else in the process cannot stop the music?

## Constraints

- The callback is owned by the operating system, not by us.
- Everything the callback touches must already exist when it runs.
- Nothing the callback does may take unbounded time.
- The user changes parameters continuously while audio is running, so the design
  must move data in both directions without blocking either side.

## Alternatives considered

### A. Shared mutable state protected by locks

The UI and the audio thread share deck state guarded by a mutex.

*Rejected.* Priority inversion is not a theoretical concern here: if a lower
priority thread holds the lock when the audio thread wants it, the audio thread
waits and the buffer is missed. This violates MP#18 directly.

### B. Lock-free command queue in, published snapshots out (chosen)

The audio thread is a single consumer of a bounded, wait-free command queue and
a single producer of state snapshots. All memory is preallocated before it is
reachable from the callback.

### C. Audio engine in a separate process

A dedicated audio server process with shared-memory ring buffers, as some
professional hosts do for plugin isolation.

*Rejected for v1, deliberately kept possible.* It offers the strongest possible
answer to "preserve the transport clock", because a crash in the application
process leaves audio running. But it adds inter-process latency on the control
path, considerable complexity in device and session management, and it conflicts
with the iOS process model, where Phase 1 also ships. Since `prv-core` has no
dependency on the application shell, moving the realtime path into its own
process later is an infrastructure change, not an architectural rewrite. The
review date on this record exists chiefly to re-examine this choice.

## Decision

Adopt **alternative B**, with the following contract.

### The realtime entry point

The core exposes exactly one function reachable from the audio callback:

```
process(&mut Engine, ctx: &RenderContext, out: &mut [f32]) -> RenderResult
```

Its contract is:

1. **No heap allocation.** No `Box`, `Vec` growth, `String`, collection insert,
   or any call that may allocate.
2. **No locking and no waiting.** No mutex, no condvar, no channel that blocks,
   no atomic spin without bound.
3. **No system calls.** No file, socket, logging or time-of-day syscall. Time
   comes from the sample counter and the host-provided timestamp.
4. **No panic.** No `unwrap`, `expect`, `panic!`, slicing or indexing that can
   fail, integer division by a possibly-zero divisor, or arithmetic that can
   overflow in release.
5. **Bounded execution.** Work per call is a function of frame count and active
   node count only; it never depends on library size, project length or history
   depth.

### How work gets in

Non-realtime threads send POD commands through a bounded single-producer,
single-consumer wait-free ring buffer. Anything that requires allocation is
prepared entirely off-thread and handed over as a ready-made object; the audio
thread only swaps a pointer. The displaced object is pushed to a return queue and
dropped by a non-realtime janitor thread, so no destructor ever runs on the audio
thread. Loading a track, building an effect chain and rebuilding the graph all
follow this pattern.

If the command queue is full, the producer — never the consumer — waits. A full
queue is recorded as a diagnostic, because it means the control path is
overloaded.

### How state gets out

The audio thread publishes an immutable snapshot of transport and meter state
into a triple buffer on every block. Writing is wait-free for the audio thread;
readers always observe a complete, self-consistent snapshot and never block the
writer. The UI, the waveform renderer and the diagnostics view all read from
here. This is the mechanism by which Module Spec #003 keeps waveforms visually
synchronised with playback without ever touching the audio thread.

### Parameters and smoothing

No continuous parameter is applied as a step. Gain, EQ, filter cutoff, dry/wet,
crossfader position and tempo each carry a smoothing time constant, and the
engine advances ramps per block. This is what MP#18 means by "avoid clicks and
zipper noise", and what MP#15 means by "processing should never surprise the
user".

Discrete changes that cannot be smoothed — a decoder swap, a graph topology
change — are scheduled to occur at a block boundary and, where musically
meaningful, at a beat or bar boundary supplied by the transport clock.

### The clock

A single `TransportClock` is owned by the audio thread and advanced by the exact
number of frames rendered. It is the only source of musical position in the
system. Sample position is integral and never derived from wall-clock time, so
it cannot drift. Everything else — timeline, waveform, automation, recording,
lighting later — reads the published snapshot. Module Spec #002's rule that no
module creates its own playback clock is enforced by the fact that no other type
in the core can construct one.

### Device changes

Device loss, sample-rate change and route change are handled by rebuilding the
output chain off-thread while the transport clock and deck state remain intact,
then swapping the prepared chain in at a block boundary. Playback position is
preserved across the swap. Where the hardware makes a gap unavoidable, the gap is
bounded and reported, and the transport does not reset.

### Failure containment

The realtime path is written to be panic-free rather than protected by unwinding,
because unwinding across the FFI boundary is undefined behaviour. Panic-freedom
is enforced by lints that reject the constructs listed above on any function
reachable from `process`, and by tests that exercise the edge cases those lints
cannot see.

## Consequences

### Positive

- The contract is mechanically checkable, so it survives staff turnover and
  deadline pressure — the conditions under which conventions normally fail.
- MP#22's "preserve transport clock" holds for every in-process failure: the
  audio thread shares no lock, no allocator path and no data structure with the
  AI, cloud, library or UI subsystems.
- Meter, waveform and timeline synchronisation come from one published snapshot,
  so they cannot disagree with each other.

### Negative

- Every feature that touches audio must be designed with its allocation and
  handoff strategy decided up front. This is slower to write and is the intended
  trade.
- The command and snapshot machinery is genuinely subtle code. It is isolated in
  one small module, documented, and covered by concurrency tests.

### Risks and mitigations

| Risk | Probability | Impact | Mitigation |
|------|-------------|--------|------------|
| An allocation slips onto the realtime path | Medium | Critical | Allocation-detecting allocator asserts zero allocations during render in tests; CI gate on every pull request |
| Priority inversion via an unexpected shared resource | Low | Critical | Audio thread touches only preallocated engine state and the two queues; reviewed as a checklist item on every audio change |
| Command queue overflow under heavy automation | Medium | Medium | Queue depth sized from measured worst case; overflow counted, surfaced in diagnostics, and treated as a defect |
| Unbounded work sneaks into a processor | Medium | High | Per-block work is benchmarked with a worst-case graph; benchmark regression fails CI |

## Success criteria

1. A sustained render test over millions of frames reports zero allocations,
   zero locks taken and zero panics.
2. A worst-case graph benchmark stays within the per-block CPU budget recorded
   in the Master Technical Specification.
3. Transport drift over a simulated six-hour session is exactly zero samples.
4. Simulated device loss and sample-rate change preserve playback position and
   do not reset the transport.
