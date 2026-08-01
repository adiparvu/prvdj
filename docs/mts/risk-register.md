# Risk Register

Living register required by Master Prompt #31. Reviewed each sprint.

| ID | Risk | Category | Probability | Impact | Mitigation | Owner | Review |
|----|------|----------|-------------|--------|------------|-------|--------|
| R-01 | Apple-framework code accumulates unverified because no macOS runner exists | Operational | High | High | Verified/authored distinction enforced in the status board; macOS job already defined; acquiring a runner is the top Phase 1 dependency | Platform | Phase 1 start |
| R-02 | An allocation reaches the realtime path | Technical | Medium | Critical | Allocation gate on every pull request, debug and release, with a positive control | Audio DSP | Continuous |
| R-03 | Objective function produces valid but musically dull sets | Product | High | High | Reference sets judged by working DJs; creativity dimension rewards distinctness; A/B/C returns genuinely distinct optima | AI | Phase 3 |
| R-04 | Confidence labels drift out of calibration as analysis improves | Product | Medium | High | Calibration is a versioned artefact with its own tests; changing an analysis stage requires recalibration before release | AI | Phase 3 |
| R-05 | Foreign-function boundary drifts between Rust and Swift | Technical | Medium | High | Bindings generated, never hand-written; contract tests on both sides | Platform | Phase 1 |
| R-06 | Operation log growth degrades project load time | Performance | High | Medium | Periodic snapshots; compaction that preserves every named version and branch point | Core | Phase 2 |
| R-07 | Separation model licence incompatible with commercial distribution | Legal | Medium | High | Licence compatibility is a gate in the Phase 3 evaluation, not an afterthought | AI | Phase 3 |
| R-08 | WebAssembly plugin runtime allocates during execution | Technical | Medium | Critical | Pooled pre-instantiated modules; the core allocation gate extended to cover a hosted plugin | Platform | Phase 6 |
| R-09 | Team unfamiliarity with Rust slows feature delivery | Operational | Medium | Medium | Boundary kept narrow; most feature work is Swift; core coding standards documented | Engineering | Phase 1 |
| R-10 | Accessibility on the timeline is deferred and becomes unaffordable | UX | Medium | High | Structural navigation model designed alongside the timeline, not after; accessibility regressions block release | Design | Phase 2 |
