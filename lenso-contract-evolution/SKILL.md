---
name: lenso-contract-evolution
description: Use whenever adding, changing, deprecating, migrating, or retiring a Lenso Service, Event, Config, Reliability, or Workflow Contract. Preserve parallel majors and Consumer evidence, and stop before Contract Retirement authority.
---

# Lenso Contract Evolution

Evolve Contracts from committed public artifacts and the exact GA Support
Manifest.

## Workflow

1. Identify the Contract, owning Service or Module, current versions, active
   Consumers, and support combination.
2. Classify the change:
   - compatible addition: remain on the current major;
   - breaking change: publish a parallel major;
   - deprecation: retain both versions through the declared window;
   - retirement: build a stale-safe plan from current Consumer evidence.
3. Regenerate clients and schemas from the Contract source.
4. Verify old and new Consumers, mixed-version operation, rollback, and
   deterministic cleanup.
5. Return normalized plan, Policy Evidence, issue codes, and next actions.

## Guardrails

- Do not reinterpret compatibility from semantic-version proximity.
- Do not hand-edit generated clients.
- Active Consumers, stale evidence, an incomplete deprecation window, or a
  missing replacement blocks Retirement.
- A valid plan is not approval. Stop before Contract Retirement until named
  approval is bound to the exact plan digest.
