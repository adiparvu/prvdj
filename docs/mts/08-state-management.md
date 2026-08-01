# 8. State Management

Status: **Partially fixed** by ADR-0002 and ADR-0003; presentation-layer
patterns scheduled for Phase 1.

## Where state lives

| State | Owner | Mechanism |
|-------|-------|-----------|
| Playback position, meters, transport | audio thread | published through a wait-free triple buffer (ADR-0002) |
| Project content | operation log | append-only; interface state is a fold over it (ADR-0003) |
| Library index | core | queried, never mirrored into view models |
| Ephemeral interface state | presentation | selection, scroll, disclosure — never business rules |

## Rules

1. **The interface never mutates domain state directly.** Every change is an
   operation appended to the log. This is what makes undo, branching and
   synchronisation fall out of one mechanism instead of three.
2. **The interface never polls the audio thread.** It reads the most recent
   published snapshot. Two subsystems computing position independently would
   eventually disagree, and a disagreement of one sample is a visible
   misalignment.
3. **No business logic in view models.** Master Prompt #4 makes this a hard
   layering rule; the language boundary of ADR-0001 makes it structural, since
   the rules live in a different language entirely.
