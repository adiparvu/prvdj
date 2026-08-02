# Sprint 19 — Plugins

ADR-0005 was written in Sprint 0 and has been waiting for the crate that makes it
true. `prv-plugin` is the policy half of it: tiers, manifests, lifecycle, the
watchdog, and the registry that ties them together.

| Delivered | Tests |
|-----------|-------|
| `prv-plugin` — tier, manifest, lifecycle, watchdog, registry | 44 new; 752 in total |

All gates green: `cargo test`, `cargo clippy --all-targets -- -D warnings`,
`cargo fmt --check`, `cargo doc`, eight architecture rules, and the design-token
staleness gate.

## Business outcome

**A plugin can fail during a set and the set continues, in time.** That sentence
has two halves and the second is the one products get wrong. Everyone thinks
about the crash; almost nobody thinks about latency compensation shrinking when
the failed plugin leaves the chain, which pulls the whole channel forward against
the beat. A plugin failing would knock the set out of time — worse than the
failure itself. The registry counts every slot that exists, running or not.

**A permission prompt means what it says.** The manifest says what a plugin
wants; the registry holds what the user approved; every question is answered from
the second, and approval is intersected with the request so the host cannot grant
something the user never read in a prompt. A plugin that asked for more than it
got can say so, which is the state a user needs to make sense of a plugin that is
installed, running, and not doing what it was installed for.

**A marketplace can accept third-party effects.** The point of ADR-0005's tiering
was that submissions stop being a stability risk to live performers. This sprint
is the part of that which can be tested without a runtime.

## Architecture review

**No new decision record.** The crate is ADR-0005 implemented. Where it went
beyond the record, it did so in ways the record implies rather than contradicts,
and each is documented in the source.

**The lifecycle's guarantee is a property, not a path.** `audio_behaviour`
answers *processes* or *passes through* for every state, and there is no third
answer, so there is no event whose outcome is silence. The test walks every state
against every event rather than following the interesting route. That is the
difference between "we handled the crash case" and "there is no case we did not
handle".

**Four journeys converge on `Loaded`.** First load, supervisor restart, ordinary
stop, and being switched back on all arrive at the same state. That is
deliberate: recovery is not a special path, it is the ordinary path taken again,
which is why a recovered plugin must be started like any other rather than
resuming mid-block. Merging the arms made the design visible; it was three
separate arms with the same body until clippy pointed at them.

**Authorisation is asked, never answered here.** `Registry::authorise` delegates
to `prv-security`. Master Prompt #26 requires one decision point, and a plugin
manager that answered permission questions itself would be the second — the exact
failure the centralisation rule exists to prevent.

**The watchdog is written for the audio thread even though this crate is not on
it.** Total, allocation-free, no unbounded loops; the history is a 64-bit word
shifted once per block, which is why the window is 64 rather than a rounder
number. The shape of the data structure is the reason the measurement costs
nothing.

Eighteen crates, acyclic. `prv-plugin` is the second crate to depend on another
core crate, and it depends on the right one.

## Security review

**A manifest is a claim, and the code says so in its name.** `Manifest::read`
rather than `Manifest::new`: everything in it came from outside the process.

**Forbidden authority is narrowed, not refused.** A manifest asking for the
user's credentials is not an error to reject the plugin over — refusing the whole
plugin would let a hostile manifest deny service by asking for something it knows
it cannot have. The bits are dropped and what remains is what the plugin gets.
The same claim is refused again at the question, by `prv-security`, and the
redundancy is deliberate.

**Untrusted is treated as worse than unsigned.** Both refuse to load without
approval, and they are separate states because an interface should say them
differently: nobody signed this, versus somebody went to the trouble of making
this look signed.

**Revocation applies from wherever the plugin is.** A plugin that had to be
stopped before it could be revoked would be one whose removal a wedged instance
could refuse. It is also terminal — revocation that the revoked thing can undo is
not revocation.

**An uninstalled plugin asking a question is refused as if it held nothing.**
That is a bug or an attack, and neither is a reason to answer yes.

## Findings raised on my own work

**A plugin off the audio path was going to be believed about its latency.** The
first version stored whatever the manifest declared. A metadata provider is not
in the signal chain, so compensating for its claimed 512 frames would delay
everything else in the project to line up with something that is not in it. Now
the latency of an off-path plugin is zero regardless of what it says.

**One overrun rule was not enough, and I only noticed by writing the adversarial
case.** Three-in-a-row is the obvious reading of "repeatedly", and a plugin that
overruns every third block never triggers it while producing a dropout four times
a second. The second rule — eight in the last sixty-four — exists because of that
test, not the other way round.

**The bypass is reported exactly once.** The first version returned the verdict
on every block after the threshold, which would have produced a notification
every five milliseconds for the rest of the set. Caught by asking what the caller
would do with the answer.

**A recovered plugin keeps its lifetime record.** Clearing everything on restart
was the simpler implementation and would have let a badly behaved plugin look new
every few minutes. The recent window resets; the totals and the bypass count do
not.

## Performance review

Nothing measurable. The watchdog is a shift, an or, and a population count per
block per plugin. The registry's latency sum is linear in the number of installed
plugins and runs when the graph is rebuilt, not per block.

## Accessibility review

No surface. One decision in its favour: every tier, state, event, reason and
signature status carries a stable key, so a permission prompt or a failure notice
is localised by the interface without the core knowing any language.

## Are the project's principles still respected?

| Principle | Verdict |
|-----------|---------|
| Never the easy solution when a premium one exists (MP#1) | Held. Two overrun rules; slot latency rather than running latency; narrowing rather than refusing. |
| Reliability above performance (MP#15) | Held — it is the substance of ADR-0005 and of this crate. |
| A crash never stops the music (MP#23) | Held, as a property over the whole state machine. |
| One decision point for authorisation (MP#26) | Held. The registry asks `prv-security` and obeys it. |
| Production-ready, never placeholder (MP#13) | Held. What is not built is named as not built, not stubbed. |
| Quality is not a phase (MP#27) | Held. 752 tests; four findings raised and fixed within the sprint; review in the same commit. |

## Known limitations

1. **Nothing runs a plugin.** No WebAssembly runtime, no child process
   supervisor, no dynamic loading. All three are input and output, which ADR-0001
   puts outside the core, and all three are Phase 6.
2. **No signature verification.** The crate records what the platform found. A
   trust root and a cryptographic implementation belong where the keychain is.
3. **Execution metering is a policy without a meter.** The watchdog decides what
   to do with a cost; measuring the cost needs a clock, which the core does not
   have. The host supplies both numbers.
4. **The plugin parameter surface is not modelled.** A plugin's own parameters
   need to reach the automation system, which means `prv-project`'s
   `ParameterOwner::Effect` needs a plugin-shaped host. That is a real piece of
   work and it belongs with the runtime rather than ahead of it.
5. **Latency compensation is reported, not applied.** The registry says what the
   graph owes; `prv-dsp` will have to spend it. Wiring the two together needs the
   plugin node type, which needs the runtime.

## Definition of done

| Criterion | Status |
|-----------|--------|
| Architecture reviewed | Yes — ADR-0005 implemented, no new record needed |
| Implemented, no placeholders | Yes; what is absent is named as absent |
| Tests passing | Yes — 752 |
| Performance validated | Watchdog is constant-time and allocation-free; nothing else is per block |
| Documentation updated | Yes, in the same commit as the code |
| Accessibility verified | No surface; stable keys throughout |
| Security reviewed | Yes — five properties named, each with the reason it is shaped that way |
| No critical technical debt introduced | Five recorded limitations, all of them "needs the runtime" rather than "needs rework" |
