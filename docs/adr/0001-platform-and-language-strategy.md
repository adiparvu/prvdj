# ADR-0001: Platform and language strategy

- Status:        Accepted
- Date:          2026-08-01
- Review date:   2026-11-01
- Deciders:      Chief Architect, Lead Audio DSP, Lead Apple Platform, Lead AI
- Related modules: all
- Source requirements: MP#1 (Long-Term Vision), MP#2 (UI/UX Bible), MP#3A (Engine Architecture),
  MP#4 (Clean Architecture), MP#7 (System Principles), MP#8 (Target Platforms),
  MP#15 (Engineering Values), MP#17 (SwiftUI Component Library), MP#18 (Audio Thread),
  MP#23 (Plugin SDK), MP#26 (AI Privacy)

## Context

The specification corpus pulls in two directions that must both be satisfied.

**Toward Apple-native.** MP#2, MP#16 and MP#17 mandate an interface built from
Liquid Glass materials, SF Pro Display, Dynamic Type, haptics, VoiceOver, and a
*SwiftUI Component Library* named as such. MP#8 sets Phase 1 to iPhone, iPad and
macOS. MP#11 P8 makes accessibility non-negotiable, and platform-native
accessibility is far ahead of any cross-platform toolkit.

**Toward portable.** MP#8 lists Windows, Web, Android and Vision Pro as future
targets and asks for "feature parity where practical". MP#3A requires the engine
to be "cross-platform ready" and "future-proof". MP#7 states that future
expansion "should require adding modules, not rewriting the system". MP#26
requires the AI planning path to work with cloud AI disabled, which means the
musical intelligence must run on-device on every platform we ship.

Layered on top, MP#18 imposes a hard-realtime contract on the audio callback:
never allocate, never lock, never block, never wait. MP#11 P4 instructs us to
assume every session is a live performance, and Module Spec #002 states plainly
that "during a live performance, failure is not an acceptable outcome".

## Problem

Which language implements which layer, so that all of the following hold?

1. The Apple experience is fully native, with no compromise on accessibility,
   motion, typography or platform integration.
2. The parts of the product that constitute its actual value — DSP, music
   analysis, mix planning, the project document — reach Windows, Web and Android
   without being rewritten.
3. The absence of allocation and locking on the audio thread is *demonstrable*,
   not merely intended.
4. The core is testable in continuous integration on commodity Linux runners,
   so that the feedback loop is fast and every merge is verified.

## Constraints

- MP#15 ranks engineering values: correctness → reliability → simplicity →
  maintainability → performance → scalability → developer experience → visual
  polish. Introducing a second language costs simplicity and developer
  experience; it must buy correctness and reliability to be justified.
- MP#7 requires that a new engineer understand the architecture after three
  years. Any language boundary must be narrow, stable and documented.
- MP#28 requires reproducible builds and quality gates on every pull request.

## Alternatives considered

### A. Single-language Swift monolith

SwiftUI presentation, Swift domain, DSP hosted in AVAudioEngine with Swift
render callbacks.

*For:* one language; smallest conceptual surface; best possible integration with
Apple frameworks; fastest initial velocity; a single toolchain.

*Against:* Swift uses automatic reference counting. Retain/release traffic,
existential boxing, array growth, string operations, closure context capture and
class deallocation can all allocate, and the compiler offers no mechanism to
prove that a given function does not allocate. Enforcing MP#18's audio-thread
contract therefore rests entirely on reviewer vigilance, and a single missed
allocation in a 128-frame callback is an audible dropout on stage. Swift on
Android and WebAssembly is not production-grade in 2026, so Phase-2 platforms
would require reimplementing the DSP, analysis and planning code — precisely the
rewrite MP#7 forbids.

### B. Swift application and presentation over a portable Rust core (chosen)

*For:* Rust has no garbage collector and no reference-counting traffic unless
explicitly requested, so a render function that takes `&mut [f32]` and touches
only preallocated state provably does not allocate; this is enforceable in tests
and in CI. Rust is production-grade on macOS, iOS, Windows, Linux, Android and
WebAssembly, so the musical intelligence is written once. A stable C ABI gives
the plugin system of MP#23 a natural, language-neutral boundary, and the
WebAssembly toolchain gives it a real sandbox. The core builds and tests on Linux
runners, which are cheap and fast.

*Against:* two languages; a foreign-function boundary to design, version and
document; more build machinery (xcframework assembly, cross-compilation);
higher onboarding cost.

### C. Swift application over a portable C++ core

*For:* the established industry pattern — Logic Pro, Ableton Live, Serato and
djay all run C++ audio cores under native shells. Mature DSP library ecosystem
(JUCE and others). Excellent Apple toolchain integration.

*Against:* no memory-safety guarantees, which matters most in exactly the code
that must never fail; weaker dependency management and reproducibility;
a considerably worse WebAssembly and Android story than Rust; slower iteration.
C++ buys nothing over Rust here except library familiarity, and pays for it in
the currency MP#15 ranks first — correctness.

### D. Cross-platform UI framework over a native core

Flutter, React Native or Electron for presentation.

*For:* one UI codebase across all target platforms; fastest route to Phase-2
platform coverage.

