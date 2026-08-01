# Coding Standards

Derived from Master Prompts #4, #11, #13, #14, #15 and #31. These are the rules a
change is reviewed against.

## The order of values

When two of these conflict, the earlier one wins. This ordering is from Master
Prompt #15 and it is not a preference list — it is how disagreements are settled.

1. Correctness
2. Reliability
3. Simplicity
4. Maintainability
5. Performance
6. Scalability
7. Developer experience
8. Visual polish

Visual polish sitting last does not make it unimportant. It means an animation
never costs audio stability. The beauty this product needs comes from the first
seven done impeccably: an interface that is correct, reliable, simple and fast
*is* an interface that feels premium.

## Before writing code

Master Prompt #13 requires this sequence, and it is not optional:

1. Understand the problem.
2. Understand why the feature exists.
3. List multiple possible implementations.
4. Compare them.
5. Choose the best long-term architecture.
6. Design reusable components.
7. Only then implement.

Any decision that is expensive to reverse, constrains other modules, or would
make a future engineer ask "why is it done this way?" gets a record in
[docs/adr](../adr/README.md) before the code lands.

## Non-negotiables

**No placeholder code.** No `TODO`, no `unimplemented!()`, no stub that returns a
plausible value. Enforced by `tools/check-architecture.sh`. A marker left in the
tree is a claim of completeness the code does not honour.

**Nothing is reported as working before it is measured.** Code that has not been
compiled and tested is *authored*, not verified, and is recorded as such in the
implementation status board. This is the single easiest way to lose a reader's
trust and it is not worth the momentary convenience.

**Every public item is documented.** Enforced by the compiler. Documentation
explains *why*, not what — the code already says what.

**Panic-capable constructs are denied in the core.** No `unwrap`, `expect`,
`panic!`, unchecked indexing, or integer division without an explicit, justified
allow. Any function reachable from the audio callback must not panic, and the
only way to guarantee that across a growing team is to forbid the constructs
everywhere and require a written reason at the few sites where the alternative is
genuinely less clear.

**Unsafe code lives in one crate.** `prv-rt` only, with a safety argument on
every block. Enforced by `tools/check-architecture.sh`.

**The core performs no I/O.** No filesystem, network, process or environment
access. Everything enters through ports. Enforced.

## The audio thread

Anything reachable from `process` obeys [ADR-0002](../adr/0002-realtime-audio-core.md):
no allocation, no locking, no waiting, no syscalls, no panics, bounded time.

Before merging a change that touches the render path, confirm:

- [ ] No allocation, including `Vec` growth, boxing, formatting and closure capture
- [ ] No destructor runs on the audio thread
- [ ] No lock, channel, or unbounded spin
- [ ] Work per block depends only on frame count and active node count
- [ ] Every continuous parameter is smoothed, not stepped
- [ ] The allocation test still passes in release mode

## Tests

**Positive controls for anything that measures.** A harness that has silently
stopped measuring lets the real assertion pass for the wrong reason. The
allocation test is preceded by a test that deliberately allocates and asserts the
counter noticed.

**Ground truth over self-consistency.** Check against an external source where one
exists. A test that confirms the implementation agrees with itself proves
nothing.

**Failure messages state the product consequence.** "A single allocation in a
2.7 ms callback is an audible dropout" tells the next engineer what broke.
"Expected 0, got 1" does not.

**Prefer properties to examples** where the property is what you actually mean.

## Naming and structure

- Files small and focused. A file doing two things becomes a file doing five.
- Composition over inheritance; interfaces over concrete types.
- Names say what the thing is for, not what it is made of.
- Comments explain the decision, not the syntax. A comment restating the code is
  noise that will drift out of date.

## Definition of done

From Master Prompt #13 and #30. A change is complete only when all of these are:

- [ ] Architecture reviewed, with a decision record if the decision is significant
- [ ] Implemented, with no placeholder
- [ ] Tested, and the tests pass
- [ ] Performance validated against a stated budget, or the budget is stated as
      pending with a phase
- [ ] Documentation updated in the same commit — including diagrams and the
      dependency map if they moved
- [ ] Accessibility verified, for anything with a user-facing surface
- [ ] Security reviewed
- [ ] No critical technical debt introduced
- [ ] Implementation status board updated honestly

## After every change

Ask the four questions from Master Prompt #15: is it simpler, faster, safer,
easier to understand and maintain? If the answer is no, improve it before moving
on. Debt is not repaid later; it is repaid now or it compounds.
