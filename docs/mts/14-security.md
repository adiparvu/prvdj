# 14. Security

Governed by Master Prompt #26. Status: **Not started** — hardening begins with
the first stored credential in Phase 5; the constraints below already bind
current work.

## Constraints in force today

- **Authorisation rules are centralised.** Scattered permission checks cannot be
  audited, and an unauditable rule is not a rule.
- **No secrets in source, ever.** Keys, tokens, credentials and certificates live
  in platform-appropriate secure storage.
- **No secrets in logs.** Structured logging redacts by field type rather than by
  the author remembering.
- **The user is told where their data is processed.** Master Prompt #26 requires
  external AI use to be clearly indicated, so on-device and cloud intelligence
  are visually distinct states in the token system (section 5), not one
  undifferentiated "AI" colour.
- **User projects are never used to train models without explicit permission.**
- **Jurisdiction-specific assumptions stay out of the core** so that future
  regulatory change is a policy edit rather than an architectural one.
- **Entitlement checks live above the engines.** The DSP graph, the planner and
  the analysis pipeline do not know what tier a user is on. Master Prompt #29
  requires that essential functionality is never artificially restricted; keeping
  entitlements out of the engines makes that structurally true.
