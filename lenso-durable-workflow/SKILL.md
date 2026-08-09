---
name: lenso-durable-workflow
description: Design, implement, review, or evolve a Lenso Durable Workflow, including immutable definition versions, stable step identities, retries, timeouts, timers, child workflows, Events, effects, compensation, context propagation, in-flight migration, and protected operator controls.
---

# Lenso Durable Workflow

Define engine-neutral durable meaning that survives retries, restarts, rolling
deployments, and definition evolution.

## Workflow

1. **Frame the durable outcome.** Identify the owning Service, trigger,
   completion evidence, business effects, time bounds, intervention owner, and
   first failure that must survive process loss. Finish when ordinary request
   orchestration has been ruled out for a concrete durability reason.
2. **Define identities and state.** Follow
   [identity and execution](references/identity-and-execution.md). Pin each
   instance to an immutable definition version and assign stable identities to
   steps, transitions, attempts, timers, children, effects, and compensation.
3. **Declare execution policy.** Specify retry schedule, per-attempt timeout,
   absolute deadline behavior, idempotency, timer semantics, exhaustion, and
   operator intervention. Persist decisions before external effects.
4. **Model collaboration.** Publish cross-Service work through the owning
   Service Outbox and resume from stable completion Events. Preserve Story,
   causation, principal, delegated actor, tenant, region, deadline, and
   idempotency context.
5. **Design failure and compensation.** Follow
   [failure and evolution](references/failure-and-evolution.md). Record
   completed effects before choosing deterministic reverse-order compensation,
   and wait for declared completion evidence rather than assuming a request
   reversed the effect.
6. **Evolve safely.** Never reuse a version string to reinterpret in-flight
   state. Classify a change as safe, needs attention, breaking, or blocked.
   Prepare a dry-run mapping and stop at the in-flight migration Approval
   Boundary.
7. **Verify recovery.** Exercise restart, duplicate delivery, timeout, retry
   exhaustion, timer recovery, child completion, and compensation cases
   relevant to the definition. Finish when history and operator evidence
   explain every state transition.

## Report

Return the versioned definition, identities, state transitions, retry and timer
policy, effects and compensation, context, compatibility result, failure
scenarios, operator controls, checks, and Approval Boundaries.
