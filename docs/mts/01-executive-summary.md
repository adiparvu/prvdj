# 1. Executive Summary

## What this product is

PRV AI DJ Studio lets a person describe the set they want in ordinary language
and receive a professional DJ mix built from music they already own — then edit
every decision it made, or perform live with it.

The person does not need to understand beatmatching, harmonic mixing or audio
engineering. They describe intent. The system understands music.

## What makes it different

Three things, and each one is an architectural commitment rather than a feature.

**The musical decisions are computed, not generated.** Track order, transition
points, effect placement and energy shape come from a deterministic planner
operating under hard musical constraints. A language model translates the user's
words into goals and turns the planner's evidence into prose — it never decides
what plays next. This is why the system can explain itself truthfully, why the
same request produces the same set, why it cannot violate a musical safety rule,
and why it works with no network. See ADR-0006.

**The audio path cannot be interrupted by anything else in the product.** The
realtime engine allocates nothing, locks nothing and waits for nothing. A failed
AI agent, a hung cloud sync, a crashed plugin or a stalled interface cannot stop
the music or move the transport clock. See ADR-0002 and ADR-0005.

**Nothing the user does is destructive.** The project is an append-only log of
operations. Undo, branching, named versions, comparison, crash recovery and
incremental cloud sync all fall out of that one mechanism rather than being seven
separate features layered on a mutable document. See ADR-0003.

## Shape of the system

```
   SwiftUI presentation          PRVUI      Apple platforms
   platform adapters, FFI        PRVKit     Apple platforms
   ─────────────────────────────────────────────────────────
   domain + application          prv-core   every platform
   (pure, no I/O, no OS)         Rust
```

`prv-core` holds everything that constitutes the product's value: the transport
clock and DSP graph, audio analysis, the mix planner and transition scorer, the
project document, and the library index. It performs no input or output; the
outside world reaches it through ports. It is therefore identically usable from
the Apple applications shipping in Phase 1 and from the Windows, Web and Android
targets named in MP#8, without a rewrite.

The Swift layers own what only they can: an Apple-native interface built from a
single design-token system and component library, and the platform services the
core requires.

## Current state

Sprint 0 (Phase 0, *Foundation*). Architecture decided and recorded; portable
core scaffolded with its first verified modules; documentation spine established;
quality gates defined.

Nothing user-facing exists yet, by design. MP#30 sequences the work so that the
timeline, playback and export are stable before AI is layered on top, because a
planner evaluated against an unstable foundation cannot be debugged.

Verified-versus-authored status for every module is tracked in
[implementation-status.md](implementation-status.md). That distinction is
maintained honestly: code that has not been compiled and tested is never reported
as working.
