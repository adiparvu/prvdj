# Sprints 29–30 — The application layer begins

Twenty-eight sprints produced twenty-three crates of musical judgement that no
host could reach. That is not a criticism of the twenty-eight sprints; the core
had to be right first. But it does mean there was no application, and these two
sprints are the ones that start making one.

| Delivered | Tests |
|-----------|-------|
| `prv-ffi` — the C ABI boundary | 35 |
| `bridgegen` — generated header and Clang module map | 5 |
| A C host that drives the boundary end to end | 2 |
| `PRVCore` — safe Swift over the boundary | 15 (Swift) |
| Architecture rule 9 — generated bindings are current | enforced |
| — | **977 Rust, 15 Swift** |

All gates green: `cargo test`, `cargo clippy --all-targets -- -D warnings`,
`cargo fmt --check`, nine architecture rules, the design-token staleness gate,
the allocation gate, and now `swift test`.

## Business outcome

**The core is reachable.** A host can start an engine, place a track, drive the
transport and render audio. That is the difference between a library and a
product, and until this sprint the product did not have it.

**A defect in the core cannot corrupt the host.** Unwinding out of an
`extern "C"` function is undefined behaviour — not "usually fine", undefined,
and on some targets it corrupts the stack of a process that has no idea Rust is
involved. Every entry point catches and returns `PRV_PANICKED`. It should never
fire; the case that should never arise is exactly the one nobody has a plan for.

**A stale header cannot ship.** The architecture overview requires generated
bindings "because hand-written bindings drift", and a drifting binding does not
fail to compile — it computes the wrong answer. A header still claiming
`PRV_PLAYBACK_PAUSED` is 4 after the library moved it to 5 shows the wrong thing
on stage. Rule 9 fails the build.

**Most of the Apple layer is now tested.** See below; this is the largest change
of the two sprints and it is not a code change.

## Architecture review

**No new decision record.** ADR-0001 already put the language boundary on the
ports seam and said what belongs on each side. This is that decision, built.

**One handle, not twelve.** The architecture overview requires coarse-grained
calls. A host holding a transport, a project and a renderer separately would be
a host asked to keep invariants consistent in a language with no access to the
definitions of consistent.

**Audio crosses as a callback, not a queue.** The renderer reads the frames it
needs in the order it needs them, and a set is not always played forwards. A push
model makes the host predict seeks it cannot see, and the first thing it gets
wrong is the block after a jump.

**The numbers are written out by hand, on purpose.** `#[repr(i32)]` on the
core's own enums would be shorter and wrong: it would put an ABI commitment
inside `prv-transport`, a crate that has no idea a boundary exists and should
stay free to reorder its variants for readability. The moment somebody sorts
`PlaybackState` alphabetically, a host compiled last year reads "paused" as
"recovering". So the mapping lives at the boundary that promises it.

**Rule 1 was narrowed, and narrowing a rule deserves saying out loud.** It now
scans only sources cargo links into a library, so a generator that writes a
header and a test that spawns a compiler are not mistaken for the core reaching
for the filesystem. The exclusion is structural rather than a judgement call —
`tests/`, `benches/`, `examples/` and `src/bin/` are never linked into a
library — and every other rule still scans everything, because a placeholder
marker is just as wrong in a test.

## The limitation that shrank

R-01 used to read "no Apple code has been compiled". It now reads "no
Apple-*framework* code has been compiled", and the gap between those two
sentences is most of the Apple layer.

Swift 6.1 runs on Linux. `PRVCore` — the wrapper over the boundary, where
pointer lifetimes, the audio callback and every error code live — imports
nothing but Foundation, so it builds and its tests run on every commit against
the real static library.

That property was bought, not found. It is why `Package.swift` splits `PRVCore`
from `PRVKit`: the moment one file in the wrapper imports AVFoundation, the whole
target stops building on Linux and the most dangerous code in the application
goes back to being unverified. The split is a load-bearing decision, and the
comment in the manifest says so.

What is left genuinely unverified is CoreAudio, AVFoundation, the keychain and
SwiftUI. Real, and much smaller than "the Apple layer".

## What the tests found

**The generator dropped the last two arguments of every wrapped declaration.**
`write_signature` never emitted its final line. The header still looked
plausible — that is the whole problem with generated code that nobody compiles —
and the C harness caught it within a minute of existing.

**Rust calling Rust proves nothing about a header.** Every unit test in `prv-ffi`
passed while the header was unparseable. `tests/c_host.rs` compiles a real C
program against the committed header with `-Werror`, links the real archive, and
renders audio through a C callback. It is the only test in the crate that could
have found the wrapping defect, and it did.

**A `cdylib` beside a `staticlib` silently changed what Swift linked.** A Unix
linker given `-lprv_ffi` prefers the shared object, so `swift test` compiled
against the archive's declarations and then failed to start for want of a library
at run time. No host wanted the shared object; the fix was not to build one.

## Known limitations

- `PRVKit`'s framework adapters and all of `PRVUI` are not written. The boundary
  they will sit on is.
- The boundary carries transport, placement and rendering. The planner, the
  library index, analysis, consent and entitlements are reachable in Rust and not
  yet across C. Each is a minor ABI addition — a new call, no existing call
  moving — which is exactly what the version scheme's minor field is for.
- There is no snapshot mechanism yet. The architecture overview describes a
  triple-buffered publication out of the realtime domain; today a host reads
  transport state with an ordinary call, which is correct under the documented
  one-thread-at-a-time rule and is not yet what a 60 Hz interface wants.
- The C harness runs on the host that builds the repository, today Linux on
  x86-64. A calling-convention mismatch specific to arm64-apple-darwin would not
  be caught by it.
