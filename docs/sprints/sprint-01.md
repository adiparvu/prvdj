# Sprint 1 — Core Experience, first slice

Phase 1 of the plan in Master Prompt #30. Reported in the eleven-section format
required by Master Prompt #13.

## Sprint goal

Build the layer between the foundation and anything a user can hear: musical time
that follows a real recording, a transport that behaves predictably when things
go wrong, and a signal path that can actually process audio without ever
allocating.

## Business outcome

None user-facing yet, and one thing that matters more than a feature would: the
audio-thread guarantee now covers the code a performer's audio actually runs
through. Sprint 0 proved that the primitives allocate nothing. That was
necessary and not sufficient — nobody's music passes through a queue, it passes
through an equaliser. The gate now covers a full channel strip with its controls
moving.

## Architecture changes

No new decision records. Every choice here follows from ADR-0001 and ADR-0002,
which is the point of having recorded them.

Two design decisions worth noting, both recorded in the code:

- **The tempo map is deliberately not on the audio thread.** Multiple tempo
  regions need a growable structure, and a growable structure in the render path
  is forbidden. The map is a domain object; the audio thread carries one segment
  at a time and receives changes as scheduled commands. This keeps the clock
  `Copy` and allocation-free while satisfying Master Prompt #3A's requirement for
  multiple tempo regions.
- **Intent is a public value, not a private boolean.** Module Specification #002
  forbids hidden state. Remembering "the user wanted this playing" in a private
  flag would have been exactly that, so the transport carries `PlaybackIntent`
  alongside `PlaybackState`, and both are inspectable.

## Files created

- `core/prv-time/` — `conversion.rs`, `tempo_map.rs`, `beat_grid.rs`
- `core/prv-transport/` — state machine, loop regions, transport
- `core/prv-dsp/` — processor contract, chain, biquad, three-band equaliser,
  filter, gain, and an allocation gate over the whole strip
- `docs/mts/traceability.md`

## Files modified

`prv-time/clock.rs` (shared conversion extracted; the Sprint 0 review finding
about `set_sample_rate` resolved), the Master Technical Specification sections
for the audio engine, testing, deployment, limitations, roadmap and status.

## Tests added

227 total, up from 111.

| Suite | Count |
|-------|-------|
| `prv-time` | 73 |
| `prv-transport` | 38 |
| `prv-rt` | 34 |
| `prv-dsp` | 48 |
| `prv-harmony` | 30 |
| `tokengen` | 4 |

## Documentation updated

In the same commit as the code, as Master Prompt #14 requires.

---

# Code review

## Architecture review

**The layering held without effort**, which is the useful signal. Adding two
crates that touch audio required no exception to the purity rule, no new unsafe,
and no change to the dependency direction. `tools/check-architecture.sh` passes
unchanged.

**The tempo map forced a real decision** rather than an arbitrary one, and the
decision fell out of ADR-0002 rather than being invented: allocating structures
belong off the audio thread, so the map lives in the domain and the clock
receives commands. Had the constraint not been written down in Sprint 0, this is
exactly the point at which it would have been quietly broken.

## Audio quality review

**Measured, not assumed.** The equaliser's flatness, the depth of its kill, the
filter's slope and its stability under fast modulation are all driven with real
signals and measured. Nothing here is verified by re-deriving the mathematics the
implementation was written from.

**Three findings came from the tests, and all three were real:**

1. The first measurement used peak amplitude and reported 1.4 dB of attenuation
   that did not exist — near Nyquist a sampled sine rarely has a sample at its
   crest. Now measured as energy.
2. The second version compared against the theoretical value for a sine and
   failed at 50 Hz, because a measurement window rarely spans a whole number of
   cycles. Now compared against the energy actually presented.
