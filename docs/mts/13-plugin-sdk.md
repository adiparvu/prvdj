# 13. Plugin SDK

Governed by Master Prompt #23. Isolation model fixed by
[ADR-0005](../adr/0005-plugin-isolation.md). Status: **In progress** — the
policy is built and verified (`prv-plugin`, Sprint 19); the runtime that obeys it
is Phase 6.

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

## What is built

`prv-plugin` holds the policy: which tier a plugin belongs to, what its manifest
may assert, what the user has to approve, when a plugin has had enough chances,
and what the graph owes it in latency. It runs none of it — there is no
WebAssembly runtime, no child process, no signature verification and no dynamic
loading, because all four are input and output and ADR-0001 keeps those outside
the core. What the crate provides is the set of decisions a host has to obey, in
a form that can be tested without one.

Five properties are worth naming because they are the ones a host cannot be
trusted to reimplement consistently:

- **Every lifecycle state answers either *processes* or *passes through*.** There
  is no third answer, so there is no event whose outcome is silence. That is
  ADR-0005's second success criterion as a property over the whole state machine
  rather than a test of one path.
- **A bypass does not move the music in time.** Latency compensation counts every
  plugin whose slot exists, running or not, and the pass-through delays by the
  amount the plugin declared. Counting only running plugins would mean a plugin
  failing knocks the set out of time — worse than the failure.
- **Granted is not requested.** The manifest says what a plugin wants; the
  registry holds what the user approved; every authorisation question is answered
  from the second, and the approved set is intersected with the requested one so
  the host cannot invent an authority nobody read in a prompt.
- **Two overrun rules, not one.** A run of three consecutive overruns, *or* eight
  within the last sixty-four blocks. The consecutive rule alone misses a plugin
  that overruns every third block, which is a dropout four times a second.
- **The certified tier cannot be claimed by a file.** A review that a manifest can
  assert it passed is not a review, so promotion is a separate call made by a
  part of the build that holds the review's outcome.

Unsigned code cannot load without the user saying so, and code signed by
something unrecognised is treated as worse than unsigned rather than better —
somebody went to the trouble of making it look signed.
