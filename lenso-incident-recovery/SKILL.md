---
name: lenso-incident-recovery
description: Use whenever diagnosing or planning recovery for a Lenso Service incident, failure scenario, Store restore, deployment drift, migration failure, identity outage, broker interruption, or active-passive disaster event.
---

# Lenso Incident Recovery

Map stable issue codes to authoritative evidence and the smallest safe next
action without inventing business compensation or production authority.

## Triage

1. Load the exact GA Support Manifest and incident evidence.
2. Identify Service, Workload, Store, Contract, Config Revision, release,
   deployment, Story, and observation revisions.
3. Classify the outcome as continue, degraded, paused, rejected, fail-closed,
   partial, or blocked.
4. Keep desired and observed state distinct.
5. Preserve completed Migration effects, Inbox/Outbox checkpoints, Workflow
   history, and the last valid configuration.
6. Choose idempotent resume, rollback, restore, isolation, or human
   intervention from the stable issue code.

## Output

Return evidence inspected, authority state, impact, stable issue codes,
completed effects, proposed actions, stop conditions, cleanup, and escalation.

## Approval Boundaries

Backup restore, destructive cleanup, regional authority cutover, failback,
trust changes, policy bypass, Workflow termination, and business compensation
require exact named approval. Prepare a stale-safe plan, then stop.
