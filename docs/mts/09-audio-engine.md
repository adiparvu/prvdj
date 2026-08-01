# 9. Audio Engine

Governed by Master Prompt #3A, #18 and Module Specification #002.
Contract fixed by [ADR-0002](../adr/0002-realtime-audio-core.md).

## Status

| Component | Status |
|-----------|--------|
| Musical time and transport clock (`prv-time`) | **Completed, verified** |
| Realtime primitives (`prv-rt`) | **Completed, verified** |
| Multi-segment tempo map and beat grid (`prv-time`) | **Completed, verified** |
| Transport state machine, loops, slip (`prv-transport`) | **Completed, verified** |
| Processor contract, chain, gain, EQ, filter (`prv-dsp`) | **Completed, verified** |
| Mixer, master bus, metering | Not started — Phase 2 |
| Time stretching, key lock | Not started — Phase 2 |
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
  pull request, in both debug and release configurations — now covering a full
  channel strip with its controls moving, not only the primitives.
- **Every playback state transition is defined.** All 405 combinations of state,
  event and intent produce either a state or an explicit rejection. A device lost
  during a seek recovers; it does not stop the music.
- **No control change produces a click.** Every continuous parameter is ramped,
  and the ramp lands exactly on its target rather than approaching it.
- **The equaliser is flat at unity and its kill is a kill.** Measured, not
  assumed: within 0.6 dB across the spectrum with the bands at centre, better
  than −30 dB of rejection with a band killed.
