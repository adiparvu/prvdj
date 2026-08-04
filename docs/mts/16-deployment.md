# 16. Deployment

Governed by Master Prompt #28. Status: **Continuous integration operational.
The release pipeline is authored end to end and has never run** — see *What
has not happened* below, which is the part of this page that matters.

## Gates in force on every pull request

| Gate | What it enforces |
|------|------------------|
| Format check | Consistent formatting before any diff is read |
| Static analysis, warnings denied | The panic-capable constructs the realtime contract forbids |
| Build, all targets | No broken build reaches the branch |
| Tests | 1102 Rust tests and 104 Swift tests |
| **Realtime contract, release mode** | Zero allocation on the render path in the configuration that ships |
| Documentation build, warnings denied | No undocumented public interface |
| Architecture rules | Eleven rules: core purity, unsafe confinement, no placeholder markers, crate documentation, decision-log integrity, entitlements, generated bindings, generated icon |
| Design tokens | Generated bindings current, and compiling |
| Dependency review | Advisories, licences, sources |

The architecture and token gates are themselves verified: each has been run
against a deliberate violation to confirm it fails, and against the clean tree to
confirm it passes. A gate nobody has seen fail is a gate nobody should trust.

## Getting an application out of a repository

The Swift package builds an *executable*. TestFlight takes an *application*: a
bundle carrying an Info.plist, an icon, a privacy manifest, an embedded
provisioning profile and a signature. SwiftPM does not produce one and is not
trying to, so there are two build systems over one source tree — the package,
which is what Linux tests on every commit, and an Xcode project, which is what
ships. Both read the same directories, so a file added to `Sources/PRVCore`
appears in both without anybody remembering.

| Step | What it is | Where |
|---|---|---|
| 1 | Build the core for five Apple targets, merge them, assemble an `.xcframework` | `tools/build-apple-libraries.sh` |
| 2 | Generate the Xcode project from a text specification | `apple/project.yml`, via XcodeGen |
| 3 | Archive and export each platform | `.github/workflows/release.yml` |
| 4 | Upload to App Store Connect with an API key | same |

**Why five targets.** `Package.swift` links `core/target/debug`: one
architecture, built for debugging, for whatever host ran `cargo`. A shipped
macOS application runs on Apple silicon and Intel; iOS needs a device slice and
a simulator slice, and the simulator is a different target triple rather than a
different build of the same one. An `.xcframework` is the container that holds
them and lets the linker choose.

**Why the project is generated.** `project.pbxproj` is a build artefact that
happens to be committed by convention: opaque identifiers, no meaningful diff,
and merge conflicts that take an afternoon. The specification is the source, the
project is disposable, and a reviewer can see what changed in a build setting —
which is the part that actually matters.

**Why nothing is signed in the repository.** Signing is an organisation's
identity. Team identifier, certificate, profiles and the App Store Connect key
all arrive as repository secrets; architecture rule 8 checks that none of them
is committed. An API key rather than an Apple ID and password, because a
password cannot be scoped and has to be rotated by hand.

**The build number is the run number.** It rises on its own, and two uploads can
never claim to be the same build — which App Store Connect rejects, and finding
that out at the upload is finding out late. The marketing version stays in the
Info.plist, because it is a product decision rather than a counter.

## What has not happened

**None of the above has ever run.** Not the framework build, not the project
generation, not an archive, not an upload. It is authored against Apple's
documented behaviour and reviewed, and it has never met a compiler.

That matters more here than in most of this repository, because the code it
packages has also never been compiled: the four framework adapters and
`Views.swift` are the one part of the product no continuous-integration run has
ever touched. The first `macos-14` job to run will find whatever is wrong with
about three hundred lines of never-compiled Swift *and* with the pipeline
carrying it, at the same time.

The honest expectation is that the first run fails, probably several times, and
that the failures are ordinary — a missing import, a signing setting, a path.
Publishing the pipeline is what makes those failures visible instead of
hypothetical.

**Also missing before a tester can be invited:** an Apple Developer Program
membership, a registered bundle identifier for `studio.prv.aidj`, an App Store
Connect record, and the eight secrets the release workflow names. None of those
is code, and none can be done from here.

## Not yet in place

Environments beyond continuous integration, feature flags with an emergency kill
switch, rollback procedures and release notes automation. All are Phase 8
(Master Prompt #30) and all are specified in Master Prompt #28.

## The macOS gap

The Apple job is declared and disabled. Until a macOS runner executes it, no
Apple-framework code may be reported as verified. See section 17.
