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

## 6. Unreadable operations are relayed but not stored

The wire format lets a device pass on operations made by a newer build, byte for
byte, so an install a version behind relays rather than blocking a fleet. What it
does not do is *hold* them: they are not written into the log, so a device that
merges a message and later derives a new one from its own log will not re-emit
them.

Doing it properly means the log holding operations it cannot fold, and it means
being careful about the version vector — a device that recorded them as seen
would be telling peers it holds work it cannot produce, which is worse than not
holding it at all. `SyncReport::carried` counts them so a person can be told
"part of this project was made with a newer version of the app", which is true
and actionable. The storage is deferred deliberately, not overlooked.

## 7. Placement identities are namespaced, not globally unique

The high half of a placement identity is a 32-bit fingerprint of the device that
allocated it, so two devices collide only if their fingerprints do. Exactness
would need a wider identity, and a placement identity is a `uint64_t` at a
boundary whose major version forbids changing a call that already exists.

The risk is bounded by the devices sharing one project — a handful, not a
population — and a collision fails as a reported conflict rather than as silent
loss. Widening it belongs with the next major version of the boundary, not before.

## 8. There is no transport

The core produces bytes and reads bytes; nothing opens a socket, which is
ADR-0001 working rather than a gap. The same four calls serve a cloud service, a
local network, a memory stick and a file attached to an email.

The application's entitlements still have no `com.apple.security.network.client`,
and architecture rule 10 keeps it that way. It arrives in the same commit as the
consent screen it depends on — deliberately, so that the operating system makes
an outbound connection impossible until the user has been asked.

## 9. Nothing user-facing exists

By design. Master Prompt #30 sequences AI behind a stable timeline, playback and
export, because a planner evaluated against an unstable foundation cannot be
debugged. Phase 1 begins the user-facing work.
