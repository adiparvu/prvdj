# 4. Dependencies

Master Prompt #31 requires every dependency to record a reason, an owner, a risk
and a **replacement strategy**. The replacement strategy is the part that
matters: it is what stops an abandoned third-party crate from becoming an
existential problem four years from now.

## Portable core

**The core has no third-party runtime dependencies.** Twenty-two crates, zero.
Two of them depend on other core crates — `prv-plugin` on `prv-security`, so that
authorisation has one decision point rather than two, and `prv-ai` on `prv-mix`,
`prv-time` and `prv-security`, because an orchestrator that could not name the
planner's own goal would have to describe it in strings. Nothing in the core
depends on anything outside it.

This is a deliberate position, not an accident of being early. Every crate on the
realtime path is a crate whose allocation behaviour, panic behaviour and
threading behaviour become our problem, and ADR-0002 makes those properties
things we must be able to prove rather than assume. The wait-free structures in
`prv-rt` are hand-written for that reason: they are small enough to audit in full
and their safety arguments are stated in the source.

Additions to the core require an entry in the table below and a review.

| Crate | Scope | Reason | Owner | Risk | Replacement strategy |
|-------|-------|--------|-------|------|----------------------|
| `proptest` | dev only | Property tests over musical time arithmetic | Audio DSP | Low — test-only, never linked into a shipping binary | Hand-written generators; the properties themselves are the asset, not the framework |

## Tooling

| Crate | Scope | Reason | Owner | Risk | Replacement strategy |
|-------|-------|--------|-------|------|----------------------|
| `serde_json` | `tools/tokengen` | Parses the design token source | Design Systems | Low — build tooling, not shipped | Any JSON parser; the generator's contract is the token schema, which is stable and documented |

## Toolchains

| Toolchain | Version | Reason | Risk | Replacement strategy |
|-----------|---------|--------|------|----------------------|
| Rust | stable, ≥ 1.82 | Portable core (ADR-0001) | Low | Pinned in `core/rust-toolchain.toml`; the core uses no nightly features |
| Swift | 6.1 | Apple application layer | Low | Platform requirement, not a choice |

## Platform frameworks

Named here for completeness; they are obligations of shipping on Apple platforms
rather than dependencies we selected.

| Framework | Used by | Replacement strategy |
|-----------|---------|----------------------|
| SwiftUI | presentation | None — replacing it means replacing the Apple client, which ADR-0001 accepts as the cost of a native experience |
| AVFoundation / AudioToolbox | audio host, decoders | Decoding is behind a port; the portable decoder adapter covers non-Apple platforms and can cover Apple platforms if needed |
| CoreAudio | audio host | Platform-mandated for low-latency output |

## Rules

1. A dependency is added by a change to this document and the manifest in the
   same commit. A manifest entry without a row here fails review.
2. Nothing enters the core that allocates, spawns threads or performs I/O on our
   behalf without that behaviour being documented and bounded.
3. Licence and advisory checking is automated (`cargo deny`, gated in continuous
   integration per Master Prompt #26).
4. Multiple versions of the same crate are a warning, not an error, but each
   occurrence is expected to be resolved rather than accumulated.
