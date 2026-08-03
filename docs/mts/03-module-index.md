# 3. Module Index

The eighteen bounded contexts of MP#7, mapped to their implementation location,
layer and owning specification. `L` marks the layer: D domain, A application,
I infrastructure, P presentation.

| Module ID | Context | L | Location | Source spec |
|-----------|---------|---|----------|-------------|
| `musical-time` | Musical time, transport clock, beat/bar arithmetic | D | `core/prv-time` | MS#002, MP#18 |
| `audio-graph` | DSP graph, processors, realtime contract | D | `core/prv-dsp` | MP#3A, MP#18, MS#002 |
| `mixer` | Decks, channels, EQ, filter, crossfader, metering | D | `core/prv-dsp` | MP#3A, MP#22 |
| `effects` | Effect chain, parameters, automation targets | D | `core/prv-dsp` | MP#3A, MP#3C |
| `master-bus` | Master chain, limiter, true peak, LUFS | D | `core/prv-dsp` | MP#3A, MP#3C |
| `playback-transport` | Playback states, transport operations, loops, slip | A | `core/prv-transport` | MS#002 |
| `audio-analysis` | Tempo, key, structure, energy, loudness, spectrum | D | `core/prv-analysis` | MP#20, MP#3A |
| `waveform` | Tile generation, resolutions, viewport maths | D | `core/prv-waveform` | MS#003 |
| `music-library` | Import, metadata, playlists, collections, search, duplicates | D+A | `core/prv-library` | MS#001 |
| `project` | Operation log, materialisation, versions, branches | D | `core/prv-project` | MP#9, MP#21, ADR-0003 |
| `timeline` | Timeline objects, lanes, editing operations, snapping | D | `core/prv-timeline` | MP#21 |
| `mix-engine` | Planner, transition scoring, pairwise affinity at import, energy curves, versions A/B/C | D | `core/prv-mix` | MP#3B, MP#20, ADR-0006 |
| `learning` | Personal profile, style model, preference confidence | D | `core/prv-learning` | MP#5 |
| `ai-orchestrator` | Task planning, agent registry, conflict resolution | A | `core/prv-ai` | MP#6, MP#19 |
| `export` | Render, encode, metadata embedding, integrity, reports | A | `core/prv-export` | MP#3A, MP#3C |
| `cloud-sync` | Sync state, change detection, conflicts, backup; the wire format lives in `prv-project` with the enum it encodes | A | `core/prv-sync`, `core/prv-project::wire` | MP#24 |
| `plugin-manager` | Manifest, permissions, lifecycle, tiered isolation | A | `core/prv-plugin` | MP#23, ADR-0005 |
| `security` | Authorisation rules, secrets, audit, integrity | A | `core/prv-security` | MP#26 |
| `settings` | Preferences, modes, capability flags | A | `core/prv-settings` | MP#8 |
| `licensing` | Entitlements, tiers, feature availability | A | `core/prv-entitlements` | MP#29 |
| `analytics` | Metrics, diagnostics, telemetry consent | A | `core/prv-telemetry` | MP#8, MP#26 |
| `render` | Turning a project into audio, block-size independent | A | `core/prv-render` | MP#3A, MP#3C |
| `ffi` | The C ABI boundary: handles, statuses, the render entry point | A | `core/prv-ffi` | ADR-0001, MP#4 |
| `notifications` | Notification model, delivery preferences | A | `core/prv-notify` | MP#24 |

### Swift side

| Module ID | Purpose | L | Location |
|-----------|---------|---|----------|
| `prv-bridge` | Generated C header and module map, emitted by `bridgegen` | I | `apple/PRVKit/Bridge/Generated` |
| `prv-core-swift` | Safe Swift over the boundary; imports no Apple framework, so it is tested on Linux | I | `apple/Sources/PRVCore` |
| `audio-host` | CoreAudio render host behind an `AudioOutput` port | I | `apple/Sources/PRVKit` |
| `platform-io` | Decoding and project storage behind `MediaDecoder`/`ProjectStore` | I | `apple/Sources/PRVKit` |
| `secure-store` | Keychain behind a `SecretStore` port | I | `apple/Sources/PRVKit` |
| `net-transport` | Cloud transport, retry, integrity | I | not started |
| `design-tokens` | Generated token definitions | P | `apple/PRVUI/Tokens` |
| `components` | Models and views for the component library of MP#17 | P | `apple/Sources/PRVUI` |
| `spaces` | Home, Library, AI Studio, Mix Editor, Live, Settings | P | `apple/Sources/PRVUI` |

## Entitlements are a thin layer, deliberately

`licensing` sits above the engines and is consulted only at feature boundaries.
No engine knows what tier the user is on. The DSP graph, the planner and the
analysis pipeline behave identically in every edition. MP#29 requires that
essential functionality is never artificially restricted; keeping entitlement
checks out of the engines is what makes that structurally true rather than a
promise, and it is what allows the same core to ship offline, in the free tier
and inside a plugin.

## Module documents

Each module gets a document following [module-template.md](module-template.md)
when it enters *In Design*. Modules not yet in design are listed here and nowhere
else, so the index never implies more exists than does.