*Against:* directly contradicts MP#2, MP#16 and MP#17, which specify an
Apple-native design language down to the material and type system. Accessibility
fidelity (VoiceOver rotor behaviour, Dynamic Type, Switch Control) is materially
worse than native, and MP#11 P8 forbids treating accessibility as optional.
Sustained 120 FPS timeline interaction with thousands of automation points
(Module Spec #003) is not reliably achievable. Rejected on product grounds, not
technical convenience.

## Decision

Adopt **alternative B**, structured as a hexagonal architecture whose ports
boundary coincides with the language boundary.

```
┌──────────────────────────────────────────────────────────────┐
│  PRVUI            SwiftUI  — presentation only               │
│  PRVKit           Swift    — infrastructure adapters,        │
│                              platform services, FFI bridge   │
├──────────────────────────────────────────────────────────────┤
│  prv-core         Rust     — domain + application            │
│                              (pure: no I/O, no OS calls)     │
└──────────────────────────────────────────────────────────────┘
        dependencies point downward and inward only
```

**`prv-core` (Rust) owns:**
musical time and the transport clock; the DSP graph and every processor; audio
analysis (tempo, key, structure, energy, loudness, spectrum); the mix planner,
transition scorer and energy-curve engine; the project document and its event
log; the library index and query engine; all invariants and validation rules.

It performs no file I/O, no networking and no OS calls. Everything the outside
world provides enters through *ports* — traits implemented by the host — or as
plain data. This is what makes it identically usable from a SwiftUI app, a
Windows shell, an Android app, a WebAssembly module and a headless test binary.

**`PRVKit` (Swift) owns:**
adapters that implement those ports — file access and security-scoped bookmarks,
the CoreAudio/AVAudioEngine render host, platform decoders, keychain, network,
cloud sync transport, notification delivery — plus the generated FFI bridge.

**`PRVUI` (SwiftUI) owns:**
presentation only. It holds no business rules, exactly as MP#4 requires.

**The audio callback is hosted by the platform and executed by the core.** The
platform owns the callback because only the platform can (CoreAudio on Apple,
WASAPI on Windows, AAudio on Android). Inside it, the host performs no work
beyond handing preallocated buffers to a single core entry point whose contract
is: no allocation, no locking, no syscalls, bounded execution time. The entire
realtime path is therefore Rust, and its safety is a property we can test rather
than a convention we hope holds. The detailed contract is ADR-0002.

**Decoders are a port, not a fixed choice.** Module Spec #001 requires format
support to be modular and every decoder replaceable. On Apple platforms the
adapter uses AudioToolbox, which gives hardware-accelerated AAC/ALAC decoding and
correct handling of platform-managed formats. On other platforms the adapter uses
a portable Rust decoder. The core sees only decoded frames.

## Consequences

### Positive

- The audio-thread contract of MP#18 becomes verifiable in CI rather than
  aspirational, which is the single highest-value property in the entire system.
- The product's differentiating logic — analysis, planning, transition scoring —
  is written once and reaches every future platform in MP#8 without a rewrite,
  satisfying MP#7's "add modules, do not rewrite".
- MP#26's requirement that AI features work with cloud disabled is satisfied
  structurally: the deterministic planner lives in the core and runs on-device
  everywhere (see ADR-0006).
- The plugin boundary of MP#23 gets a stable C ABI for free, and WebAssembly
  gives untrusted plugins a real sandbox (see ADR-0005).
- Core tests run on Linux runners in seconds, so most merges are gated without
  needing scarce macOS capacity.
- The Clean Architecture dependency rule of MP#4 is enforced by the build system
  rather than by review: `prv-core` cannot import a UI framework because the
  framework does not exist in its language.

### Negative

- Two languages. Onboarding cost is real and is accepted deliberately.
- The FFI boundary must be versioned, documented and covered by contract tests.
  It is a permanent maintenance obligation.
- Build machinery is more complex: cross-compilation for six targets and
  xcframework assembly for Apple platforms.
- Some duplication of small value types across the boundary is unavoidable; this
  is mitigated by generating the Swift side from the Rust definitions rather
  than hand-writing it.

### Risks and mitigations

| Risk | Probability | Impact | Mitigation |
|------|-------------|--------|------------|
| FFI marshalling cost on hot paths | Medium | Medium | Coarse-grained calls only; audio and waveform data crosses as pointers to shared preallocated buffers, never copied per frame; the boundary is benchmarked and the benchmark is a CI gate |
| Boundary drift between Rust and Swift types | Medium | High | Swift bindings are generated from Rust source, never hand-written; a contract test suite runs on both sides of every ABI change |
| Team unfamiliarity with Rust | Medium | Medium | Boundary kept narrow; core coding standards documented; the majority of feature work happens in Swift |
| Rust toolchain instability on a future Apple target | Low | High | Core depends only on stable Rust and a small, audited dependency set; every dependency carries a replacement strategy in the dependency map |

## Success criteria

1. `prv-core` builds and its full test suite passes on Linux with no Apple SDK
   present.
2. A test proves that the realtime entry point performs zero heap allocations
   across a sustained render run, and that test gates every pull request.
3. The public FFI surface is generated, not hand-written, and a contract test
   fails on any unversioned change.
4. No file under `prv-core` references a UI, networking or filesystem API.
5. A new engineer can build the whole project from a clean checkout with one
   documented command.

## Notes

**Environment limitation recorded honestly.** The continuous-integration
environment available for this work is Linux. It compiles and tests the Rust core
and pure-Swift packages, but it cannot compile SwiftUI, AVAudioEngine or
CoreAudio code, which require the Apple SDKs. Consequently:

- Everything in `prv-core` and in pure-Swift packages is built, tested and
  benchmarked here, and is reported as verified.
- Apple-framework code is authored against the specifications and compiled on
  macOS runners defined in CI. Until such a runner executes, that code is
  reported as *authored, not yet compiled* — never as verified.

This distinction is maintained explicitly in the Implementation Status board of
the Master Technical Specification. Reporting unverified code as working would
violate MP#13 (*no placeholder implementations*) and MP#27 (*no feature is
complete without verification*).
