# 5. Design Tokens

Master Prompt #16 states the rule without qualification: never hardcode visual
values, and no screen may bypass the token system.

## Where the truth lives

`design/tokens/tokens.json` is the single source. Everything else is generated.

```
design/tokens/tokens.json                          ← edited by humans
        │
        └─ tools/tokengen ─→ apple/PRVUI/Tokens/Generated/DesignTokens.swift
```

The generated Swift carries **no framework imports**. That is a deliberate
choice with a practical payoff: it compiles on the Linux runners that gate every
pull request, so a token change is verified without needing scarce macOS
capacity. The mapping from these values to `Color`, `Font` and `Animation` is a
small hand-written adapter in the presentation layer.

## Why generated files are committed

An Xcode build must not require a Rust toolchain to produce the tokens it
compiles against. The generated file is therefore checked in, and drift is
prevented by a gate rather than by discipline: continuous integration runs the
generator with `--check` and fails if the committed file differs from a fresh
generation. Hand-editing a generated file fails the build.

## Groups defined so far

| Group | Contents |
|-------|----------|
| `color` | 20 semantic colours, each with a light and a dark appearance |
| `spacing` | Nine-step four-point scale |
| `radius` | Seven-step corner scale |
| `typography` | Nine text styles, each mapped to a platform text style for Dynamic Type |
| `motion` | Five intent-named durations and four curves |
| `elevation` | Four shadow definitions |
| `opacity` | Five named opacities |
| `blur` | Three material radii |

## Groups deliberately not yet defined

Master Prompt #16 names twenty-four groups. Six remain undefined: icons,
illustrations, charts, borders, controller colours and accessibility colours.

They are declared as pending in the token source rather than invented. A token
defined before anything consumes it is a guess, and a guess that has shipped is
harder to correct than an absence. Each lands with the first screen that needs
it.

## Two decisions worth recording

**Colour never carries meaning alone.** Master Prompt #2 asks for reactive,
artwork-driven visuals; Master Prompt #8 and #11 make colour-safe visualisation
non-negotiable. These pull against each other only if colour is the sole carrier
of information. Every semantic colour is therefore paired with an icon, a label
or a texture in the component that uses it — a clipping indicator is a colour
*and* a persistent marker, never a colour alone.

**Motion degrades rather than disappears.** Each motion token has a
reduced-motion form with zero duration. Under reduced motion the state change
still happens and is still communicated; it simply arrives without travel. This
lets Master Prompt #2's animated waveforms and reactive backgrounds coexist with
Master Prompt #17's accessibility contract instead of being an exception to it.

**Numbers that change while being read use tabular figures.** The `numeric` and
`mono` styles set `monospacedDigits`. Without it, a BPM readout shifts
horizontally on every update, which reads as instability in exactly the values a
performer is trusting.
