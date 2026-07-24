---
name: lenso-durable-workflow
description: Use whenever designing or changing a Lenso Durable Workflow, including versioned steps, Events, retries, timers, child workflows, compensation, context propagation, migration, or operator controls.
---

# Lenso Durable Workflow Design

Design engine-neutral Workflows whose durable meaning survives restarts and
definition evolution.

## Design

- Pin each instance to an immutable versioned definition artifact.
- Give steps, transitions, timers, attempts, child instances, effects, and
  compensations stable identities.
- Preserve Story, causation, Service Principal, delegated actor, tenant,
  deadline, and idempotency context.
- Publish cross-Service work through the owning Service Outbox.
- Resume parents from stable child-completion evidence exactly once.
- Declare retry schedule, attempt timeout, exhaustion, and intervention path.
- Record completed effects before selecting deterministic reverse-order
  compensation; wait for the declared completion Event.

## Evolution

Classify definition changes as safe, needs-attention, breaking, or blocked.
Never reuse a version string to reinterpret in-flight state. Produce a dry-run
mapping and stop at the in-flight migration Approval Boundary.

## Reject

Reject implicit distributed transactions, missing completion evidence,
undeclared compensation, identity reuse, erased history, or operator actions
without exact protected plans.

Return the definition, compatibility result, failure paths, evidence, tests,
and Approval Boundaries.
