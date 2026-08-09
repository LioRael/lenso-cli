---
name: lenso-incident-recovery
description: Diagnose a Lenso Service or System incident and prepare or validate recovery for process failure, Store outage or restore, deployment drift, migration failure, identity or trust outage, broker interruption, Workflow failure, regional disaster, or incomplete cleanup. Use before taking recovery action so evidence, authority, and Approval Boundaries remain explicit.
---

# Lenso Incident Recovery

Map authoritative evidence and stable issue codes to the smallest safe action.
Preserve completed effects and avoid inventing production authority.

## Workflow

1. **Establish safety and scope.** Identify the affected Service, Workloads,
   Stores, region, users, current authority, and whether work must continue,
   degrade, pause, reject, or fail closed. Escalate immediate safety or data
   integrity risks before investigative convenience.
2. **Collect exact evidence.** Follow
   [evidence and triage](references/evidence-and-triage.md). Load current
   release, deployment, Contract, Config Revision, Store, identity, Story,
   migration, delivery, and observation revisions. Finish when desired and
   observed state are separated and evidence freshness is known.
3. **Evaluate the declared failure.** Inspect the current GA failure-evaluation
   command and incident map when applicable. Match stable issue codes to the
   declared reliability or degraded-mode outcome. Unknown evidence produces an
   unknown result, not a guessed recovery.
4. **Preserve durable progress.** Account for completed migrations, Inbox and
   Outbox checkpoints, Workflow history, timers, external effects, last valid
   configuration, and active authority before selecting an action.
5. **Choose the smallest action.** Prefer idempotent resume, isolation,
   reconciliation, or rollback when evidence supports it. Follow
   [protected recovery](references/protected-recovery.md) for restore, regional
   cutover, trust changes, termination, or business compensation.
6. **Plan and revalidate.** Bind the proposed action to exact evidence and
   define success, abort, rollback, cleanup, and escalation conditions. Refresh
   the plan if observed state changes.
7. **Verify recovery.** Prove the target behavior, durable progress, authority,
   dependent health, observation evidence, and cleanup. Do not close an
   incident because one process restarted.

## Report

Return impact, evidence and freshness, authority state, stable issue codes,
completed effects, proposed or executed actions, stop conditions, verification,
cleanup, escalation, Approval Boundaries, and delivery state.
