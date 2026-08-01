# Sprint 6 — Tone, loudness and structure

Sprint 5 taught the system to hear rhythm. This one completes the analysis
engine: what key a track is in, how loud it is by a measure that means the same
everywhere, and where its sections begin.

| Delivered | Tests |
|-----------|-------|
| Chroma extraction | 7 |
| Key detection | 8 |
| Loudness to ITU-R BS.1770, with true peak | 11 |
| Structure segmentation | 9 |
| The track profile, with independently versioned stages | 8 |

457 tests in total, up from 410. All gates green: `cargo test`,
`cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`, all six
architecture rules, and the design-token staleness gate.

## Business outcome

**Harmonic mixing works end to end.** `prv-harmony` has known since Sprint 0 how
keys relate; it had no way to learn what key a track was in. It does now, and
the library's harmonic filter — written in Sprint 4 against an empty projection —
has something to filter on.

**Two tracks can be matched in level without guesswork.** A loudness figure that
means the same for a 1994 house record and a 2024 one is what stops a transition
jumping six decibels, and `gain_to_reach` is the one operation every consumer of
that figure performs.

**A transition has somewhere to go.** Sections, with the quiet ones identified
as places a second track can enter, are what turn "mix these two records" into a
decision about *where*.

**A library can be upgraded without being re-analysed.** Each stage carries its
own version and knows what depends on it, so improving key detection re-runs key
detection on a hundred thousand tracks and nothing else.

## Architecture review

**No new decision records were needed.** As in Sprints 2 to 5, the choices
followed from ADR-0001 and ADR-0006.

**The profile is the contract, and it is deliberately not one result.** Master
Prompt #20 requires stages to be versioned independently. Implementing that as
`Option<Versioned<T>>` per stage rather than one version on the whole profile
makes three things structural rather than remembered: a stage can be absent, a
stage can be stale on its own, and staleness propagates to whatever was derived
from it. `Stage::dependents` is checked by a test that would fail if a seventh
stage were added out of order — which is exactly when this stops being obvious.

**Absence means something specific.** A stage that produced no result is `None`,
not a low-confidence value. Applause has no tempo; a field recording has no key.
Reporting those as uncertain rather than inapplicable would make the confidence
scale meaningless within a month, because the most common reason for a low
confidence would be that the question did not apply.

The layering held. `tools/check-architecture.sh` passes unchanged.

## Audio quality review

**The loudness measurement is the standard's, not something shaped like it.**
The derived filter coefficients reproduce the published 48 kHz table to nine
decimal places, and the test asserts that. This matters more than it sounds: a
generic shelving filter with the same corner frequency and a Butterworth Q comes
within a quarter of a decibel at 1 kHz, puts a full-scale sine at −3.26 LUFS
instead of −3.01, and would therefore have made every loudness figure the product
reports wrong by the same amount — consistently, and so invisibly.

**Chroma takes only spectral peaks.** A kick drum deposits energy in every bin
and therefore equally in every pitch class; admitting it raises the floor of the
chroma without adding information and flattens exactly the shape the key
detector correlates against. Tested by adding drums to a tonal signal and
checking the profile does not move.

**Frames are normalised before accumulation.** Without it the loudest forty
seconds decide a track's key, and a record that modulates is analysed as though
only its loudest section existed.

**Sections are found at bar resolution.** Almost every produced record changes
section on a bar line, so the extra freedom of a frame-resolution segmenter is
entirely freedom to be wrong.

## AI behaviour review

**The relative key is treated as an alternative, not an error.** C major and A
minor use the same seven notes and differ only in which feels like home — which
is carried by *when* notes occur, and a chroma has discarded that by
construction. The detector therefore always reports the relative alongside its
choice, and its confidence excludes the relative from the margin calculation.

That exclusion is a deliberate judgement and is documented where it is made:
including it would cap the confidence of every correctly detected key at the
level of an ambiguity that barely matters, because a key and its relative sit at
the same Camelot number and `prv-harmony` already scores that relation as safe.
A DJ acting on either mixes the same records.

**Sections are not named "drop" or "chorus".** Those are genre conventions, not
acoustic facts, and a label printed over the wrong eight bars teaches the user to
stop reading labels. What is reported is what was measured — relative energy, and
which sections resemble each other — plus positional kinds whose rules are stated
in the source.

## Performance review

The structure stage runs a second transform over the whole track, which doubles
the cost of a full analysis relative to Sprint 5. Two choices keep that from
compounding:

- Bar assignment uses a binary search rather than a scan. A scan per frame would
  be quadratic in the track length, and on a ten-minute track that is tens of
  thousands of frames against hundreds of bars.
