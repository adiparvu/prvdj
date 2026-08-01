# Sprint 5 — Rhythm analysis

The first sprint where the system listens to music rather than organising it.
`prv-analysis` answers three questions about a track: what happened, how fast,
and where the beats are.

| Delivered | Tests |
|-----------|-------|
| Discrete Fourier transform, complex and real-input | 7 |
| Analysis windows and their measured sidelobe behaviour | 5 |
| Streaming short-time transform | 6 |
| Spectral-flux novelty curve and onset picking | 6 |
| Tempo estimation with octave alternatives | 9 |
| Beat and downbeat tracking, fitted grid | 11 |
| The central confidence scale | 6 |

410 tests in total, up from 360. All gates green: `cargo test`,
`cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`, all six
architecture rules, and the design-token staleness gate.

## Business outcome

**A track can now be beat-matched.** Everything a DJ application does to two
tracks at once — synchronise them, align a loop, place a cue on a downbeat, plan
a transition at a phrase boundary — needs a beat grid, and this sprint is where
one comes from. Without it the product is a file browser.

**It is fast enough to be invisible.** Five minutes of audio is analysed end to
end in 1.1 seconds on the container this repository builds in, which is about
270 times faster than playing it. Importing an evening's worth of music is
therefore something that finishes while the user is still choosing what to play.

**It says how sure it is, in one vocabulary.** Master Prompt #25's five
confidence labels now have exactly one definition in the codebase, and every
stage reports against it.

## Architecture review

**No new decision records were needed.** ADR-0001 (portable core, no runtime
dependencies) and ADR-0006 (deterministic core, confidence calibrated centrally)
between them determined most of what this sprint could do, including the two
choices that look like they should have needed a record.

**Writing the transform rather than depending on one** follows from ADR-0006
rather than from ADR-0001. The no-dependency rule alone might have been
negotiable for a well-maintained transform library. The determinism rule is not:
several of the good ones dispatch on runtime CPU features, so the same input can
produce results that differ in the last bits between an Apple Silicon Mac and an
x86 build machine. Inaudible in a spectrum; capable of changing *a decision
derived from* a spectrum, which is what makes a preview disagree with an export.

**Two named transforms instead of one configurable one.** `Stft::for_rhythm` and
`Stft::for_tone` exist because time resolution and frequency resolution trade
against each other and the two jobs want opposite ends of that trade. A single
constructor with default parameters would have made the choice belong to whoever
last edited the defaults rather than to the requirement.

**The spectrogram is never materialised.** Ten minutes at 44.1 kHz is about
fifty thousand frames of a thousand bins; storing it is 210 megabytes for one
track, and Module Specification #001 sizes the library at a hundred thousand of
them. So the transform pushes frames to a consumer and keeps none, and each
stage reduces as it goes. This also makes analysis compatible with a decoder
that streams, which is what a large import needs.

The layering held. `tools/check-architecture.sh` passes unchanged: the new crate
performs no input or output, contains no unsafe code, and depends only on
`prv-time` and `prv-harmony`.

## Audio quality review

Four decisions here are the difference between an analysis that works on test
signals and one that works on records.

**Logarithmic compression before differencing.** Flux is measured on
`log(1 + 1000·|X|)` rather than on magnitude, so it measures relative change. A
click track attenuated by 20 dB produces the same onsets as the original — tested
— which is what lets a quiet intro contribute to the tempo estimate as much as
the drop does.

**Half-wave rectification.** Only increases count. A note *ending* is not an
onset, and counting it would put a spurious peak after every event, which the
tempo estimator would read as double time.

**Local mean subtraction.** The curve is compared against its own recent average,
so a dense passage does not drown a sparse one.

**The window is short, on purpose.** Spectral flux fires when a transient
*enters* the analysis window rather than when the window is centred on it, so
every onset is detected early by up to half a window. That is a constant offset
applied to the whole grid — the one timing error a listener notices immediately.
The rhythm window is 1024 samples rather than 2048 specifically to halve it, and
the residual is a few milliseconds.

