# 14. Security

Governed by Master Prompt #26. Status: **In progress** — the policy layer is
built and verified (`prv-security`, Sprint 17); enforcement lands with the
platform layer that holds a keychain, a socket and a log file.

## What is built

`prv-security` decides; it does not enforce. Every rule in it is a pure function
of values, which is what makes "what can a viewer do" a question with a printable
answer rather than a behaviour someone has to observe.

| Module | Question it answers |
|---|---|
| `authorisation` | May this subject do this? |
| `consent` | Has the user agreed to this happening at all? |
| `redaction` | May this be written down or sent? |
| `audit` | What was decided, and when? |

Authorisation and consent are separate checks and are never collapsed. A user is
*permitted* to have a track analysed in their own project; whether that audio may
be sent to a server is a different question with a different answer, and merging
them is how a product ends up reading "you own this" as "we may do anything with
it".

## Constraints in force today

- **Authorisation rules are centralised.** `prv_security::authorise` is the only
  decision point. Scattered permission checks cannot be audited, and an
  unauditable rule is not a rule.
- **Some authority is never delegated.** No plugin manifest can hold
  `ReadSecrets`, `ManageConsent`, `ManageLicence`, `ManageCollaborators`,
  `DeleteProject` or `InstallPlugin`, whatever it is signed with. A sandbox that
  can grant itself permissions is not a sandbox. The rule is enforced when the
  permission set is built *and* when it is consulted.
- **No secrets in source, ever.** Keys, tokens, credentials and certificates live
  in platform-appropriate secure storage. The core can name a location
  (`SecretRef`) and cannot resolve one, because resolving it is input and
  ADR-0001 forbids the core input. Architecture rule 8 scans every tracked and
  staged file for credential material.
- **No secrets in logs.** Structural rather than procedural: `Field::secret`
  takes a name and no value, so a token cannot be placed in a diagnostic record
  by anybody, including someone who wants to. `Field::personal` is the same shape
  one step weaker, so redaction is visible rather than leaving a reader unsure
  whether a field was empty or removed.
- **No agreement makes a secret loggable.** `Sensitivity::Secret` is excluded
  from consented diagnostics permanently. A user may consent to sharing their own
  information; they cannot consent on behalf of the service that issued a
  credential.
- **Nothing is agreed to by default.** Every purpose starts withheld, including
  the harmless-sounding ones, so a bug that fails to load a stored answer fails
  closed. There is no opt-out anywhere in the model.
- **The user is told where their data is processed.** Every purpose states
  whether it runs on the device or leaves it, and whether what leaves is the
  user's own material or a fact about it. Master Prompt #26 requires external AI
  use to be clearly indicated, so on-device and cloud intelligence are visually
  distinct states in the token system (section 5), not one undifferentiated "AI"
  colour.
- **User projects are never used to train models without explicit permission.**
  `Purpose::ModelTraining` is its own purpose, granted by its own act, reachable
  from nothing else — asserted by a test that grants every other purpose and
  checks that training is still withheld.
- **An audit trail records decisions, not people.** An entry holds an ordinal, an
  actor *kind*, a capability and a decision — four closed vocabularies and
  nothing else. It renders through the redaction module, so the only route from
  an entry to text is one that will not print personal data. The in-memory window
  is bounded and reports how many entries it discarded, because a security log
  that quietly forgets reads as a complete record of a period in which it was not
  one.
- **Jurisdiction-specific assumptions stay out of the core** so that future
  regulatory change is a policy edit rather than an architectural one. Nothing in
  `prv-security` names a territory, a statute or a retention period; a rule such
  as "diagnostics may not leave this region" is expressed by a transport that
  consults the consent model.
- **Entitlement checks live above the engines.** The DSP graph, the planner and
  the analysis pipeline do not know what tier a user is on. Master Prompt #29
  requires that essential functionality is never artificially restricted; keeping
  entitlements out of the engines makes that structurally true, and architecture
  rule 7 keeps it that way.

## Known gaps

1. **Zeroisation of secret material is best effort.** `Secret` overwrites its
   bytes on drop, and nothing in safe Rust obliges the compiler to keep a write
   to a buffer about to be freed. A guaranteed erase needs a volatile write,
   which needs `unsafe`, which ADR-0002 confines to `prv-rt`. The measure that
   carries the weight is holding credential material in the platform keychain and
   for as short a time as possible.
2. **Signature verification is not built.** ADR-0005 requires plugins to be
   signed. Verifying a signature needs a cryptographic implementation and a trust
   root, both of which sit outside a crate with no dependencies.
3. **Durable audit retention is not built.** The core keeps a live window; an
   archive needs a file.
4. **Role resolution is the caller's.** This crate decides what a role may do; it
   does not look up which role someone holds. That lookup is infrastructure and
   arrives with the collaboration module.
