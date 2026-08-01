# PRV AI DJ Studio

An AI-powered DJ and music performance platform. You describe the set you want
in ordinary language; the system builds a professional mix from music you
already own, explains every decision it made, and lets you change any of them.

You do not need to understand beatmatching or harmonic mixing. You describe
intent. The system understands music.

## Where to start reading

| If you want to know | Read |
|---------------------|------|
| What this is and why it is built this way | [Executive summary](docs/mts/01-executive-summary.md) |
| How the system fits together | [Architecture overview](docs/mts/02-architecture-overview.md) |
| Why a decision was made the way it was | [Decision records](docs/adr/README.md) |
| What actually exists today | [Implementation status](docs/mts/implementation-status.md) |
| Everything | [Master Technical Specification](docs/mts/README.md) |

The Master Technical Specification is the single engineering reference. If the
implementation and that document ever disagree, the document is updated in the
same change that caused the disagreement.

## Three commitments that shape everything else

**Musical decisions are computed, not generated.** A deterministic planner
operating under hard musical constraints decides track order, transition points
and effect placement. A language model translates your words into goals and turns
the planner's evidence into prose — it never decides what plays next. This is why
the system can explain itself truthfully, why the same request produces the same
set, why it cannot violate a musical safety rule, and why it works with no
network. ([ADR-0006](docs/adr/0006-ai-decision-architecture.md))

**The audio path cannot be interrupted by anything else in the product.** The
realtime engine allocates nothing, locks nothing and waits for nothing. A failed
AI agent, a hung sync, a crashed plugin or a stalled interface cannot stop the
music or move the transport clock. This is verified on every pull request, not
asserted. ([ADR-0002](docs/adr/0002-realtime-audio-core.md))

**Nothing you do is destructive.** The project is an append-only log of
operations. Undo, branching, named versions, comparison, crash recovery and
incremental cloud sync all fall out of that one mechanism.
([ADR-0003](docs/adr/0003-project-document-and-persistence.md))

## Repository layout

```
core/          Portable core in Rust — domain and application layers.
               Pure: no I/O, no OS calls. Builds and tests on any platform.
  prv-time/      Musical time, tempo, the authoritative transport clock
  prv-rt/        Realtime primitives: wait-free queues, snapshot publication,
                 parameter smoothing, preallocated buffers
  prv-harmony/   Keys, the Camelot wheel, harmonic compatibility
  prv-transport/ Playback state, loops, slip, and the transport that drives the clock
  prv-dsp/       The signal path: processors, the chain, gain, EQ, filter
  prv-waveform/  Waveform tiles, resolution ladder, viewport rendering
  prv-project/   The project as an append-only operation log

apple/         Swift application and presentation layers
design/tokens/ The single source of truth for every visual value
tools/         Build tooling and architecture gates
docs/          Master Technical Specification and decision records
```

Dependencies point inward only. `core` cannot import a user-interface framework,
because none exists in its language — the layering rule is enforced by the build
rather than by review.

## Building

The portable core needs only a Rust toolchain.

```sh
cd core
cargo test              # 310 tests
cargo clippy --all-targets -- -D warnings
```

Architecture rules and design tokens:

```sh
./tools/check-architecture.sh
cargo run --manifest-path tools/tokengen/Cargo.toml -- --check
```

The Apple application requires macOS with Xcode. Its continuous-integration job
is defined and currently disabled; until a macOS runner executes it, no
Apple-framework code is reported as verified. See
[known limitations](docs/mts/17-known-limitations.md).

## Contributing

Read [the coding standards](docs/engineering/coding-standards.md) first. The
short version: every change leaves the codebase cleaner than it was, nothing is
reported as working before it has been measured, and a feature is not complete
until its architecture, tests, documentation, accessibility, performance,
security and error handling are complete too.