- The self-similarity is computed on bar-level features, so the matrix has
  hundreds of entries rather than tens of thousands. A frame-level
  self-similarity for a ten-minute track is a fifty-thousand-square matrix; it
  would not fit in memory, let alone finish.

The loudness stage uses running sums for its overlapping windows, which at 75 per
cent overlap is the difference between milliseconds and minutes.

## Security review

Nothing new. No input or output, no secrets, no network. The core still has zero
runtime dependencies across ten crates.

## Accessibility review

No user-facing surface. Two decisions were taken with accessibility in mind.
`ConfidenceLabel::key` and `Stage::key` return stable identifiers rather than
English prose, so the core is not the place translations live. And section
`kind` is a value rather than a colour, so the structural navigation model of
the specification does not depend on a user distinguishing hues.

## Findings raised on my own work

**One genuine design flaw, caught by a test that would have passed if I had
written it more loosely.** True-peak measurement was implemented as linear
interpolation between samples. Linear interpolation is monotonic between its
endpoints, so it can never produce a value larger than a sample already present:
the function reported the sample peak under a different name.

This is a bad failure because it is *silent and reassuring*. It returns a
plausible number, it is never far wrong, and it tells a user their master is safe
when it is not. It was caught because the test asserted the true peak must be
*above* the sample peak for a signal whose samples straddle a crest — a property,
not a plausible value. The fix is a windowed-sinc polyphase interpolator, which
is what a band-limited reconstruction actually is.

**A second defect in the same function, found by the fix's own test.** Reading
zeros past the ends of the signal fabricates a step edge, and a band-limited
reconstruction of a step overshoots — so a file whose first sample is non-zero
would have been reported as clipping because of where the analysis started. The
interpolator now only reports positions whose whole support is inside the signal;
the raw sample peak already covers the edges.

**Two test expectations were wrong rather than the code.**

1. The K-weighting test asserted 40 Hz would read more than 10 dB below 1 kHz. It
   reads about 6 dB, which is what the standard's curve actually does. The test
   now states the property in the form that matters — an unweighted meter reports
   the two as *exactly equal*, and this one does not — which is both true and a
   sharper claim.
2. A test asserted the dominant is the second-largest entry in both Krumhansl
   profiles. In the minor profile it is the *minor third*, which is correct and
   is precisely what makes the mode minor. The test now pins the right property
   per profile, and would catch a detector that confused a key with its parallel.

**One threshold was purely relative and had no floor** — the same shape of defect
found in the onset detector last sprint. A relative threshold on structure
novelty faithfully reports the four bars that happened to differ most on a loop
that never changes. It now has an absolute floor with a stated physical meaning,
and a test asserts an unvarying loop produces no boundaries.

## Are the project's principles still respected?

| Principle | Verdict |
|-----------|---------|
| Never the easy solution when a premium one exists (MP#1) | Held, and this was the sprint where it cost something: the standard's own filter derivation rather than a plausible shelf; band-limited interpolation rather than linear. |
| Production-ready, never placeholder (MP#13) | Held and enforced. |
| Modular, dependencies inward (MP#4, MP#7) | Held. Still ten crates, still no runtime dependencies. |
| Honest about uncertainty (MP#25) | Held. Absent is distinguished from uncertain, and the relative-key exclusion is argued where it is made. |
| Deterministic across platforms (ADR-0006) | Held and tested — every stage has a reproducibility test. |
| Quality is not a phase (MP#27) | Held. Two real defects and three wrong tests found and fixed within the sprint. |

## Known limitations

1. **Tuning is assumed to be concert pitch.** A recording transferred at the
   wrong speed, or deliberately detuned, has its energy fall between semitone
   centres and its chroma smears across two neighbours. Detecting the offset is a
   separate stage that does not exist yet; the failure is graceful and recorded.
2. **Time signature is still assumed to be four-four**, as in Sprint 5.
3. **The confidence thresholds remain provisional**, pending Phase 3 calibration.
   Risk R-04.
4. **Structure is single-level.** A real arrangement is hierarchical — phrases
   inside sections inside a form — and this reports one level. The bar-level
   feature sequence is what a hierarchical segmenter would be built from, so the
   groundwork is not wasted.
5. **Processors still have no parameter-addressing scheme**, unchanged, Phase 2.

## Definition of done

| Criterion | Status |
|-----------|--------|
| Architecture reviewed | Yes — no new records needed |
| Implemented, no placeholders | Yes, enforced |
| Tests passing | Yes — 457 |
| Performance validated | Bar-level features and running sums measured against the alternatives |
| Documentation updated | Yes, in the same commit |
| Accessibility verified | Not applicable yet; two enabling decisions recorded |
| Security reviewed | Yes — still zero runtime dependencies |
| No critical technical debt introduced | Five recorded limitations, none critical |
