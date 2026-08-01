# Architecture Decision Records

This directory is the permanent decision log for PRV AI DJ Studio, mandated by
Master Prompt #14 (*Execution Protocol* — AI Decision Log) and Master Prompt #31
(*Master Technical Specification* — Decision Records).

## Rules

1. **Records are immutable.** A decision that changes does not edit its record.
   It creates a new record that supersedes the old one. The old record stays,
   with its status changed to `Superseded by ADR-XXXX`. The history of reasoning
   is as valuable as the current conclusion.
2. **Every significant architectural decision requires a record.** "Significant"
   means: it is expensive to reverse, it constrains other modules, or a future
   engineer would reasonably ask "why is it done this way?"
3. **Every record must state what was rejected and why.** A record that lists
   only the chosen option is a description, not a decision.
4. **Every record carries a review date.** Decisions are made with the
   information available at the time. The review date is when we re-check
   whether that information still holds.

## Format

Every record uses exactly this structure. No custom formats (Master Prompt #31,
*Consistency is mandatory*).

```
# ADR-XXXX: <Title>

- Status:        Proposed | Accepted | Superseded by ADR-YYYY | Deprecated
- Date:          YYYY-MM-DD
- Review date:   YYYY-MM-DD
- Deciders:      <roles>
- Related modules: <module ids>
- Source requirements: <Master Prompt / Module Spec references>

## Context
## Problem
## Constraints
## Alternatives considered
## Decision
## Consequences
### Positive
### Negative
### Risks and mitigations
## Success criteria
## Notes
```

## Index

| ID | Title | Status | Review date |
|----|-------|--------|-------------|
| [0001](0001-platform-and-language-strategy.md) | Platform and language strategy | Accepted | 2026-11-01 |
| [0002](0002-realtime-audio-core.md) | Realtime audio core and the audio thread contract | Accepted | 2026-11-01 |
| [0003](0003-project-document-and-persistence.md) | Project document, event sourcing and persistence | Accepted | 2027-02-01 |
| [0004](0004-stem-separation.md) | Stem separation strategy | Accepted | 2026-11-01 |
| [0005](0005-plugin-isolation.md) | Plugin isolation versus realtime latency | Accepted | 2027-02-01 |
| [0006](0006-ai-decision-architecture.md) | AI decision architecture: deterministic planner, generative shell | Accepted | 2026-11-01 |
| [0007](0007-the-project-crate-is-the-document-model.md) | The project crate is the document model | Accepted | 2027-02-01 |
