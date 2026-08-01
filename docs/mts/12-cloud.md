# 12. Cloud

Governed by Master Prompt #24. Model fixed by
[ADR-0003](../adr/0003-project-document-and-persistence.md).
Status: **Not started** — Phase 5.

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
