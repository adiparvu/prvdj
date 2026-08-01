# Sprint 0 — Foundation

Phase 0 of the plan in Master Prompt #30. Reported in the eleven-section format
required by Master Prompt #13.

## Sprint goal

Establish the foundation on which everything else is built: architecture decided
and recorded, repository structure enforced by the build rather than by
convention, the portable core started with real and verified code, the design
token system generating, and every quality gate operational.

## Business outcome

None user-facing, deliberately. Master Prompt #30 sequences AI behind a stable
timeline, playback and export because a planner evaluated against an unstable
foundation cannot be debugged. What this sprint buys is that the next eight
phases can be built without an architectural rewrite, and that nothing built on
top can silently break the two properties the product cannot survive losing:
audio that never stops, and work that is never lost.

## Architecture changes

Six decision records, each with alternatives considered and rejected:

| Record | Decision |
|--------|----------|
| ADR-0001 | Swift application over a portable Rust core; the language boundary sits on the ports seam |
| ADR-0002 | Wait-free command queue in, triple-buffered snapshots out; the audio-thread contract |
| ADR-0003 | Append-only operation log; undo, branching, versions and sync from one mechanism |
| ADR-0004 | Stem separation as a port, on-device by default; model selection deliberately deferred |
| ADR-0005 | Three-tier plugin isolation, resolving sandbox against realtime latency |
| ADR-0006 | Deterministic planner with a generative shell; musical decisions are computed |

## Files created

- `docs/adr/` — six records plus the index and format
- `docs/mts/` — Master Technical Specification, twenty sections plus module
  template, implementation status board and risk register
- `docs/engineering/coding-standards.md`
- `core/` — Rust workspace with three crates
- `design/tokens/tokens.json` and `tools/tokengen/`
- `tools/check-architecture.sh`
- `.github/workflows/ci.yml`, `core/deny.toml`

## Tests added

111 passing, plus one documentation example intentionally not executed.

| Suite | Count |
|-------|-------|
| `prv-time` | 43 |
| `prv-rt` | 32 |
| `prv-rt` realtime contract | 2 |
| `prv-harmony` | 30 |
| `tokengen` | 4 |

## Documentation updated

Everything listed above. Documentation and implementation landed in the same
changes, as Master Prompt #14 requires.

---

# Code review

Conducted against the ten-point checklist in Master Prompt #11 and the review
categories in Master Prompt #13.

## Architecture review

**Layering holds.** The core has no filesystem, network, process or environment
access, verified mechanically. The dependency rule of Master Prompt #4 is
enforced by the build rather than by review: `prv-core` cannot import a
user-interface framework because none exists in its language.

**Unsafe confinement holds.** Three blocks, all in `prv-rt`, each with a written
safety argument. Verified mechanically.

**One clock.** No type outside `prv-time` can construct musical position, so
Module Specification #002's single-clock rule cannot be violated by accident.

## Audio quality review

The central claim — that the render path allocates nothing — is measured, not
asserted: 150 000 blocks with zero allocations and zero destructors, in debug and
release. Transport drift over a simulated six-hour session is exactly zero
samples.

**One weakness, recorded.** The test harness advances a single shared smoother
per block and copies it per channel. A real mixer holds a smoother per channel.
The harness exercises the right *shape* of work, but it is a simplification and
should be replaced by the actual mixer once processors exist in Phase 1, rather
than left as the definitive proof.

## Security review

No secrets, no I/O, no network in anything written. Dependency policy in place
and gated. Nothing to review beyond that yet, and saying so is more useful than
manufacturing findings.

## Performance review

Budgets are recorded in section 20 with, for each, either the gate enforcing it
today or the phase in which that gate arrives. The realtime budgets are enforced
now. The DSP load budget cannot be measured until processors exist and is marked
accordingly rather than assumed.

## Accessibility review

No user-facing surface exists yet. Two decisions were nonetheless made now
because making them later would be far more expensive:

- Colour never carries meaning alone; every semantic colour is paired with an
  icon, label or texture in the component that uses it.
- Every motion token has a zero-duration reduced-motion form, so the reactive
  visuals of Master Prompt #2 degrade rather than being an exception to the
  accessibility contract.

The hardest accessibility problem in the product — VoiceOver over a continuous
timeline canvas — has its navigation model recorded in section 11 before the
timeline is built.

## AI behaviour review

Nothing executes yet. The architecture that makes the AI requirements satisfiable
is fixed in ADR-0006, and `prv-harmony` is its first concrete instance:
classification and weighting are separate functions, so recalibrating the weights
cannot change what the system *claims* about the music — only how strongly it
prefers one move to another. That separation is what keeps explanations truthful
across tuning.

## Findings raised on my own work

Three, none blocking, all recorded rather than quietly carried:

1. **`TransportClock::set_sample_rate` uses a temporary zeroing of the anchors**
   to reuse the conversion, then re-anchors. It is correct and tested, but the
   trick is not obvious to a reader. Refactor to a direct computation in
   Phase 1, when the multi-segment tempo map replaces this code anyway.

2. **Harmonic distance 3 is classified as clashing**, which sits at the strict
   end of accepted practice. A minor to F♯ minor shares four of seven scale
   degrees and is usable under a filtered transition. Because clashing is a
   *hard* constraint, being too strict costs the planner reachable, musically
   valid moves. Flagged for the Phase 3 calibration (risks R-03 and R-04); the
   test asserting no key is ever stranded is what currently bounds the damage.

3. **The realtime harness is not the real mixer.** See the audio quality review
   above. The gate is real; the workload it gates is a stand-in until Phase 1.

## Are the project's principles still respected?

| Principle | Verdict |
|-----------|---------|
| Never the easy solution when a premium one exists (MP#1) | Held. Two languages were chosen over one because the audio-thread contract must be provable, and that cost was paid deliberately. |
| Production-ready, never placeholder (MP#1 R10, MP#13) | Held, and enforced — placeholder markers fail the build. |
| Modular, dependencies inward (MP#4, MP#7) | Held, and enforced by the build. |
| Privacy by design (MP#1 R7, MP#26) | Held in the decisions made; nothing yet collects data. |
| Offline-first (MP#1 R8) | Held structurally — the planner and the whole core run on-device. |
| Quality is not a phase (MP#27) | Held. 111 tests, six architecture gates, two of which were verified by running them against deliberate violations. |
| Every decision recorded (MP#14, MP#31) | Held. Six records, each with rejected alternatives. |
| Nothing reported as working before measured (MP#13, MP#27) | Held, and made structural through the verified/authored distinction. |

## Known risks

Ten entries in the [risk register](../mts/risk-register.md). The one that most
shapes the next sprint is **R-01**: without a macOS runner, Apple-framework code
accumulates unverified. Acquiring one is the top Phase 1 dependency, and until
then the status board will keep saying *authored* rather than *verified*, however
inconvenient that is to read.

## Future work

Phase 1, ordered in [section 18](../mts/18-roadmap.md). Phase 1 is complete when
a person can import a folder of music and hear two tracks mixed — with nothing in
the path reported as working before it was measured.

## Definition of done

| Criterion | Status |
|-----------|--------|
| Architecture approved and recorded | Yes — six records |
| Code implemented, no placeholders | Yes — enforced |
| Tests passing | Yes — 111 |
| Performance validated | Realtime budgets enforced; others marked pending with a phase |
| Documentation complete | Yes — specification, standards, status, risks |
| Accessibility verified | Not applicable yet; two structural decisions taken early |
| Security reviewed | Yes |
| No critical technical debt introduced | Three findings recorded above, none critical |
