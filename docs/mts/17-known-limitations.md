# 17. Known Limitations

Master Prompt #28 requires known issues to be documented before a release is
approved. Recording them here, plainly, is cheaper than discovering them later.

## 1. No Apple-framework code has been compiled

The continuous-integration environment available for this work is Linux. It
compiles and tests the Rust core and framework-free Swift. It cannot compile
SwiftUI, AVFoundation, AudioToolbox or CoreAudio, which require the Apple SDKs.

**Consequence.** Every module in `core/` and the generated Swift token bindings
are reported as *verified*. Apple-framework modules will be reported as
*authored* until a macOS runner compiles and tests them. The job exists in the
workflow and is disabled.

Reporting authored code as working would violate Master Prompt #13 and #27, so
the distinction is maintained explicitly in
[implementation-status.md](implementation-status.md).

## 2. Harmonic weights are provisional

The relation classification in `prv-harmony` reflects established practice and is
stable. The numeric weights attached to those relations are initial values, not a
calibrated model. ADR-0006 schedules calibration against listener evaluation in
Phase 3.

The ordering is meaningful and is tested; the absolute values are not yet
trustworthy and are documented as such at the point of use.

## 3. Processors have no parameter-addressing scheme

Each processor exposes typed setters — `set_gains`, `set_position` — which is
clear to call and cannot express automation, a MIDI mapping or a plugin
parameter, all of which need to address a parameter by identity rather than by
name at the call site. Master Prompt #21 requires automation of ten parameter
kinds and Master Prompt #23 requires plugins to declare theirs.

A parameter descriptor scheme — identity, range, unit, default, automatable — is
the correct answer and is scheduled for Phase 2 with the automation lane. It is
recorded here rather than improvised now, because a parameter model designed
around one processor's needs would have to be redone when the second arrived.

*Resolved in Sprint 1: the tempo map now supports multiple segments, so a beat
grid can follow a recording that drifts.*

## 4. Separation model not selected

ADR-0004 fixes the boundary, caching and privacy posture for stem separation but
deliberately does not choose a model. That requires measured comparison on
separation quality, transient behaviour, latency, memory and licence terms across
supported device classes. Phase 3.

## 5. Six token groups undefined

Icons, illustrations, charts, borders, controller colours and accessibility
colours are declared as pending rather than invented. Each lands with the first
screen that consumes it. See section 5.

## 6. Nothing user-facing exists

By design. Master Prompt #30 sequences AI behind a stable timeline, playback and
export, because a planner evaluated against an unstable foundation cannot be
debugged. Phase 1 begins the user-facing work.
