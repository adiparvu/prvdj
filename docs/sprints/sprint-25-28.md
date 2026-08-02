# Sprints 25–28 — What the review found

Four sprints in one document because they belong to one movement: an adversarial
review of everything built so far, and the repairs it produced. Three of the four
defects below were found by probes the review's agents left behind in the working
tree, which were read before they were deleted.

| Delivered | Tests |
|-----------|-------|
| `prv-render` — the project-to-audio path, block-size independent | 12 new |
| `prv-sync::backup` — restore-point retention | 9 new |
| `prv-mix::pacing` — one rule for how far a set advances | 6 new, and 2 cross-checks |
| `prv-project` — `author_all`, `inverses_of`, `Undo::Superseded`, `issued` | 15 new |
| `prv-dsp` — the limiter's three timing bugs, and no allocation in the streaming path | 8 new |
| `prv-waveform::Tile::fold` — energy no longer depends on the order of a span | 3 new |
| — | **933 in total** |

All gates green: `cargo test`, `cargo clippy --all-targets -- -D warnings`,
`cargo fmt --check`, eight architecture rules, the design-token staleness gate,
and the allocation gate.

## Business outcome

**A set is as long as the planner says it is.** It was not. The planner laid
tracks end to end and the renderer started each track at the outgoing track's
*exit point* — where the analysis says a record wants to be left. Two models of
the same clock, in two files, neither aware of the other.

Eight five-minute records whose exit points sit a quarter of the way in produced
a **forty-minute plan and a thirteen-minute mix**, and `duration_error` reported
the set as a perfect match for what the user asked for. That second half is what
made it serious rather than merely wrong: a planner that comes up short can say
so, and the user can accept it or ask for more. A planner that comes up short and
reports success has no way to be caught — and `duration_error` is also the number
the beam search *sorts by*, so the wrong clock was choosing between plans as well
as describing them.

**A month-old version is still there when somebody looks for it.** The obvious
retention rule is "keep the last fifty", and it loses the only version anybody
wanted: a user who worked on a set in March and comes back in June has fifty
automatic points from June and nothing from March. Points are thinned by age band
instead — everything recent, then one an hour, then one a day — so the count
stays bounded and the history stays legible.

**A version somebody named is never deleted.** Somebody typed a name. If the
named points alone exceed the bound, the bound gives way. Master Prompt #9 says
the user owns their work, and a retention policy that deletes something they
deliberately kept is not a policy, it is data loss with a schedule.

**A branch no longer loses work silently.** A branch of the same device
re-allocated the trunk's operation numbers, and the merge then dropped them as
"already present". The user saw a successful sync and an hour of missing edits.

**Undo no longer reverts a collaborator.** It reports who edited afterwards
instead, which is a question the user can answer.

## Architecture review

**No new decision record, and one rule moved.** Everything here is repair rather
than new ground. The one structural change is `prv-mix::pacing`: the rule for how
far a set advances now lives in one function that both the planner and the
renderer call.

A rule that lives in one place can be wrong. A rule that lives in two places will
eventually be wrong in only one of them, which is worse — it looks right from
wherever you happen to be reading. That is the general form of the defect and the
general form of the fix.

**`Goal` now carries its sample rate.** A duration expressed in frames is not a
duration until something says how long a frame is. The goal carried one
implicitly — every caller had a rate in mind when it converted minutes to frames
— and the planner, which had no way to ask, could not work out how long a
transition would overlap for. So it assumed there was none. Requiring the rate is
what closes that off: the question can now be asked, so it is answered.

**`prv-render` keeps ADR-0001 intact.** The renderer computes every position from
the project's own timeline and reads audio through a `Source` port. It decides
which samples; the platform reads them.

**`Frames` is signed, and that is now tested.** Two of the repairs here
(`pacing::advance`, the limiter's window) turned on a subtraction that could go
negative. Both are clamped, and both have a test that says so by name.

## What the tests were doing wrong

Worth recording, because it is the transferable part.

**`tracks_are_laid_end_to_end_without_gaps` asserted the bug.** It walked the plan
and checked that each track started where the last one ended — which is the
arithmetic the planner did, restated. It passed for every fixture in the crate
because every fixture used tracks with no analysis attached, and it would have
gone on passing forever. It is now
`a_set_moves_forward_by_one_handover_at_a_time`, which asserts something weaker
and true, alongside a cross-check that renders the plan and compares the two
lengths.

**A test that builds its fixture out of the code under test cannot fail.** Three
of the four defects here share that shape. The repair in each case was a test that
states the property in the user's terms — *the set is as long as it says it is*,
*the newest point is always kept*, *no name is ever handed out twice* — rather
than in the implementation's.

**`the_user_is_warned_while_there_is_still_room_to_act` measured arithmetic, not
the property.** It filled the outbox to nine tenths of the limit and asserted the
warning; integer division put it four entries below the line, so it was measuring
the fixture. It now fills *until* the warning appears and asserts there is still
room.

## Known limitations

Unchanged from Sprint 24, minus the render path, which is now built:

- Time-stretching with key lock is implemented (`prv-dsp::PitchShift`) but not
  calibrated against reference material; ±6 semitones is a stated bound rather
  than a measured one.
- The loudness meter reports momentary and short-term figures and deliberately
  reports no integrated figure — an integrated measurement over a whole set is
  the exporter's job, not the meter's.
- `prv-mix::pacing::advance` is used by the planner with the overlap left out of
  its energy-target estimate, because the overlap comes from a score the loop has
  not computed yet. The estimate is short by at most one overlap — seconds, in a
  set measured in hours — and the placement itself uses the exact figure.
- The Apple layer remains uncompiled here (see
  [17-known-limitations.md](../mts/17-known-limitations.md), R-01).
