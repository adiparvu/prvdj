# Module document template

Every module document uses exactly this structure. Custom formats are not
permitted (MP#31 — *Consistency is mandatory*).

---

# Module: <name>

- Module ID:      <kebab-case-id>
- Layer:          Domain | Application | Infrastructure | Presentation
- Owner:          <role>
- Status:         Not Started | In Design | In Development | In Review | Testing | Blocked | Completed | Released | Deprecated
- Source specs:   <Master Prompt / Module Specification references>

## 1. Purpose
What this module exists to do, in one paragraph. If it cannot be stated in one
paragraph, the module is doing too much.

## 2. Business value
Which user problem this solves and for which persona. A module that cannot
answer this does not get built (MP#11 P1).

## 3. Responsibilities
What it owns. Explicit and bounded.

## 4. Non-responsibilities
What it must never do, and which module owns that instead. This section is not
optional — it is what prevents a central module becoming a god object.

## 5. Public interfaces
The operations exposed, their inputs, outputs, error cases and stability
guarantees. Every public item is documented (MP#28 quality gate).

## 6. Internal components
Subsystems inside the boundary. Not visible to callers.

## 7. Dependencies
What it depends on, why, and the replacement strategy for each (MP#31).

## 8. Events
Events produced and consumed, with schema versions.

## 9. Data models
Entities owned, their invariants, and their persistence and synchronisation
metadata (MP#9 universal entity rules).

## 10. Performance requirements
Measurable budgets: latency, memory, CPU, maximum blocking time, recovery time,
throughput. Targets, not aspirations (MP#31).

## 11. Security considerations
Trust boundaries, permissions required, data sensitivity, validation rules.

## 12. Accessibility
How the module's user-facing surface satisfies MP#8 and MP#17. Modules with no
user-facing surface state that explicitly and say which module presents them.

## 13. Testing strategy
Which of the eight test categories apply and what each covers. Acceptance
criteria must be executable.

## 14. Failure recovery
Recoverable and non-recoverable errors, user-facing message, retry policy,
diagnostic identifier, and what is guaranteed to survive.

## 15. Future extensions
Named extension points, and what must not change to keep them viable.

## 16. Known risks
Cross-referenced with the risk register.

## Related documents
Specs, ADRs, sibling modules.
