# Sprint 18 — Preferences

Master Prompt #8's interface requirements, as the part of them that is not an
interface: experience modes, the settings document, and the accessibility join.

| Delivered | Tests |
|-----------|-------|
| `prv-settings` — modes, settings, document, accessibility | 37 new; 708 in total |

All gates green: `cargo test`, `cargo clippy --all-targets -- -D warnings`,
`cargo fmt --check`, `cargo doc`, eight architecture rules, and the design-token
staleness gate.

## Business outcome

**A professional mode that is quiet rather than reduced.** Master Prompt #8 asks
for an interface that never intervenes uninvited for a professional. The usual
implementation hides things, which turns a preference into a downgrade: the user
picks "professional" for unrelated reasons and discovers six months later that a
feature they needed was behind it. Here a mode changes only how much the product
volunteers, and every setting outside assistance answers identically at every
mode. The professional's product is the same product with less arriving on its
own.

**Preferences that survive two machines.** Someone with a laptop and a studio
machine runs two versions of the product against one settings document. Until
this sprint that was a data-loss bug waiting to be written; unrecognised entries
are now kept verbatim and written back, so the older build stops deleting the
newer build's preferences every launch.

**Accessibility that cannot be argued with.** A user who set "reduce motion" in
the operating system has already answered, for a reason they should not have to
explain. Nothing in the settings document can withdraw that. The reverse
direction stays open — someone whose system setting is off may still want less
motion here — so the effective value is the union and never the intersection.

**The per-user state finally has somewhere to live.** `prv-security`'s consent
record and `prv-learning`'s profile are both per user rather than per project, so
neither belonged in the operation log, and until this crate existed neither had
anywhere else to be. This was the most concrete gap recorded at the end of
Sprint 17.

## Architecture review

**No new decision record.** The crate applies ADR-0001 (the document is defined
here, the file is written by the platform) and ADR-0003's discipline about
forward compatibility, in a place ADR-0003 does not itself reach.

**Only choices are stored.** A setting the user has not touched is absent rather
than written at its default. The difference is invisible until a default changes,
and then it is the whole thing: a document full of defaults freezes every user on
the values current the day they first ran the product, and nobody can tell which
of those values they actually chose. `clear` therefore returns a setting to
*following the mode*, not to the value it happened to have — "reset" and "stop
having an opinion" are different operations and only the second one is useful
here.

**The mode rule is mechanical, and checked in both directions.** Only
`Category::Assistance` settings may vary with the experience mode. One test says
a setting that varies must be assistance; the other says an assistance setting
that does *not* vary is a gap, because it is one the mode was supposed to reach
and does not. A third checks the same property through the document rather than
through the key, so the rule holds for the composed behaviour and not only for
the declaration.

**This crate depends on neither security nor entitlements, and must not.**
Capability is `prv-security`'s question and payment is `prv-entitlements`'. A
preferences screen that could remove capability would be a place where a user
quietly disables something they will need later and will never connect to a
choice they made today.

**Accessibility state is not persisted.** The platform's answers arrive at launch
and change while the application runs. A stored copy would be wrong every time
the user changes their mind in system settings, and wrong in the direction that
favours *less* accessibility — which is the direction that matters.

Seventeen crates, acyclic, no runtime dependencies.

## Findings raised on my own work

**I nearly made the experience mode a setting like any other.** It cannot be:
it is the thing other defaults are computed from, so it would have needed a
default of its own that could not depend on itself. It is a field on the
document, and the asymmetry is documented rather than left to be rediscovered.

**The first `default_for` had assistance settings return fixed values with the
mode "applied later" by the caller.** That would have put the rule in every call
site — which is exactly the failure mode `prv-security` exists to avoid for
authorisation. The mode is consulted in one place, in the document's `get`, and
the rule about which categories may consult it is checked by a test rather than
observed by a reader.

**`follows_the_mode` is computed, not declared.** It compares the defaults across
every mode rather than asking the author to remember to mark a setting. A
declared flag would have been one more thing to get out of step, and the
computed version is what makes the two-directional test meaningful — it measures
the behaviour rather than the annotation.

**Refusing to keep an unrecognised setting is reported, never silent.** The
bound exists so a malformed writer cannot grow the document without limit, and a
bound that silently drops entries would make "we keep what we do not understand"
true only when convenient. A caller can now say that a document was not preserved
whole.

## Accessibility review

This sprint is largely an accessibility review, so what is worth recording is
where the limits come from.

**The text-scale ceiling is taken from the platform's own accessibility sizes,
not from what the layout is comfortable with.** A limit set by the layout tells a
user with low vision that their need is an edge case. The layout will have to
cope; that is the correct order of obligations.

**An impossible reported scale becomes the default rather than an error.** If a
platform reports a nonsensical value, the user did nothing wrong and the remedy
is not theirs. Clamping is right here in a way it is not right for a value the
user typed.

**Every accommodation is a union in the same direction.** Written as one property
and tested as one property, so a fourth accommodation cannot be added with the
polarity accidentally reversed.

## Security review

Nothing new. No input, no output, no secrets, no network. One property is worth
naming: `clear_all` — "reset my preferences" — deliberately does not discard
unrecognised entries, because those are another build's settings rather than this
user's history, and resetting on one machine must not delete preferences on
another.

## Performance review

Nothing on a hot path. A settings lookup is a map lookup over at most thirteen
entries with a computed fallback.

## Are the project's principles still respected?

| Principle | Verdict |
|-----------|---------|
| Never the easy solution when a premium one exists (MP#1) | Held. Absent-means-follow rather than a document full of defaults; a computed `follows_the_mode` rather than a declared flag. |
| The interface adapts, the capability does not (MP#8) | Held, mechanically, and checked in both directions. |
| Accessibility is not retrofitted (MP#8, MP#16) | Held. The platform's answer wins upward and cannot be withdrawn here. |
| Nothing is lost (MP#9) | Held, and extended to a case that had not been considered: an older build no longer deletes a newer one's preferences. |
| Modular, dependencies inward (MP#4, MP#7) | Held. Seventeen crates, acyclic; this one depends on nothing. |
| Quality is not a phase (MP#27) | Held. 708 tests; the review written in the same commit as the code, for the second sprint running. |

## Known limitations

1. **The settings document is not serialised here.** The document is defined; the
   bytes are the platform's, by ADR-0001. The unrecognised-entry mechanism is
   built for a writer that does not exist yet, which is the right order — the
   alternative is discovering the requirement after the first release has shipped
   a format that cannot express it.
2. **Consent and the learned profile are still not carried by anything.** This
   crate is their home in the sense that it establishes the per-user document;
   neither is re-declared here, deliberately, because a settings document that
   held them would make deleting a profile and resetting a preference the same
   operation. Wiring them together belongs to the layer that persists all three.
3. **No audio device or output settings.** Buffer size, sample rate and device
   selection are platform state, not preferences, and modelling them here would
   put a value in the document that is wrong the moment the user unplugs an
   interface.
4. **Nothing consults any of this yet**, for the same reason as Sprints 16 and
   17: the surface that would is in the application layer.

## Definition of done

| Criterion | Status |
|-----------|--------|
| Architecture reviewed | Yes — no new record needed |
| Implemented, no placeholders | Yes, enforced by rule 3 |
| Tests passing | Yes — 708 |
| Performance validated | Nothing on a hot path |
| Documentation updated | Yes, in the same commit as the code |
| Accessibility verified | Yes — the sprint contains the accessibility model, and the review states where its limits come from |
| Security reviewed | Yes — nothing new; one property named |
| No critical technical debt introduced | Four recorded limitations; persistence is the next one to close |
