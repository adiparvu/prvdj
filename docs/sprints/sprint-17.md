# Sprint 17 — Security policy

Master Prompt #26, as a crate. `prv-security` decides who may act, what the user
agreed to, what may be written down, and records what was decided.

| Delivered | Tests |
|-----------|-------|
| Authorisation, consent, redaction, secrets, audit | 48 new; 671 in total |

All gates green: `cargo test`, `cargo clippy --all-targets -- -D warnings`,
`cargo fmt --check`, `cargo doc`, the design-token staleness gate, and eight
architecture rules — two of which were changed this sprint and both re-tested
against deliberate violations.

## Business outcome

**"What can a viewer do" has a printable answer.** Every rule is a pure function
of values with one entry point, so the question can be answered by reading one
file rather than by observing the product. That is what makes an authorisation
model auditable, and an unauditable rule is not a rule.

**A support ticket can carry a diagnostic without carrying a music library.** A
user whose sync fails can send a log that says which service, how many attempts
and what the failure was — and no path, no account, no token. That is a product
decision as much as a security one: the fastest support interaction is the one
where the user is not weighing what they are about to disclose.

**Consent is a set of separate, separately withdrawable answers.** Not a bundle,
not an "improve the product" umbrella, not one switch. A user who wants their
tracks analysed on a server and does not want their sets used for training can
have exactly that, and can change their mind about either without touching the
other.

## Architecture review

**No new decision record was needed**, but two existing ones did work here.
ADR-0001 is the reason `SecretRef` names a location and cannot resolve one:
resolving it is input, so a credential has exactly one path into the process and
it is a path that can be audited in one place. ADR-0005 supplied the plugin
permission model this sprint had to be compatible with.

**One vocabulary for users and plugins.** `Capability` covers both. Two
vocabularies would have been two decision points wearing one name, which is the
thing Master Prompt #26 forbids. It costs an asymmetry — `UseNetwork` is a
strange thing to ask of a collaborator's role — and the doc comment answers it
rather than leaving it odd: they are the owner's device and the owner's
authority.

**Authorisation and consent are deliberately not one check.** A user is
permitted to have a track analysed in their own project; whether that audio may
be sent to a server is a different question with a different answer. Merging
them is how a product ends up reading "you own this" as "we may do anything with
it". There is a test at the crate root asserting the two do not imply each other.

**Security does not depend on entitlements, and never will.** They are different
axes with different remedies: an entitlement problem is answered by an upgrade,
an authorisation problem by the owner of the project. Offering the wrong remedy
is worse than offering none.

**The audit entry renders through the redaction module.** That is the only route
from an entry to text, so a future field that needed redacting would have to go
through the same door rather than around it.

Sixteen crates, acyclic, no runtime dependencies.

## Security review

This sprint *is* the security review, so the useful thing to record is which
guarantees are structural and which are procedural.

**Structural — the type system enforces them:**

- A diagnostic cannot contain a secret. `Field::secret` takes a name and no
  value. There is no expression in the crate's vocabulary that puts credential
  material into a record.
- A `Secret` cannot be printed. Every rendering, including the derived `Debug` a
  containing struct gets for free, produces a placeholder. The test that matters
  builds a struct with `#[derive(Debug)]` and asserts the token does not appear.
- A plugin permission set cannot hold forbidden authority. `request` refuses to
  store one and `from_bits` drops it when a manifest arrives from outside.
- Nothing is consented to by default. There is no constructor that begins with
  anything granted, so a bug that fails to load a stored answer fails closed.

**Procedural — enforced by a build gate rather than a type:**

- No credential material is committed. Architecture rule 8 scans every tracked
  and staged file. Verified against a planted key.

**Best effort, and labelled as such:**

- Zeroisation on drop. Nothing in safe Rust obliges the compiler to keep a write
  to a buffer about to be freed, and a guaranteed erase needs a volatile write,
  which needs `unsafe`, which ADR-0002 confines to `prv-rt`. Weakening that
  confinement for a defence-in-depth measure would cost more than it buys. The
  measure that carries the weight is holding credential material in the platform
  keychain and for as short a time as possible.
- Constant-time comparison. `Secret::matches` accumulates every difference
  instead of returning at the first one, which removes the byte-at-a-time oracle.
  It does not hide the length, deliberately: a credential's length is fixed by
  the scheme that issued it. It is also not a defence against every side channel,
  and the documentation says so rather than implying more than it delivers.

