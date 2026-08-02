# Sprint 22 — Telemetry

Master Prompt #8 wants to know what is used; Master Prompt #26 governs what may
be learned about a person. `prv-telemetry` is the resolution, and it comes down
to one decision made differently from the usual.

| Delivered | Tests |
|-----------|-------|
| `prv-telemetry` — events, counts, diagnostics | 21 new; 828 in total |

All gates green: `cargo test`, `cargo clippy --all-targets -- -D warnings`,
`cargo fmt --check`, `cargo doc`, eight architecture rules, and the design-token
staleness gate.

## Business outcome

**A user who agreed to nothing leaves no trace anywhere.** Not "we hold it and do
not send it" — there is nothing held. The buffer does not exist, so there is
nothing for a future bug to release.

**A user who agreed to something can see exactly what that is.** The report a
user is shown is the same structure that would be sent, rendered through the same
module. A separately written page describing "what we collect" is a document that
drifts from the code within two releases.

**Withdrawing an agreement takes the data with it.** A cleared record is
indistinguishable from one that never counted anything, which is what makes
"withdraw" the word for what it does.

## Architecture review

**No new decision record.** The crate applies ADR-0001 and `prv-security`'s
consent model. It introduces no new concept — every purpose it uses already
existed and was already withdrawable.

**Recording is gated, not sending.** This is the sprint's whole substance. The
common shape — collect everything, gate the upload — is defensible in a diagram
and indefensible in practice: keeping a person's behaviour and deciding later not
to send it is still having kept it. Here `record` is a no-op without the
agreement.

**One number per event kind, and nothing else.** No order, no times, no session.
A sequence of timestamped events is a record of one person's evening; the same
events as totals answer "is anybody using this" and answer nothing else — and
that is the question the product actually has. There is a test that runs the same
events in two orders and asserts the results are equal, because the property is
"the order did not survive", not "we did not write a timestamp".

ADR-0001 keeps the core away from the clock, which makes the wrong version
unwritable here rather than merely discouraged. That is a coincidence and a good
one, and the documentation says which it is.

**Two agreements, never one standing in for the other.** A crash report is
offered to get something fixed; a usage count helps decide what to build.
`Event::is_a_fault` is the split, and `every_event_belongs_to_exactly_one_agreement`
checks that the routing agrees with it over the whole list.

**The secret check comes first, and the order is documented as deliberate.** A
reader of `may_be_sent` should not have to verify that a later rule cannot
override it.

Twenty crates, acyclic.

## Security and privacy review

This sprint is a privacy review, so what is worth recording is where the
guarantees come from.

**Structural.** An `Event` has no payload — not a track, not a path, not a
duration. A variant that gained one would break the test that copies every event
by value. The counts are a map from a closed enumeration to integers, so the
memory cost is bounded by the number of *kinds* and not by how much the product
is used.

**Checked.** `a_report_can_hold_nothing_a_person_could_be_recognised_by` walks
every field of a full report and asserts each is public, rather than reading the
constructor — because the constructor is what a later change edits.

**Refused rather than trimmed.** A diagnostic that names the user's own files is
withheld, not stripped and sent. Trimming would mean sending something the author
did not write and nobody reviewed.

**Never, at any setting.** A report containing credential material is refused
whatever has been agreed to. A user cannot consent on behalf of the service that
issued a token.

## Findings raised on my own work

**I had personal fields riding on the crash-report agreement.** The first
`may_be_sent` allowed a `Personal` record through on `CrashDiagnostics`, on the
reasoning that a crash report is expected to contain some context. That is the
reasoning every product uses on the way to uploading a file listing. Agreeing to
send crash reports is agreeing to send *reports*; anything naming the user's own
files needs its own answer, and `PersonalWithoutAgreement` is now a distinct
outcome with a distinct question behind it. `prv-security` has no purpose for
that answer yet, and the comment says so rather than implying one exists.

**`purpose_for` is a function of the event, not a field on it.** Storing the
purpose on each variant would have let the two lists drift; deriving it means the
test that checks the routing is checking one thing rather than two.

## Performance review

Nothing measurable. A record is a map insert into a structure with at most
fourteen keys; a report is a walk over the same.

## Accessibility review

No surface. Every event, purpose and refusal carries a stable key, and a
withholding says whether asking the user could change the answer — so a consent
prompt is never offered for something no answer would unlock.

## Are the project's principles still respected?

| Principle | Verdict |
|-----------|---------|
| Privacy by design (MP#26) | Held, and in the strong form: nothing is recorded rather than nothing is sent. |
| The user owns their data (MP#9, MP#26) | Held. Withdrawal discards; a cleared record equals a fresh one. |
| Never the easy solution when a premium one exists (MP#1) | Held. Gating collection is harder than gating upload and is the only version that means anything. |
| Errors explain (MP#10) | Held. Three distinct reasons, one of which says asking will not help. |
| Production-ready, never placeholder (MP#13) | Held. |
| Quality is not a phase (MP#27) | Held. 828 tests; the review in the same commit, fourth sprint running. |

## Known limitations

1. **Nothing sends anything.** The crate decides what may be sent; a transport
   needs a network, which ADR-0001 puts outside the core.
2. **There is no agreement covering personal detail in a diagnostic.** So today
   any such report is withheld. That is the safe direction and it is a real gap:
   some faults are genuinely easier to diagnose with a path, and the honest fix
   is a purpose in `prv-security` with its own prompt — not a widening of an
   existing one.
3. **Counts have no epoch.** They accumulate for the life of the record and there
   is no notion of "since the last upload", because that needs a clock or a
   caller-supplied ordinal, and no caller exists yet to supply one.
4. **Nothing records anything yet.** Like the last several sprints, the callers
   are in layers this environment cannot compile.
