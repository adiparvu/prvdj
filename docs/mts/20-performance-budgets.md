# 20. Performance Budgets

Master Prompt #31 requires measurable targets per module; Master Prompt #15
requires budgets to be reviewed continuously rather than set once. A target that
is not measured is an aspiration.

## Product-level budgets

From Master Prompt #4, #12 and #13.

| Measure | Budget | Source | Enforcement |
|---------|--------|--------|-------------|
| Application launch | < 2 s | MP#4 | Phase 7 gate |
| Project open | < 1 s | MP#4 | Phase 7 gate |
| Library search | Instant, updating as typed | MP#4 | Phase 1 benchmark |
| Timeline interaction | 60–120 FPS | MP#13 | Phase 2 benchmark |
| Waveform first paint | Progressive, usable immediately | MS#003 | Phase 1 benchmark |
| Export | Cancellable and recoverable | MP#13 | Phase 2 |

## Realtime budgets

The ones that cannot be missed. A miss here is audible on stage.

| Measure | Budget | Status |
|---------|--------|--------|
| Heap allocations per render block | **Exactly 0** | **Enforced on every pull request**, debug and release |
| Destructors run on the audio thread | **Exactly 0** | **Enforced on every pull request** |
| Transport drift over six hours | **Exactly 0 samples** | **Enforced** |
| Locks taken on the audio thread | 0 | Structural — the audio thread holds no lock |
| DSP load, worst-case graph | ≤ 60 % of one core at 128 frames, 48 kHz | Phase 1, once processors exist |
| Command queue overflow | 0 in normal operation | Counted and surfaced; an overflow is a defect |

## Library scale

From Module Specification #001.

| Library size | Requirement | Status |
|--------------|-------------|--------|
| 10 000 tracks | Responsive search, non-blocking indexing | Phase 1 |
| 50 000 tracks | Same | Phase 1 benchmark |
| 100 000 tracks | Same | Phase 7 |

## Rule

A budget without a measurement is not a budget. Each row above either names the
gate enforcing it today or the phase in which that gate arrives. Rows never
silently graduate from "planned" to "assumed".