## AI behaviour review

**Confidence is now a type, and the mapping to words happens in one function.**
ADR-0006 required this; the reason it matters is that without it "high
confidence" drifts — the key detector's author calls 0.7 high because that is
good for key detection, the tempo estimator's author calls 0.9 high because tempo
is easier, and the user learns the label means nothing.

`Confidence::and_then` combines stages by taking the weaker, not the product and
not the average. The product is what independent probabilities would give, but a
beat grid derived from a tempo shares all of the tempo's uncertainty, so
multiplying would compound to "experimental" down any chain of five stages. The
average is worse: it lets a confident later stage launder an unreliable earlier
one.

**One honest complication is recorded rather than smoothed over.** The octave
margin — how much better the reported tempo scored than half or double it — is
measured on preference-weighted scores rather than on raw evidence, and that
looks like the opposite of what ADR-0006 asks for.

The justification is that a signal repeating exactly every beat also repeats
exactly every two beats. Its correlation at both lags is equal, and no better
signal processing changes that: the octave is not underdetermined by this method,
it is underdetermined by the audio. Reporting the raw margin would give near-zero
confidence for every cleanly produced dance record — a number that is always the
same and therefore carries nothing. What *is* determined is the answer the system
gives, which comes from the evidence together with a stated prior about how
listeners count. So the confidence describes that, the reasoning is written where
the function is, and `TempoEstimate::alternatives` carries the ambiguity itself
with the raw unweighted evidence attached to each reading.

## Performance review

The claims that would be expensive to retrofit are measured on real signal
lengths rather than argued.

| Stage | Five minutes of audio |
|-------|----------------------|
| Novelty curve, including 51 680 transforms | 1.10 s |
| Tempo estimation | 13 ms |
| Beat tracking over 640 beats | 15 ms |

Two structural choices produced this, and both replaced something that would
have shipped and then had to be rewritten.

**The autocorrelation goes through the transform.** The direct double loop costs
the curve length times the largest lag — about five thousand million operations
for a ten-minute track. The Wiener-Khinchin route is two transforms of the padded
curve. This is why tempo estimation is 13 milliseconds rather than several
minutes.

**The real-input transform is specialised.** Packing even and odd samples into a
half-length complex sequence and untangling the result halves both the time and
the memory of every one of those fifty thousand transforms.

## Security review

Nothing new. No input or output, no secrets, no network. The core still has zero
runtime dependencies, now across ten crates — and this sprint is where that
would most plausibly have been broken, since a transform library is the obvious
thing to reach for.

## Accessibility review

No user-facing surface. One decision was taken with accessibility in mind: beats
carry an individual `strength`, so a grid drawn over a breakdown can show which
beats were measured and which were coasted through, rather than presenting a
uniform grid that implies uniform certainty.

## Findings raised on my own work

**One genuine defect, found by a test that failed for the right reason.** The
tempo estimator reported *half time* for any track whose beat period was not a
whole number of analysis frames — which is almost all of them.

The mechanism: a novelty curve is close to an impulse train, so its correlation
is near zero except at exact integer lags. A period of 114.8 frames splits its
evidence between lags 114 and 115, while the doubled period at 229.7 happens to
have harmonics landing closer to whole numbers and scores higher. The estimator
then chose half time *confidently*, because the margin was real.

This is the kind of defect that survives review: the code was correct, the
mathematics was correct, and the failure only appears for inputs that are not
round numbers. It was caught because the test used 127.3 BPM rather than 128.
The fix is a one-frame Gaussian on the correlation — restoring the width that
quantisation removed — plus a fractional-lag search so the comb places its
harmonics at the true multiples rather than at multiples of a rounded value.

**Four test expectations were wrong rather than the code**, and the pattern is
worth naming because it is the same one the last review found: each asserted a
number where a property was meant.

