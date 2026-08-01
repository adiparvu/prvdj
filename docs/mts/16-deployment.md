# 16. Deployment

Governed by Master Prompt #28. Status: **Continuous integration operational;
release engineering scheduled for Phase 8.**

## Gates in force on every pull request

| Gate | What it enforces |
|------|------------------|
| Format check | Consistent formatting before any diff is read |
| Static analysis, warnings denied | The panic-capable constructs the realtime contract forbids |
| Build, all targets | No broken build reaches the branch |
| Tests | 111 tests, plus one documentation example intentionally not executed |
| **Realtime contract, release mode** | Zero allocation on the render path in the configuration that ships |
| Documentation build, warnings denied | No undocumented public interface |
| Architecture rules | Core purity, unsafe confinement, no placeholder markers, crate documentation, decision-log integrity |
| Design tokens | Generated bindings current, and compiling |
| Dependency review | Advisories, licences, sources |

The architecture and token gates are themselves verified: each has been run
against a deliberate violation to confirm it fails, and against the clean tree to
confirm it passes. A gate nobody has seen fail is a gate nobody should trust.

## Not yet in place

Environments beyond continuous integration, artefact signing, feature flags with
an emergency kill switch, rollback procedures and release notes automation. All
are Phase 8 (Master Prompt #30) and all are specified in Master Prompt #28.

## The macOS gap

The Apple job is declared and disabled. Until a macOS runner executes it, no
Apple-framework code may be reported as verified. See section 17.
