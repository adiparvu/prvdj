# Implementation Status

Statuses are those defined by MP#31. Two additional qualifiers are used because
the build environment makes the distinction material:

- **Verified** — compiled and tested in continuous integration.
- **Authored** — written against specification, not yet compiled on a machine
  with the required SDK.

Code that is authored is never reported as working. This is a direct requirement
of MP#13 (*no placeholder implementations*) and MP#27 (*no feature is complete
without verification*).

_Last updated: Sprint 0._

## Sprint 0 — Foundation

| Item | Status | Qualifier | Notes |
|------|--------|-----------|-------|
| ADR-0001 platform & language strategy | Completed | — | |
| ADR-0002 realtime audio core | Completed | — | |
| ADR-0003 project document & persistence | Completed | — | |
| ADR-0004 stem separation | Completed | — | Model evaluation deferred to Phase 3 by design |
| ADR-0005 plugin isolation | Completed | — | |
| ADR-0006 AI decision architecture | Completed | — | |
| Master Technical Specification v0.1 | Completed | — | Sections filled as modules enter design |
| Repository structure | Completed | Verified | Builds from clean checkout |
| `prv-time` musical time & transport clock | Completed | Verified | Unit tested, zero-drift property tested |
| `prv-rt` realtime primitives | Completed | Verified | Allocation test, SPSC queue, triple buffer, parameter smoothing |
| `prv-harmony` harmonic model | Completed | Verified | Camelot wheel, compatibility scoring, property tested |
| Design token source & generator | Completed | Verified | Generates Swift and JSON from one source |
| Continuous integration pipeline | Completed | — | Linux jobs run; macOS jobs declared, not yet exercised |

## Not started

Every module in the [module index](03-module-index.md) not listed above is *Not
Started*. They are sequenced by the phase plan in [18-roadmap.md](18-roadmap.md).

## Environment limitation

The available continuous-integration environment is Linux. It compiles and tests
Rust and pure Swift. It cannot compile SwiftUI, AVAudioEngine or CoreAudio, which
require the Apple SDKs.

Consequences, stated plainly:

- Everything in `core/` is verified here.
- Pure-Swift packages are verified here.
- Apple-framework code will be marked *Authored* until a macOS runner compiles
  and tests it. The CI workflow defines those jobs; they are inert until such a
  runner is available.

This is recorded again in [17-known-limitations.md](17-known-limitations.md) so
that it is impossible to miss.