3. Directional snapping could return a position *before* the one it was given,
   because ticks are coarser than frames and the conversion rounds. A loop whose
   end snapped backwards past its own start would have been silently empty. The
   result is now verified in frames, and the test exhausts a whole beat rather
   than sampling four offsets — the failure only appears within a few frames of a
   boundary, so sampling would have missed it.

The third is the one worth dwelling on. It was a real defect in shipped-quality
code, found because the test asserted a property rather than an example.

## Security review

Nothing new to review: no I/O, no secrets, no network, no new dependencies. The
core still has zero runtime dependencies.

## Performance review

The allocation and drift budgets are enforced and met. The DSP load budget still
cannot be measured honestly — a benchmark harness is Phase 7 work and the
processor set is not yet complete — and it remains marked as pending with a phase
rather than assumed.

One cost is now visible and accepted: the equaliser runs nine second-order
sections per channel in double precision. That is more than a shelving equaliser
would cost, and it is what buys a kill that is actually a kill and a response
that is actually flat. It will be measured against the budget in Phase 7.

## Accessibility review

No user-facing surface. The token decisions from Sprint 0 stand unchanged.

## AI behaviour review

`prv-harmony` unchanged this sprint. The calibration item from Sprint 0 remains
open and is tracked as risks R-03 and R-04.

## Findings raised on my own work

Two, both recorded rather than carried quietly:

1. **Processors have no parameter-addressing scheme.** Each exposes typed setters,
   which is clear to call and cannot express automation, a MIDI mapping or a
   plugin parameter. Master Prompt #21 needs the first and Master Prompt #23 the
   third. Recorded as limitation 3 and scheduled for Phase 2 alongside the
   automation lane, deliberately rather than improvised now: a parameter model
   designed around one processor's needs would have to be redone when the second
   arrived.

2. **The filter is twelve decibels per octave.** That is the classic DJ filter
   character and the one that stays musical when swept fast, but some performers
   expect a steeper sweep. Cascading a second stage would provide it without
   changing the control mapping. Documented at the point of use rather than left
   for someone to discover.

The Sprint 0 finding about `TransportClock::set_sample_rate` is **resolved**: the
temporary-zeroing trick is gone, replaced by a direct computation through the
shared conversion module.

## Are the project's principles still respected?

| Principle | Verdict |
|-----------|---------|
| Never the easy solution when a premium one exists (MP#1) | Held. A shelving equaliser would have been a third of the work and could not kill a band; the crossover with phase compensation is what the instrument actually needs. |
| Production-ready, never placeholder (MP#13) | Held — and enforced. One placeholder was written during this sprint, in a test helper, and removed rather than shipped. |
| Modular, dependencies inward (MP#4, MP#7) | Held, enforced, and unchanged by two new crates. |
| Silence remains silent (MP#15) | Held and tested, including denormal flushing so a fade-out cannot cause a dropout. |
| No clicks (MP#18) | Held and tested: every continuous parameter is ramped and every ramp lands exactly. |
| Quality is not a phase (MP#27) | Held. 227 tests, all gates green, three real defects found by tests during the sprint. |
| Nothing reported as working before measured (MP#13, MP#27) | Held. |

## Known risks

The register is unchanged. **R-01** — no macOS runner — remains the one that most
shapes the next sprint, and now carries more weight: the audio host is the next
piece, and it cannot be verified here.

## Future work

Phase 1 continues: waveform tile generation, the library index and import
pipeline, the project operation log, then the Apple audio host and the first
interface surfaces.

## Definition of done

| Criterion | Status |
|-----------|--------|
| Architecture reviewed | Yes — no new records needed, which is itself the finding |
| Implemented, no placeholders | Yes, enforced |
| Tests passing | Yes — 227 |
| Performance validated | Allocation and drift budgets enforced; DSP load marked pending with a phase |
| Documentation updated | Yes, in the same commit |
| Accessibility verified | Not applicable — no user-facing surface |
| Security reviewed | Yes |
| No critical technical debt introduced | Two findings recorded above, neither critical |
