# Sprint 23 — Synchronisation and notices

Master Prompt #24 from both ends: your work reaches your other machine, and you
find out about it — at a moment that is not the middle of a set.

| Delivered | Tests |
|-----------|-------|
| `prv-sync` — state, outbox. `prv-notify` — notices, coalescing | 28 new; 856 in total |

All gates green: `cargo test`, `cargo clippy --all-targets -- -D warnings`,
`cargo fmt --check`, `cargo doc`, eight architecture rules, and the design-token
staleness gate.

## Business outcome

**Six hours offline costs nothing.** Two thousand edits queue, the network comes
back, everything travels, and no state in between could stop the user working.
There is a test that plays exactly that session.

**A pause means now.** Somebody on a metered connection who switched
synchronisation off gets no background transfer, no "just this small one", and no
automatic resumption when the signal improves. Only they lift it.

**Three things may interrupt a set.** A plugin that has been passed over, an
audio device that has gone, a file that has stopped reading — each is something
the person on stage is about to hear and can act on. A synchronisation conflict
is urgent to the system and is not urgent to a human standing in front of an
audience.

**Nothing withheld is thrown away.** What waits during a performance is released
afterwards, with a count where a count means something.

## Architecture review

**`prv-sync` is deliberately small, and the reason is a compliment to Sprint 3.**
The hard part of synchronising — deciding whether two edits were made in sequence
or in parallel, merging them, reporting what genuinely conflicts — is
`prv-project`'s, and was settled three sprints before there was a network to
have. What was missing was everything *around* it, and that is what this crate
is.

**The outbox refuses when full; the audit log discards its oldest.** Same
problem, opposite answers, and the difference is the point. An audit entry is a
record of something that happened, so losing the oldest costs history. An outbox
entry is the user's work, and Master Prompt #9 forbids discarding that — an
outbox that dropped its oldest entry would silently delete the first hour of a
long offline session, most reliably for whoever was working hardest. The bound is
back-pressure, not a retention policy, and both crates now say so in their own
words.

**The state machine is written as precedence rather than as a table.** The
interesting content *is* the precedence — what outranks what when two things are
true at once — and a match table hides that behind arm ordering. Reading it top
to bottom now gives the rules in the order they win: the user's pause, then the
network, then an unanswered question, then transfer.

**`prv-notify` decides interruption in one place.** Master Prompt #8's rule that
the interface never intervenes uninvited for a professional and Master Prompt
#19's rule that audio performance comes first meet in exactly one function. Every
call site obeys it rather than each having an opinion.

Twenty-two crates, acyclic.

## Findings raised on my own work

**A test measured my arithmetic instead of the property, and failed.** The
"warn while there is still room" test filled the outbox to a computed threshold
— `LIMIT * 9 / 10` — and asserted the warning had appeared. Integer division put
it four entries below the line. The fix was not to adjust the constant: it was to
fill *until the warning appears* and assert that happened before the refusal,
which is the property the product needs and does not depend on getting the
rounding right in two places.

**`exactly_three_things_may_interrupt_a_set` names them individually.** A test
that counted them would pass when a fourth was added and a fifth removed. Naming
them means adding one is a change somebody argues for rather than one that slips
in alongside a feature.

**A question is never coalesced.** The first `release` merged every repeated
notice with a count, which for two plugins asking for permission would have
produced "2 permission requests" — one line, one answer, one plugin left
unanswered. Questions are now released individually, and `asks_a_question` is
what separates them.

## Security and privacy review

**Synchronisation is a purpose that starts withheld.** This crate does not gate
itself — `prv-security` holds the agreement and the layer that opens a socket
consults it — but the test at the crate root asserts the purpose exists, starts
withheld, and is marked as sending the user's own content.

**A notice carries no content.** `Notice` is a kind and a count. Which track
failed to analyse, which file stopped reading, which plugin asked — all of that
belongs to the interface that renders it, from data it already has.

## Performance review

Nothing on a hot path. The outbox is a sorted set with logarithmic insertion and
a bounded size; notifications are a map over a twelve-entry vocabulary.

## Accessibility review

Two decisions in its favour. Every notice, state and event carries a stable key,
so an interface localises without the core knowing any language. And coalescing
with a count produces one announcement rather than forty — which is a
convenience with a screen and the difference between usable and unusable with a
screen reader.

## Are the project's principles still respected?

| Principle | Verdict |
|-----------|---------|
| Nothing is lost (MP#9) | Held twice: an outbox that refuses rather than forgets, and notices that wait rather than vanish. |
| Offline is normal, not a fallback (MP#24) | Held. Editing is allowed in every state, checked over the whole machine. |
| Audio performance first (MP#19) | Held. Three notices, named. |
| The interface never intervenes uninvited for a professional (MP#8) | Held, and decided in one function. |
| Never the easy solution when a premium one exists (MP#1) | Held. Precedence rules rather than a table; questions exempted from coalescing. |
| Quality is not a phase (MP#27) | Held. 856 tests; three findings within the sprint; review in the same commit, fifth sprint running. |

## Known limitations

1. **No transport.** Nothing opens a socket, sends bytes, or retries. That is the
   platform's, by ADR-0001.
2. **No backup or restore points.** Master Prompt #24 asks for them.
   `prv-project` already makes them expressible — a named version is a position
   in the log — so what is missing is the policy about how many to keep and when,
   which is a decision better made with a real storage cost in front of it.
3. **Conflict resolution is surfaced, not assisted.** `prv-project` reports what
   conflicts and refuses to discard either side; nothing yet helps a user choose
   between them. That is an interface problem more than a core one.
4. **A notice has no identity.** Two permission requests are two entries and the
   caller cannot tell which is which, because `Notice` deliberately carries no
   payload. When the interface layer exists it will need a correlation
   identifier, and adding one there is better than putting a plugin reference in
   the core's notice vocabulary.
5. **`Notifications` holds no ordering between kinds.** Release order is the
   vocabulary's order, not the order things happened, because there is no clock.
