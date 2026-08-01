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

## 3. The tempo map has a single segment

The transport clock keeps one tempo anchor. Master Prompt #3A requires multiple
tempo regions and live tempo changes. Extending it means storing a list of
anchors and binary-searching them; the arithmetic does not change, and no caller
is affected. Scheduled for Phase 1 alongside the beat grid.

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
