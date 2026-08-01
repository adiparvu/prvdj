# 13. Plugin SDK

Governed by Master Prompt #23. Isolation model fixed by
[ADR-0005](../adr/0005-plugin-isolation.md). Status: **Not started** — Phase 6.

## The tension, and how it is resolved

Master Prompt #23 requires plugins to be sandboxed and requires a crashing
plugin never to stop playback. Master Prompt #18 forbids blocking and waiting on
the audio thread. Genuine isolation normally means another process, and crossing
a process boundary inside a 2.7 millisecond budget is exactly what is forbidden.

Three tiers, assigned by what the plugin needs to touch:

| Tier | Plugins | Isolation | Latency cost |
|------|---------|-----------|--------------|
| 1 | Analysis, visualisation, import, export, AI, metadata, cloud | Separate sandboxed process | None — not on the audio path |
| 2 | Third-party realtime audio | WebAssembly, in process, pre-instantiated, execution-metered | Modest, accepted |
| 3 | First-party and certified native processors | In process, bound by the ADR-0002 contract | None |

A plugin that overruns its budget is interrupted and bypassed; a plugin that
crashes takes down only its own tier-1 process. In both cases the music
continues, which is the requirement.
