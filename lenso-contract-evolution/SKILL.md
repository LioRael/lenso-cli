---
name: lenso-contract-evolution
description: Add, change, version, deprecate, migrate, or retire a Lenso HTTP, Service, Event, Config, Reliability, Workflow, context, or error Contract. Use when producer or consumer wire meaning changes, including apparently additive schema edits, generated-client updates, parallel majors, and Contract Retirement.
---

# Lenso Contract Evolution

Evolve one Contract without silently changing the meaning seen by existing
Consumers.

## Workflow

1. **Inventory the Contract.** Identify the authoritative source, owner,
   bindings, current versions, generated artifacts, active Consumers, support
   combination, and deployment evidence. Finish when every known producer and
   Consumer is accounted for or explicitly unknown.
2. **Classify the semantic change.** Follow
   [compatibility](references/compatibility.md). Decide compatible addition,
   parallel-major breaking change, deprecation, or retirement. Finish when the
   classification is based on observable Consumer behavior rather than version
   number proximity.
3. **Change the source once.** Modify the owning type, handler, schema source,
   or Contract definition. Regenerate schemas, bindings, and clients through
   repository-owned commands. Never patch generated output as the source.
4. **Plan coexistence.** For parallel majors or deprecation, state routing,
   support window, Consumer migration, rollback, observability, and cleanup.
   Preserve both meanings while active Consumers require them.
5. **Verify both sides.** Follow
   [evidence and verification](references/evidence-and-verification.md). Test
   old and new Consumers, mixed-version operation, native failure behavior,
   context propagation, and rollback.
6. **Retire only through evidence.** Inspect the current Contract-retirement
   command when retirement is requested. Active Consumers, stale evidence,
   incomplete deprecation, or a missing replacement blocks the plan. Stop
   before mutation until named approval is bound to the exact current digest.

## Report

Return owner, versions, Consumers, classification, changed source and generated
artifacts, coexistence plan, verification, rollback, retirement blockers,
approval state, and next actions.
