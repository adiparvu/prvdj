# 6. SwiftUI Components

Governed by Master Prompt #17. Status: **Not started** — scheduled for Phase 1,
after the portable core can supply the data the components display.

## Decisions already fixed

- **The library is the only sanctioned source of interface elements.** A screen
  that needs a new reusable pattern adds it to the library before using it.
  Duplicated interface logic is a review failure, not a shortcut.
- **Eight packages with hard boundaries**: Foundation, Theme, Components, Audio,
  Charts, Timeline, Inspector, AI, Navigation, Feedback, Experimental. Packages
  are feature-independent; a component may not know which screen uses it.
- **Every component ships with eleven artefacts** (Master Prompt #17): purpose,
  public interface, previews, accessibility support, animation behaviour, error
  and loading states, documentation, unit tests, snapshot tests, performance
  notes.
- **Twelve states are mandatory per component** — default, focused, hovered,
  pressed, selected, disabled, loading, error, success, empty, offline, syncing.
  Leaving a state undefined is what produces the dead screens Master Prompt #2
  forbids.
- **Accessibility includes value, not only label.** For a fader, a knob or a
  crossfader, the announced *value* is the difference between a usable control
  and a decorative one.
- **No component hardcodes a visual value.** Everything resolves through the
  generated tokens (section 5).

## The hard problem, recorded early

Timeline and waveform components present a continuous spatial canvas with
overlapping objects on nine lanes. VoiceOver cannot be added to that afterwards;
it needs a navigation model of its own, structured by lane and by musical
section rather than by pixel. The data required already exists — it is the
structural analysis of Master Prompt #20. Section 11 records the design.