1. A transform test used a fixed absolute tolerance, which tightens in relative
   terms as the transform gets larger — so it would have failed at size 2048
   while passing at 16, measuring size rather than correctness. It now scales
   with the size, and the comment explains that the *reference* is the less
   accurate of the two.
2. A window test measured sidelobes by transforming a window at its own length.
   A Hann window is exactly three complex exponentials, so its own-length
   transform is exactly three non-zero bins: the test read −300 dB of sidelobes
   for both shapes and would have passed for any coefficients at all. It now
   pads sixteenfold and samples between the zeros.
3. An onset test demanded timing within one hop, when the detector has a known
   systematic early bias. It now asserts the two things that are actually true
   and actually matter: onsets are never late and never more than half a window
   early, and the *spacings* — which is what tempo consumes — carry no
   systematic error at all.
4. A test asserted that a sustained tone produces one onset at its start. The
   tone also *stops*, and cutting a tone dead is audibly a click. The test was
   asserting something false about its own signal; it now claims that nothing
   happens while the tone sustains, which is the property that matters.

**Two design errors found while testing, both fixed properly rather than tuned
around.**

The downbeat detector read low-band energy at the single frame a beat sat on.
Because onsets are detected slightly early, that frame can land just *before* the
kick, so the loudest moment of the bar was being sampled at its quietest point.
It now averages over the first half of each beat, which is also the physically
right thing: a kick is a hundred milliseconds of low end, not an instant.

The beat grid took its tempo from the coarse estimate and its origin from the
first tracked beat, so both inherited the resolution of the novelty curve. It now
fits a line through every tracked beat by least squares. The error in the slope
falls with the number of beats rather than staying fixed, so a track with 192
beats gets a tempo accurate to a hundredth of a beat per minute — which is what
makes a grid that still lines up at the end of a six-minute record.

## Are the project's principles still respected?

| Principle | Verdict |
|-----------|---------|
| Never the easy solution when a premium one exists (MP#1) | Held. A global dynamic programme rather than greedy peak-stepping, so a breakdown costs nothing; a fitted grid rather than two beats and a period. |
| Production-ready, never placeholder (MP#13) | Held and enforced. |
| Modular, dependencies inward (MP#4, MP#7) | Held. The new crate depends on `prv-time` and `prv-harmony` and nothing else. |
| Honest about uncertainty (MP#25) | Held, including where honesty was inconvenient: the octave posterior is documented as a posterior. |
| Deterministic across platforms (ADR-0006) | Held structurally — the transform is ours — and tested. |
| Quality is not a phase (MP#27) | Held. One real defect and four wrong tests found and fixed within the sprint. |

## Known limitations

1. **Time signature is assumed to be four-four.** Inferring it from audio is a
   research problem whose failure mode is a grid that is wrong in a way a user
   cannot correct by hand. `track_with_signature` accepts an explicit value, so
   the capability is present and the inference is not pretended.
2. **The grid carries a single tempo, not a tempo map.** `prv-time` supports
   multi-segment maps, and a live or hand-played record would be described better
   by one. Populating it from beat-to-beat spacing would encode every tracking
   error as a tempo change the user then has to edit;
   `deviation_from_constant_tempo` exposes the number that says whether a
   constant tempo is a good description, so the decision can be made on evidence
   when a later stage can produce something better.
3. **The confidence thresholds are provisional**, as ADR-0006 requires, pending
   Phase 3 calibration against labelled data. Risk R-04.
4. **Processors still have no parameter-addressing scheme** — unchanged from the
   last review, still Phase 2 work.

## Definition of done

| Criterion | Status |
|-----------|--------|
| Architecture reviewed | Yes — no new records needed |
| Implemented, no placeholders | Yes, enforced |
| Tests passing | Yes — 410 |
| Performance validated | Yes — measured end to end, five minutes of audio in 1.1 seconds |
| Documentation updated | Yes, in the same commit |
| Accessibility verified | Not applicable yet; one enabling decision recorded |
| Security reviewed | Yes — still zero runtime dependencies |
| No critical technical debt introduced | Four recorded limitations, none critical |