## Findings raised on my own work

**The architecture gate failed on my own documentation, and the gate was
wrong.** Rule 2 forbids `unsafe` outside `prv-rt` by grepping for the word.
`secret.rs` explains *why* it avoids unsafe code, so the file was flagged. The
tempting fix — reword the comment — would have trained every future author to
stop explaining that reasoning. The rule now strips comments before matching, and
I re-verified it still fails on real unsafe code before moving on. Worth naming:
a gate that punishes documentation will get the documentation removed, not the
code fixed.

**`is_modifying` was a predicate I invented to write a test with.** It claimed to
mean "changes something" and was implemented as "is not one of the two view
capabilities", which made it wrong for `ReadAudioStream` and `ReadSecrets`. It
existed only because the viewer test wanted a shortcut. Removed, and the test now
says what it means: a viewer's answer is *exactly* these two capabilities and
nothing else — which also means a capability added later is refused to a viewer
by default, and this test is what says so.

**I nearly gave `Field::personal` a value parameter.** It would have been useful
for exactly one case and would have made every log line in the product one
careless call away from a file path. Both withheld field kinds take no value, and
the awkwardness that creates — a diagnostic must be written in terms of derived
facts — is the feature. "Opening a file failed, extension=flac, exists=false,
bytes=0" is a better log line than one with a path in it, and it survives being
pasted into a public issue.

## Performance review

Nothing on a hot path. An authorisation check is a comparison or a bit test; the
audit log is a bounded deque; consent is a linear scan of at most nine entries.
None of it is reachable from the audio thread, and none of it should ever be —
the realtime contract is not a place to ask a policy question.

## Accessibility review

No surface yet, and two decisions made in its favour. Every tier, capability,
role, purpose, refusal and sensitivity carries a stable key rather than a display
string, so the interface localises without the core knowing any language. And a
refusal says whether asking the user would change the answer, so an interface
never offers a permission prompt that cannot be honoured — a dead end is worse
than a plain refusal.

## Are the project's principles still respected?

| Principle | Verdict |
|-----------|---------|
| Never the easy solution when a premium one exists (MP#1) | Held. A field that cannot hold a secret rather than a redaction function that must be remembered. |
| Privacy by design (MP#26) | Held. Nothing granted by default; training reachable from nothing else; an audit trail with no person in it. |
| Errors explain (MP#10) | Held. Every refusal names what was refused and whether the user can change it. |
| Production-ready, never placeholder (MP#13) | Held, with three limitations stated rather than implied. |
| Modular, dependencies inward (MP#4, MP#7) | Held. Sixteen crates, acyclic; security depends on nothing and nothing depends on it yet. |
| Quality is not a phase (MP#27) | Held. 671 tests; both changed build gates re-tested against deliberate violations; the review written in the same commit as the code. |

## Known limitations

1. **Zeroisation is best effort.** Stated above and in the source. The honest
   mitigation is architectural, not local.
2. **No signature verification.** ADR-0005 requires plugins to be signed.
   Verifying a signature needs a cryptographic implementation and a trust root,
   neither of which belongs in a crate with no dependencies. It arrives with the
   plugin manager.
3. **No durable audit retention.** The core keeps a live window of 1024
   decisions and counts what fell out of it. An archive needs a file, which needs
   input and output.
4. **Role resolution is the caller's.** This crate decides what a role may do; it
   does not look up which role someone holds on a given project. That lookup
   arrives with collaboration.
5. **Nothing consults any of this yet.** Like entitlements, the policy layer is
   complete and unreferenced, because the boundary that will consult it is in the
   application layer the Linux environment cannot compile.
6. **`Consents` and the learned profile have nowhere to live.** Both are per-user
   rather than per-project, and there is still no settings store. This is now the
   most concrete gap in the core.

## Definition of done

| Criterion | Status |
|-----------|--------|
| Architecture reviewed | Yes — no new record needed; one existing rule corrected |
| Implemented, no placeholders | Yes, enforced by rule 3 |
| Tests passing | Yes — 671 |
| Performance validated | Nothing on a hot path; nothing reachable from the audio thread |
| Documentation updated | Yes, in the same commit as the code |
| Accessibility verified | No surface yet; two decisions made in its favour |
| Security reviewed | Yes — the sprint is the review, and it separates structural from best-effort guarantees |
| No critical technical debt introduced | Six recorded limitations; a settings store is the most concrete |
