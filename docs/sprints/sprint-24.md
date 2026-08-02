# Sprint 24 — The master limiter

The last conspicuous gap in the signal path, and the one processor that may not
be approximately right.

| Delivered | Tests |
|-----------|-------|
| `prv-dsp::Limiter` — true-peak look-ahead limiting | 11 new; 867 in total |

All gates green: `cargo test`, `cargo clippy --all-targets -- -D warnings`,
`cargo fmt --check`, `cargo doc`, eight architecture rules, the design-token
staleness gate, and the allocation gate — now with the limiter in the chain.

## Business outcome

**A master cannot leave above its ceiling.** Not usually, not on the material
that was tested: the guarantee is structural, and the test asserts it to within a
few units in the last place of single precision rather than within a comfortable
percentage.

**It limits the peak a converter will actually produce.** `prv-export` reports a
true-peak figure and every delivery target sets a true-peak ceiling; it would be
strange to report a number the master chain had not been limiting against. The
detector runs on the same four-times reconstruction `prv-analysis` measures with,
so the number that gates the export and the number the limiter honours come from
the same place.

**It does it without a click.** A limiter that drops its gain the instant it sees
a peak puts a discontinuity into the *gain*, which is audible even though the
sample it protects is now under the ceiling.

## Architecture review

**No new decision record.** The limiter is a `Processor` like any other and obeys
ADR-0002's contract: it allocates in `prepare` and never in `process`, reports
its latency, and clears its tail on `reset`.

**The gain is a straight line, chosen by looking at the whole window.** At every
sample it takes the shallowest line that reaches every requirement in the window
on time. The invariant — that the gain applied to a sample is at or below what
that sample needs — holds by induction: any constraint still in the window at a
shorter distance produces a slope at least as steep, so the line never falls
behind.

**One ring length for the audio and the requirements.** The first version had two
— look-ahead for one, look-ahead plus detector delay for the other — and the
eight-sample difference filed every requirement in the wrong slot. Using one
length removes the class of bug rather than the instance.

**Deliberately not configurable beyond its ceiling and release.** A limiter with
an attack control is a limiter that can be set to fail, and the setting that
matters — how loud is too loud — is one the delivery target already states.

## Findings raised on my own work

Three, and all three were the same shape: an off-by-N in time that the tests
caught and reading would not have.

**The look-ahead window was indexed backwards.** The requirement written this
iteration is due *furthest* away, not soonest. Indexed the other way, the limiter
treated the newest requirement as immediately due and attacked far too abruptly.

**The detector's own delay was unaccounted for.** A symmetric filter reconstructs
the centre of its window, so the estimate arriving when sample *n* is read
describes sample *n − 8*. Filing it against sample *n* moved every requirement
eight samples late — a hole of exactly eight samples in the protection, which
would have shipped as "it clips sometimes".

**The gain is applied in the same iteration it is computed**, so the sample
leaving the delay is at distance *one*, not two. Getting that wrong put the whole
window one sample out.

Each of these produced an overshoot of a fraction of a decibel: enough to fail a
strict test, invisible to a listener, and exactly the kind of defect that turns
into a support thread about a mastering chain "sometimes" clipping. The lesson
worth keeping is that the strict assertion is what made them findable — a test
with a 1% tolerance would have passed all three.

## Audio quality review

**Transparent below the ceiling.** Verified sample for sample, not statistically:
material under the ceiling comes out bit-identical after the delay. A limiter
that touches what it did not need to is one nobody leaves switched on.

**The release is fifty milliseconds and linear in amplitude.** Short enough that
the level recovers between kicks rather than ducking a whole bar behind one
transient; long enough not to modulate the bass, which is the audible cost of a
fast release on dance material where the loudest thing in the mix is also the
lowest. Linear in amplitude rather than in decibels because that is what keeps
the recovery from being heard as a swell on sustained material.

**The delay is reported.** Look-ahead plus detector delay, both counted, so the
graph compensates the whole of it. An uncompensated master delay moves everything
against everything else.

## Performance review

Measured, in release, through the existing allocation gate: a four-processor
chain — gain, three-band equaliser, filter and limiter — renders about forty
million frames per second per core. At 48 kHz stereo that is well under half a
percent of one core for the whole strip.

The limiter is the most expensive part of it: a seventeen-tap filter at three
phases per channel for the detector, plus a window scan per sample. Both are
bounded and neither allocates. If it ever needs to be cheaper, the window scan is
the part to attack — but there is no evidence it needs to be.

**Zero allocations over 175 000 blocks**, with the limiter in the chain. That is
the measurement that matters, and it is the reason the limiter was added to the
existing gate rather than given a new one.

## Are the project's principles still respected?

| Principle | Verdict |
|-----------|---------|
| Never the easy solution when a premium one exists (MP#1) | Held. True-peak detection rather than sample peak; a straight line rather than a smoothed step. |
| Reliability above performance (MP#15) | Held. The detector costs what it costs. |
| No allocation on the audio thread (MP#18, ADR-0002) | Held, and measured with the limiter in the chain. |
| Silence remains silent (MP#15) | Held. `reset` clears both rings. |
| Production-ready, never placeholder (MP#13) | Held. |
| Quality is not a phase (MP#27) | Held. 867 tests; three timing defects found and fixed within the sprint by assertions that refused a comfortable tolerance. |

## Known limitations

1. **No realtime loudness meter.** `prv-analysis` measures loudness to BS.1770
   offline, and `prv-export` reports it. A momentary and short-term meter on the
   signal path is a separate piece of work: the K-weighting filters are cheap,
   the sliding windows are not free, and it wants a design rather than an
   addition.
2. **The detector estimates true peak; it does not reconstruct it exactly.**
   Seventeen taps at four times is the published measurement geometry, and a
   longer filter would be marginally more accurate and materially more
   expensive. The estimate is never *below* the sample peak, so the guarantee
   holds regardless.
3. **No dither.** `prv-export` decides whether dither is needed; nothing applies
   it. That belongs with the encoder, which is outside the core.
4. **No master metering of gain reduction over time.** The current reduction is
   reported; a history for a meter's ballistics is the interface's to keep.
