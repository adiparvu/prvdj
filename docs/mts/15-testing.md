# 15. Testing

Governed by Master Prompt #27. Quality is not a phase.

## Current coverage

| Suite | Count | What it establishes |
|-------|-------|---------------------|
| `prv-time` | 43 | Sample-accurate musical time; zero drift over six simulated hours; exact tick round-trips across the musical range; tempo and sample-rate changes preserve musical position |
| `prv-rt` | 32 | Wait-free queue correctness under concurrency; no torn snapshots; click-free parameter ramps; buffer bounds |
| `prv-rt` integration | 2 | **The render path performs zero heap activity across 150 000 blocks**, plus a positive control proving the harness would notice if it did |
| `prv-harmony` | 30 | All 24 published Camelot positions reproduced; relation classification; safety ordering; no key is ever stranded without a safe destination |
| `tokengen` | 4 | Token generation and schema validation |
| **Total** | **111** | all passing, all gated on every pull request |

## Practices that matter more than the count

**Positive controls.** The allocation test is preceded by a test that deliberately
allocates and asserts the counter noticed. Without it, a harness that had
silently stopped counting would let the real assertion pass for the wrong reason.

**Ground truth, not self-consistency.** The Camelot tests check all twenty-four
published wheel positions against an external chart, not against the formula that
produced them. A test that only confirms the implementation agrees with itself
proves nothing.

**Properties, not examples.** Drift is tested by advancing a hundred thousand
blocks and comparing against a single seek. Harmonic safety is tested by
exhausting all 576 key pairs and asserting the ordering holds and no key is
stranded.

**Tests that state the consequence.** Failure messages name what breaks in the
product, not what differed numerically — "a single allocation in a 2.7 ms
callback is an audible dropout" rather than "expected 0, got 1".

## Categories still to come

Master Prompt #27 requires eight categories. Static analysis, unit, integration
and performance-regression testing are in place for the core. Contract, system,
interface, end-to-end and manual validation arrive with the modules they cover;
accessibility regression blocks release from the first shipped screen.
