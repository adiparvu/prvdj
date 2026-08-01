# 15. Testing

Governed by Master Prompt #27. Quality is not a phase.

## Current coverage

| Suite | Count | What it establishes |
|-------|-------|---------------------|
| `prv-time` | 73 | Sample-accurate musical time; zero drift over six simulated hours; multi-segment tempo maps that stay monotonic; beat-grid snapping that never crosses the position it was given |
| `prv-transport` | 38 | All 405 state-event-intent combinations defined; a device lost mid-seek recovers rather than stopping; loops keep phase even when shorter than a block; slip returns to the arrangement |
| `prv-rt` | 32 | Wait-free queue correctness under concurrency; no torn snapshots; click-free parameter ramps; buffer bounds |
| `prv-rt` integration | 2 | **The render path performs zero heap activity across 150 000 blocks**, plus a positive control proving the harness would notice if it did |
| `prv-dsp` | 46 | Filter responses measured, not derived; the equaliser is flat at unity to within 0.6 dB; a kill is better than −30 dB; the filter stays stable under a fast sweep |
| `prv-dsp` integration | 2 | **A full channel strip under constant control movement allocates nothing** across 25 000 blocks, and chain dispatch adds nothing |
| `prv-harmony` | 30 | All 24 published Camelot positions reproduced against an external chart; safety ordering; no key is ever stranded without a safe destination |
| `prv-waveform` | 38 | Tiles report true extremes; chunk size does not change the result; aggregation preserves transients; level selection never upscales |
| `prv-project` | 49 | Two devices converge whatever order operations arrive in; concurrent edits to the same thing are reported and to different things are not; undo restores state exactly and can itself be undone |
| `prv-library` | 46 | Typing more narrows rather than widens; a renamed track stops matching its old name; removal keeps ratings, tags and collections; duplicates are reported once, strongest signal first |
| `tokengen` | 4 | Token generation and schema validation |
| **Total** | **360** | all passing, all gated on every pull request |

## Practices that matter more than the count

**Positive controls.** The allocation test is preceded by a test that deliberately
allocates and asserts the counter noticed. Without it, a harness that had
silently stopped counting would let the real assertion pass for the wrong reason.

**Ground truth, not self-consistency.** The Camelot tests check all twenty-four
published wheel positions against an external chart, not against the formula that
produced them. The filter tests measure a magnitude response by driving the
filter with a sine, rather than re-deriving the transfer function the
implementation was written from. A test that only confirms the implementation
agrees with itself proves nothing.

**Measure the thing, not a proxy for it.** The first version of the filter tests
measured peak amplitude and reported attenuation that did not exist: near Nyquist
a sampled sine rarely has a sample at its crest. They now measure energy, against
the energy actually presented rather than against the theoretical value for a
sine, because a measurement window rarely spans a whole number of cycles. Both
corrections came from a failing test that was right about the measurement and
wrong about the filter.

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
