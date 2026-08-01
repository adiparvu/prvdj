# Master Technical Specification

**PRV AI DJ Studio — v0.1 (Sprint 0)**

Mandated by Master Prompt #31. This document is the single engineering reference
for the platform. If implementation and this document disagree, this document is
updated in the same change that caused the disagreement.

> If every other document disappeared, this one should contain enough information
> for a new engineering team to continue development with confidence.

## Sections

| # | Section | File |
|---|---------|------|
| 1 | Executive Summary | [01-executive-summary.md](01-executive-summary.md) |
| 2 | Architecture Overview | [02-architecture-overview.md](02-architecture-overview.md) |
| 3 | Module Index | [03-module-index.md](03-module-index.md) |
| 4 | Dependencies | [04-dependencies.md](04-dependencies.md) |
| 5 | Design Tokens | [05-design-tokens.md](05-design-tokens.md) |
| 6 | SwiftUI Components | [06-swiftui-components.md](06-swiftui-components.md) |
| 7 | Navigation | [07-navigation.md](07-navigation.md) |
| 8 | State Management | [08-state-management.md](08-state-management.md) |
| 9 | Audio Engine | [09-audio-engine.md](09-audio-engine.md) |
| 10 | AI Orchestrator | [10-ai-orchestrator.md](10-ai-orchestrator.md) |
| 11 | Timeline | [11-timeline.md](11-timeline.md) |
| 12 | Cloud | [12-cloud.md](12-cloud.md) |
| 13 | Plugin SDK | [13-plugin-sdk.md](13-plugin-sdk.md) |
| 14 | Security | [14-security.md](14-security.md) |
| 15 | Testing | [15-testing.md](15-testing.md) |
| 16 | Deployment | [16-deployment.md](16-deployment.md) |
| 17 | Known Limitations | [17-known-limitations.md](17-known-limitations.md) |
| 18 | Future Roadmap | [18-roadmap.md](18-roadmap.md) |
| 19 | Decision Log | [../adr/README.md](../adr/README.md) |
| 20 | Performance Budgets | [20-performance-budgets.md](20-performance-budgets.md) |

## Supporting registers

| Register | File | Purpose |
|----------|------|---------|
| Module template | [module-template.md](module-template.md) | The 16-section structure every module document must use |
| Implementation status | [implementation-status.md](implementation-status.md) | What is verified, authored, in design, not started |
| Risk register | [risk-register.md](risk-register.md) | Living register with probability, impact, mitigation, owner, review date |

## Maintenance rule

After every completed task: update the affected section, the dependency map, the
implementation status board, the risk register and the roadmap. Documentation
never falls behind implementation (MP#31, *Continuous Synchronisation*).
