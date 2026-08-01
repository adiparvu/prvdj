# ADR-0004: Stem separation strategy

- Status:        Accepted
- Date:          2026-08-01
- Review date:   2026-11-01
- Deciders:      Lead AI, Lead Audio DSP, Privacy Engineering
- Related modules: audio-analysis, stem-engine, live-performance, remix-engine
- Source requirements: MP#1 (stem playback), MP#2 (stem waveform), MP#3C (Stem Engine),
  MP#22 (Stem Performance), MP#26 (AI Privacy), MP#29 (copyright principles)

## Context

MP#3C requires six stems — vocals, drums, bass, melody, FX, other — each with
solo, mute, gain, EQ, effects, automation, waveform and spectral preview. MP#22
requires stem control during live performance with minimal latency. MP#26
requires that cloud-based AI features can be disabled and that external AI use is
clearly indicated. MP#29 requires that the platform not become a mechanism for
distributing content.

## Problem

Where does source separation execute, and how do its outputs participate in the
non-destructive model?

## Alternatives considered

### A. Cloud-only separation

*Rejected as the default.* It requires uploading the user's music to a server,
which is a privacy problem under MP#26, a rights problem under MP#29, and makes
a core creative feature unavailable offline, contradicting MP#1's offline-first
rule. It would also make stem performance impossible in a venue with poor
connectivity — precisely where it is most wanted.

### B. On-device separation only

*Chosen as the default.* Separation runs locally using the platform's neural
accelerator. Nothing leaves the device.

### C. Hybrid, on-device by default with an optional cloud accelerator

*Chosen as the overall shape.* Cloud separation is available as an explicit,
opt-in accelerator for users who want faster batch processing and accept the
transfer, with a clear indication that an external service is being used, as
MP#26 requires.

## Decision

Separation is a **port** with interchangeable adapters, defaulting to on-device.

- The core defines the separation contract: input frames, requested stem set,
  progress reporting, cancellation, and an output artefact description. It does
  not know what implements it.
- The Apple adapter executes the model on the platform's neural accelerator; the
  portable adapter uses a cross-platform inference runtime. Both are
  interchangeable from the core's point of view.
- The optional cloud adapter is opt-in, disclosed in the interface while active,
  and never enabled by default.

**Stems are derived artefacts, never replacements.** They are written to the
content-addressed cache described in ADR-0003, keyed by source fingerprint plus
model identity and version. The source file is never modified, satisfying MP#3C's
non-destructive rule. A model upgrade invalidates exactly the artefacts that
model produced.

**Separation never runs on the audio thread.** It is a background job scheduled
by the orchestrator, subject to MP#19's resource priorities: during live playback
it is suspended entirely; during export and idle it may use available capacity.
Live stem performance plays back *already separated* stems, so the realtime path
only mixes additional channels.

**Model selection is deferred to a dedicated evaluation.** Choosing a separation
model requires measured comparison on separation quality, artefact behaviour on
transients, latency, memory footprint and licence terms across the device classes
we support. That evaluation belongs to Phase 3 of the roadmap and will produce
its own record. What this record fixes now is the boundary, the caching model and
the privacy posture — so that the evaluation can change the model without
touching anything else.

## Consequences

### Positive

- Stem features work with cloud AI disabled, satisfying MP#26 and MP#1.
- The user's library never leaves the device by default.
- Model upgrades are a cache-invalidation event, not a migration.

### Negative

- On-device separation is slower than a server with dedicated accelerators, and
  is not viable on the oldest supported devices. Capability is therefore
  device-dependent and must be communicated honestly rather than hidden.

### Risks and mitigations

| Risk | Probability | Impact | Mitigation |
|------|-------------|--------|------------|
| Separation too slow on entry-level devices | High | Medium | Device capability probe; realistic time estimates shown before starting; batch separation offered as a background job with progress and cancellation |
| Model licence incompatible with commercial distribution | Medium | High | Licence compatibility is a gate in the Phase 3 evaluation, not an afterthought |
| Stem cache consumes excessive storage | High | Medium | Cache is size-bounded and evictable; stems are regenerable by definition; storage view shows what is reclaimable |

## Success criteria

1. Stem features function with networking disabled.
2. No separation work is scheduled while a live performance is in progress.
3. Changing the separation model invalidates only stem artefacts, and no source
   file or project operation is affected.
