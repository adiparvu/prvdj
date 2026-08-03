# 12. Cloud

Governed by Master Prompt #24. Model fixed by
[ADR-0003](../adr/0003-project-document-and-persistence.md).
Status: **Partial** — the protocol is built and tested; the transport is not.

## What exists

The wire format and the four boundary calls that use it. `prv-project::wire`
encodes and decodes messages; `prv_engine_sync_state`, `prv_engine_sync_prepare`,
`prv_engine_sync_outbound` and `prv_engine_sync_merge` let a host exchange them.

The protocol is two messages and three steps: a device sends its version vector,
the other side answers with the operations that vector has not seen, and each
merges what it received. Both may do it at once and neither is authoritative.

The synchronisation state machine crosses the boundary too — `prv_sync_create`
and its five companions — because "connected" is the smallest part of what that
state means. Editing is permitted in every state, a conflict stops the transfer
and nothing else, and a pause is lifted only by the user: rules a host would
otherwise reimplement, differently, eventually.

Above it, `PRVUI` holds the two models a person actually meets: a consent screen
that tells both truths about what leaves the device, and a status model in which
offline is a state rather than an error.

What does not exist is anything that moves bytes. That is ADR-0001 working rather
than a gap — the core decides and the host acts — and it is why the same four
calls serve a cloud service, a local network, a memory stick and a file attached
to an email without knowing which is which.

Three properties are worth naming because they were designed rather than
inherited:

- **A build that cannot read an operation still carries it.** Unrecognised
  entries are written into the log byte for byte, so they survive being closed
  and reopened and are re-emitted in every message afterwards. An install a
  version behind is a relay rather than a hole in the fleet, and stays one. After
  an upgrade they are re-read, and the ones the new build understands become
  ordinary parts of the project.
- **Nothing half-understood is ever applied.** An entry carrying a payload this
  build knows plus a field it does not is treated as not understood, rather than
  stored stripped of its author's meaning and relayed as though it were theirs.
- **Untrusted bytes cannot make the core allocate.** Every count is checked
  against the bytes actually present before anything is reserved.

The format's limits are recorded in
[17-known-limitations.md](17-known-limitations.md), sections 6 and 7.

## Decisions already fixed

- **Offline-first is structural, not a mode.** Every project exists locally and
  is fully editable, analysable, mixable and exportable with no network.
- **Synchronisation ships operations, not documents.** A project's entire editing
  history is typically kilobytes, so it reconciles in seconds on a poor
  connection in a venue. Media is gigabytes and transfers selectively in the
  background. That asymmetry is what makes cross-platform sessions and future
  real-time collaboration practical.
- **Conflicts are surfaced, never resolved silently.** Operations that commute
  merge automatically; those that do not are presented with both intentions
  shown.
- **Sharing a project shares the document, not the audio.** A collaborator sees
  the complete timeline; tracks they do not own appear as missing, with an option
  to supply them from their own library. Replicating media on share would turn
  synchronisation into a distribution channel, which Master Prompt #15's ethics
  principle and Master Prompt #29's copyright principles both forbid.
- **Cloud failure never degrades local work.** Synchronisation is an enhancement
  and is suspended entirely during live performance.
