# 11. Timeline

Governed by Master Prompt #21. Status: **Not started** — Phase 1 and 2.

## Decisions already fixed

- **One uniform object model.** Tracks, transitions, markers, cue points, loops,
  automation, effects, tempo changes, notes and regions are all timeline objects
  with identity, position, duration, layer, metadata, version and history.
  Video, lighting, MIDI, lyrics and spatial audio extend the model later
  **without changing existing objects**.
- **Every edit is an operation on the project log** (ADR-0003). Undo, branching,
  named versions and comparison are consequences of that, not features.
- **Snapping includes the phrase.** DJs think in groups of eight and sixteen
  bars, not in individual bars; a snap system without phrases is a snap system
  professionals will switch off.
- **AI overlays are optional and dismissible**, satisfying Master Prompt #8's
  rule that the interface never intervenes uninvited for a professional.

## Accessibility: designed now, not retrofitted

A timeline is a continuous spatial canvas with overlapping objects on nine
lanes. VoiceOver over such a surface cannot be added afterwards.

The navigation model is structural rather than spatial: the user moves by
*musical meaning* — next transition, next drop, next automation point, next
marker — within a chosen lane, and hears position announced in bars and beats
rather than in pixels. Every announcement is derived from data the analysis
pipeline already produces (Master Prompt #20), so this costs a presentation
model, not a new subsystem.
