# 10. AI Orchestrator

Governed by Master Prompt #6, #19 and #25.
Architecture fixed by [ADR-0006](../adr/0006-ai-decision-architecture.md).
Status: **Not started** — Phase 3.

## Decisions already fixed

- **Musical decisions are computed, not generated.** A deterministic planner
  under hard constraints decides; a language model translates intent inward and
  evidence outward. It never chooses what plays next.
- **The eight agents of Master Prompt #6 are orchestration roles**, not separate
  intelligences. Each registers with declared capabilities, inputs, outputs,
  latency and confidence characteristics.
- **The orchestrator performs no heavy work itself** (Master Prompt #19). It
  plans, schedules, collects, resolves conflicts and returns.
- **Confidence has one central definition.** The mapping from numeric confidence
  to the five labels of Master Prompt #25 is defined once and calibrated. No
  screen chooses its own thresholds; a label the user cannot trust is worse than
  no label.
- **During live playback, non-critical AI work is suspended** (Master Prompt
  #19). Audio performance is protected first, always.
- **A failed agent cannot touch audio.** The boundary is structural, not a
  matter of error handling.
