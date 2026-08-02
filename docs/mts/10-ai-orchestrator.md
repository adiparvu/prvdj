# 10. AI Orchestrator

Governed by Master Prompt #6, #19 and #25.
Architecture fixed by [ADR-0006](../adr/0006-ai-decision-architecture.md).
Status: **In progress** — the orchestration is built and verified (`prv-ai`,
Sprint 20); the agent implementations that sit behind it arrive with the layers
that can reach a network and a model.

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

## What is built

`prv-ai` is the coordination and deliberately not the intelligence. Four modules:
`intent` bounds what arrives from a model, `agent` says what the system can do
and where, `task` works out the order, `run` records what happened.

Three properties carry the weight:

- **No musical decision is made on a server.** `AgentKind::decides_musically` and
  `AgentKind::location` are checked against each other over every agent, so one
  added later that both decided musically and ran remotely would fail the build.
  The reason is not privacy, though it is good for privacy: a musical decision
  has to be reproducible, explainable and the same this evening as it was this
  afternoon, and a request to a model is none of those.
- **Losing the cloud costs fluency, never capability.** With no agreement of any
  kind, a whole set-planning plan still schedules — interpret, analyse, search,
  plan, choose transitions, explain, deliver — entirely on the device. There is a
  test that withdraws everything and schedules it, because "graceful degradation"
  means nothing until something measures it.
- **A task never runs on an input that was not produced.** When a step fails,
  everything downstream is skipped and *said to be* skipped, transitively, and
  each skip names the step that caused it. The failure mode this exists against
  is not a visible error but a plausible wrong answer built on a measurement that
  never arrived.

Two further decisions are worth recording. A task names a *capability*, not an
agent, and the agent is resolved at scheduling time — which is why the same plan
runs with every agreement granted and with none. And availability asks two
questions rather than one: has the user agreed, and can this machine do it.
Conflating them would tell someone on a modest laptop that they had withheld a
permission they never withheld.
