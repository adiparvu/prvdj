# ADR-0006: AI decision architecture — deterministic planner, generative shell

- Status:        Accepted
- Date:          2026-08-01
- Review date:   2026-11-01
- Deciders:      Chief AI Architect, Chief Architect, Privacy Engineering
- Related modules: ai-orchestrator, mix-engine, transition-scoring, learning-engine, ai-ux
- Source requirements: MP#3B (Explainable AI, Safety Rules), MP#5 (Learning Engine),
  MP#6 (Multi-Agent), MP#19 (Orchestrator, *deterministic whenever possible*),
  MP#25 (Confidence Levels), MP#26 (AI Privacy), MP#27 (AI Validation)

## Context

MP#3B requires every recommendation to explain why, forbids black-box behaviour,
and lists seven hard safety rules the system must never violate — clashing keys,
abrupt volume jumps, unnatural tempo changes, vocal collisions, over-compression,
overused effects, transitions without musical logic. MP#19 requires the
orchestration layer to be deterministic wherever possible and forbids fabricated
confidence. MP#25 requires five calibrated confidence labels. MP#26 requires
cloud AI to be disableable. MP#27 requires AI validation tests, which presuppose
reproducible behaviour.

A language model asked to choose track order and transition points cannot satisfy
these. Its output is not reproducible, its stated confidence is not calibrated
against anything measurable, its reasoning is a plausible narrative rather than
the actual cause of its choice, and it cannot be guaranteed to respect a hard
constraint. It also cannot run offline on a phone at acceptable quality.

## Problem

How is musical intelligence structured so that it is explainable, reproducible,
testable, safe by construction, and functional with no network?

## Alternatives considered

### A. Language model as planner

Give the model the library and the prompt; ask for a set.

*Rejected.* Fails reproducibility (MP#19), fails calibrated confidence (MP#19,
MP#25), fails hard safety guarantees (MP#3B), fails offline operation (MP#26),
and makes MP#27's AI behaviour tests impossible to write meaningfully. Its
explanations would be post-hoc narration, which MP#3B's "never behave like a
black box" is precisely intended to prevent.

### B. Pure algorithmic system, no language model

*Rejected.* MP#3B requires understanding of natural-language intent — "make it
feel cinematic", "surprise the crowd" — and interpretation of intent rather than
keywords. A purely algorithmic system cannot do this, and MP#12 makes the natural
language prompt central to the product.

### C. Deterministic planner with a generative shell (chosen)

Separate *deciding* from *understanding* and *explaining*.

## Decision

Musical decisions are computed. Language is used to translate intent inward and
reasoning outward.

### The deterministic core

Inside `prv-core`, and therefore on every platform and offline:

- **Feature extraction** produces the track profile of MP#20 — tempo, key,
  structure, energy curve, spectral balance, vocal regions, loudness, and the
  pre-scored transition regions.
- **The constraint model** encodes MP#3B's safety rules as hard constraints. A
  candidate that violates one is not ranked low; it is not a candidate. This is
  why the safety rules are guarantees rather than tendencies.
- **The objective function** scores candidate sets on harmonic compatibility,
  energy-curve adherence, spectral and rhythmic compatibility at each junction,
  vocal collision risk, structural fit, and narrative shape. Weights come from
  the scenario (festival, club, wedding, sunset…) and from the user's learned
  profile from MP#5.
- **The search** explores the candidate space under the constraints and returns
  the best distinct solutions — the Version A / B / C of MP#3B, which are
  genuinely different optima rather than three samples from one model.
- **The evidence record.** Every score is retained with its components. The
  explanation shown to the user is a rendering of the actual arithmetic that
  produced the decision, not a story about it.

Because scoring is precomputed at import time (MP#20 scores transition regions
per track), the search operates over a small indexed candidate space rather than
all pairs of tracks at all positions. This is what makes a sixty-minute set
computable in seconds and lets recommendations stream progressively, as MP#12
requires.

### The generative shell

A language model, reached through the provider adapters of MP#6 and MP#19, does
two jobs and no others:

1. **Intent translation.** Convert a natural-language prompt into a structured
   goal — scenario, duration, energy curve shape, constraint adjustments,
   creativity level. The output is validated against a schema before it reaches
   the planner. An unparseable prompt produces a clarifying question, never a
   guess.
2. **Verbalisation.** Turn the evidence record into prose appropriate to the
   user's mode, translating jargon as MP#25 requires. It may not introduce any
   claim absent from the evidence record.

When cloud AI is disabled, intent translation falls back to an on-device parser
over a structured vocabulary, and verbalisation falls back to templates driven by
the same evidence record. The system loses fluency; it loses no capability. This
is how MP#26's "allow users to disable cloud-based AI features" is satisfied
without the product becoming a shell of itself.

### Confidence

Confidence is a property of measurement, not of phrasing. Each analysis stage
emits a numeric confidence with its result (MP#20). The mapping from those
numbers to MP#25's five labels — Very High, High, Medium, Low, Experimental — is
defined once, centrally, calibrated against held-out labelled data, and used
identically everywhere in the product. No screen chooses its own thresholds. A
label the user cannot trust is worse than no label.

### Agents

The specialised agents of MP#6 are orchestration roles, not separate
intelligences. Each is a bounded capability with declared inputs, outputs,
latency and confidence characteristics, registered as MP#19 describes. The Music
Analyst, Transition Specialist and Mix Architect invoke deterministic core
functions; the Creative Assistant uses the generative shell. Conflict resolution
compares evidence, not opinions.

## Consequences

### Positive

- Same inputs produce the same set, so MP#27's AI behaviour tests are meaningful
  and regressions are detectable.
- MP#3B's safety rules are structurally impossible to violate.
- Explanations are true by construction, because they render the actual decision
  arithmetic.
- Full musical capability offline, on every platform, satisfying MP#1 and MP#26.
- Model providers can be swapped or upgraded without changing any musical
  behaviour, because they do not produce musical behaviour.

### Negative

- The objective function and its weights are a substantial, ongoing research and
  tuning effort. There is no shortcut in which a model absorbs this work.
- Constraint modelling makes some creative-but-valid moves unreachable until the
  model is extended. The creativity slider of MP#3B mitigates this by relaxing
  soft constraints, never the hard safety ones.

### Risks and mitigations

| Risk | Probability | Impact | Mitigation |
|------|-------------|--------|------------|
| Objective function produces technically valid but musically dull sets | High | High | Evaluation set of reference sets judged by working DJs; the creativity dimension explicitly rewards distinctness; A/B/C returns genuinely distinct optima |
| Confidence labels drift out of calibration as analysis improves | Medium | High | Calibration is a versioned artefact with its own tests; changing an analysis stage requires recalibration before release |
| Generative shell introduces claims not supported by evidence | Medium | High | Verbalisation output is validated against the evidence record; unsupported claims fail the check and fall back to templates |
| Search cost grows with library size | Medium | Medium | Candidate space is precomputed and indexed at import; search is benchmarked against a hundred-thousand-track library |

## Success criteria

1. Given identical inputs and profile, generation is byte-identical across runs
   and platforms.
2. No generated set violates any MP#3B safety rule, verified by a property test
   over randomised libraries.
3. Every recommendation carries an evidence record from which its explanation is
   derived.
4. With networking disabled, a user can describe a set in natural language and
   receive three distinct professional results.
