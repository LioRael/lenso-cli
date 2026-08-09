---
name: lenso-module-extraction
description: Evaluate, plan, scaffold, backfill, reconcile, verify, quiesce, provisionally cut over, roll back, or transfer authority when extracting a linked Lenso Module into an Autonomous Service. Use from the first extraction-readiness request and preserve digest-bound evidence through the authority Approval Boundary.
---

# Lenso Module Extraction

Move one proven Module boundary across a process and authority boundary without
changing its capability identity or losing rollback evidence.

## Workflow

1. **Resolve the candidate.** Identify the linked Module, owning host, data,
   transactions, dependencies, Consumers, Console Surfaces, background work,
   and target Service boundary. Finish when the extraction scope names every
   authoritative table, effect, and public Contract.
2. **Run readiness.** Inspect the current `lenso module extraction --help` and
   produce the authoritative readiness report. Follow
   [blockers](references/blockers-and-approval.md). Stop on unresolved
   ownership, cross-Module tables, cross-boundary transactions, missing
   Consumer evidence, or ambiguous authority.
3. **Create the digest-bound plan.** Follow
   [extraction stages](references/extraction-stages.md). Pin readiness, Module,
   Contract, source state, target shape, migration, backfill, verification,
   rollback, and approval inputs. A changed input invalidates the plan.
4. **Scaffold without authority.** Generate only plan-bound Service, Store,
   migration, and Contract client files. Keep the linked Module authoritative.
5. **Expand and backfill.** Create the isolated candidate Store, copy data with
   durable checkpoints and stable record identities, and preserve a trustworthy
   high-water mark or protected write pause.
6. **Reconcile and verify.** Compare business invariants, Contract behavior,
   Events, runtime work, Console requirements, and failure scenarios between
   linked and autonomous paths.
7. **Drain and provisionally cut over.** Quiesce requests, Inbox, Outbox,
   schedules, timers, and Workflows through the current protected plan. Record
   candidate health and rollback constraints.
8. **Stop at authority.** Prepare the exact authority-transfer plan and fresh
   revalidation. Repository access and passing checks are not authorization.
   Apply transfer only with named approval bound to the current digest.

## Report

Return artifact ids and digests, blockers, completed stages, checkpoints,
reconciliation, behavior comparison, drain state, candidate health, rollback
limits, approval state, cleanup, and next action.
