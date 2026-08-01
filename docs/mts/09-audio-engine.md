# 9. Audio Engine

Governed by Master Prompt #3A, #18 and Module Specification #002.
Contract fixed by [ADR-0002](../adr/0002-realtime-audio-core.md).

## Status

| Component | Status |
|-----------|--------|
| Musical time and transport clock (`prv-time`) | **Completed, verified** |
| Realtime primitives (`prv-rt`) | **Completed, verified** |
| Transport state machine | Not started — Phase 1 |
| DSP graph, mixer, effects, master bus | Not started — Phase 1 and 2 |
| Decoding, device management, audio host | Not started — Phase 1 (Apple layer) |

## What is already guaranteed

- **One authoritative clock.** No other type in the core can construct musical
  position, so no subsystem can invent its own.
- **Zero drift.** Position is an exact integer frame count; musical position is
  computed from it with integer arithmetic, never accumulated in floating point.
  Verified over a simulated six-hour session.
- **Tempo changes re-anchor** rather than reinterpreting the timeline, so the bar
  a listener is hearing does not move underneath them.
- **A sample-rate change preserves musical position**, which is what allows
  recovery from a device change without stopping playback (Module Specification
  #002).
- **The render path allocates nothing.** Proven by a gate that runs on every
  pull request, in both debug and release configurations.
