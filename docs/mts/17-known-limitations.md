# 17. Known Limitations

Master Prompt #28 requires known issues to be documented before a release is
approved. Recording them here, plainly, is cheaper than discovering them later.

## 1. Four adapters and the SwiftUI views have not been compiled

The continuous-integration environment available for this work is Linux. It
compiles and tests the Rust core, the C boundary, and Swift that imports no Apple
framework. It cannot compile SwiftUI, AVFoundation, AudioToolbox, Security or
CoreAudio.

**This limitation has been narrowed twice, deliberately, and it is now as small
as the architecture can make it.** It began as "no Apple code has been
compiled". Swift 6.1 runs on Linux, so it became "no Apple-*framework* code".
Then the framework imports were moved from *target* boundaries to `#if
canImport` blocks *inside* the targets, so `PRVCore`, `PRVKit` and `PRVUI` all
build and test on every commit.

What remains unverified is exactly four types and one file of views:

| Unverified | What it does |
|---|---|
| `AVFoundationDecoder` | Decodes a file to mono samples |
| `CoreAudioOutput` | Pulls blocks from a render handle |
| `KeychainStore` | Writes bytes to the keychain |
| `Views.swift` | Draws the models |

**The rule that keeps this honest: no decision lives inside a `#if`.** Every
threshold, format, refusal and fallback is in `Session`, `SpaceModels` or the
core — all tested. An adapter above can be wrong about a pixel or an audio unit
flag. It cannot be wrong about the product, because it does not know anything
about the product.

**The macOS job is now enabled rather than disabled.** It was `if: false`, which
reports nothing and looks identical to a job that passes. It now runs and will
fail on a runner without Xcode — which is the honest signal, and the thing that
turns "we have not checked this" into a red mark somebody has to act on.

**Consequence.** Everything in `core/`, the generated bindings, `PRVCore`,
`PRVKit`'s ports and session, every `PRVUI` model, and *the application starting*
are reported as *verified*. The four adapters and the views are *authored* until
that job runs green somewhere.

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

## 3. Separation model not selected

ADR-0004 fixes the boundary, caching and privacy posture for stem separation but
deliberately does not choose a model. That requires measured comparison on
separation quality, transient behaviour, latency, memory and licence terms across
supported device classes. Phase 3.

## 4. Six token groups undefined

Icons, illustrations, charts, borders, controller colours and accessibility
colours are declared as pending rather than invented. Each lands with the first
screen that consumes it.

## 5. Placement identities are namespaced, not globally unique

The high half of a placement identity is a 32-bit fingerprint of the device that
allocated it, so two devices collide only if their fingerprints do. Exactness
would need a wider identity, and a placement identity is a `uint64_t` at a
boundary whose major version forbids changing a call that already exists.

The risk is bounded by the devices sharing one project — a handful, not a
population — and a collision fails as a reported conflict rather than as silent
loss. Widening it belongs with the next major version of the boundary, not before.

## 6. There is no transport

The core produces bytes and reads bytes; nothing opens a socket, which is
ADR-0001 working rather than a gap. The same four calls serve a cloud service, a
local network, a memory stick and a file attached to an email.

The application's entitlements still have no `com.apple.security.network.client`,
and architecture rule 10 keeps it that way. It arrives in the same commit as the
consent screen it depends on — deliberately, so that the operating system makes
an outbound connection impossible until the user has been asked.

## 7. On iOS, the platform does not enforce the privacy promise

The most important thing to come out of adding iOS, and it is a genuine loss
rather than a detail.

On macOS the guarantee is structural. The sandbox has no
`com.apple.security.network.client` entitlement, so an outbound connection is
*impossible* — not unused, not guarded by our code being right, impossible.
Architecture rule 10 keeps that key absent, and Master Prompt #26's promise rests
on the operating system rather than on us.

**iOS has no such entitlement.** Network access there is not gated by anything an
application declares; every iOS application can open a socket. So on iOS the same
promise is only as good as `prv-security` deciding correctly and every call site
consulting it — which is a much weaker thing, and it would be dishonest to
present the two platforms as offering the same assurance.

What is still true on both: the core performs no input or output at all
(ADR-0001), so there is no code path in it that could open a connection; the
privacy manifest declares no collection; and consent is required before anything
is sent. What is not true on iOS is that the platform would stop us if we were
wrong.

The mitigation worth building, when there is a transport: route every outbound
call through one audited place, so "did anything leave?" is a question about one
file rather than about the whole application. That is not a substitute for the
entitlement, and this section should not be removed when it ships.

## 8. No screen has been drawn

Narrower than it used to read, and the difference is worth stating rather than
leaving as a stale sentence. This section said "nothing user-facing exists", and
that stopped being true several sprints ago: the application starts, and `PRVUI`
holds the models behind six spaces, the library, the transport, planning, the
consent screen and the synchronisation status — all built and tested on every
commit.

What has not happened is a SwiftUI view compiled against the real framework, and
that is section 1 rather than a claim of its own. Master Prompt #30 sequences the
work this way deliberately: a planner evaluated against an unstable foundation
cannot be debugged, so the decisions come first and the drawing follows.

## Resolved

A register that quietly drops an entry is as untrustworthy as one that misses a
problem. Resolved limitations stay here, with what closed them.

| Was | Closed by |
|---|---|
| The tempo map held one segment, so a beat grid could not follow a recording that drifts | Sprint 1 — `prv-time::TempoMap` takes multiple segments |
| Processors had no way to address a parameter, so automation, MIDI mapping and plugin parameters had nowhere to point | Sprint 7 and after — `prv-project::ParameterAddress` names one, `prv-timeline::ParameterDescriptor` says what values it takes, and the renderer reads both |
| Operations from a newer build were counted and dropped, so a relay stopped at a restart | Sprint 40 — carried operations are written into the log, re-emitted, and promoted after an upgrade |
| The sync state machine did not reach the host, so a host would have reimplemented its rules | Sprint 42 — `prv-ffi::sync`, with the rules staying in the core |
