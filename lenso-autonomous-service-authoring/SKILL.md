---
name: lenso-autonomous-service-authoring
description: Create, change, or validate an independently authoritative Lenso Service, including stable Service identity, Workloads, isolated Service Stores, Service Contracts, reliability and degraded modes, System Sandbox proof, or production delivery evidence. Use only for combinations admitted by the current GA Support Manifest; host-managed providers use lenso-service-authoring instead.
---

# Lenso Autonomous Service Authoring

Build one logical Service that owns its authority, state, execution, Contracts,
reliability, and recovery boundary.

## Workflow

1. **Prove support.** Inspect the released GA Support Manifest and current
   `lenso ga --help`. Run the support check for the exact CLI, runtime,
   Contract, adapter, and state-format combination. Follow
   [support boundary](references/support-boundary.md). Finish with a supported
   combination or an exact unsupported issue; do not infer compatibility.
2. **Define the Service model.** Follow
   [Service model](references/service-model.md). Declare a stable Service id,
   owned Modules, API/Worker/Migration Workloads, isolated Stores, Tenancy Mode,
   regions, configuration descriptors, Contracts, reliability profile, and
   degraded modes. Finish when every authoritative record and durable effect
   belongs to one Store and Workload path.
3. **Compose owned behavior.** Inject Module routes, migrations, Events,
   schedules, runtime functions, and Durable Workflows without importing
   another Service's internals or Store.
4. **Preserve distributed context.** Carry Story, causation, Workload identity,
   delegated actor, tenant, region, deadline, and idempotency through declared
   Contracts, Inbox, Outbox, and Workflow state.
5. **Prove locally.** Use the deterministic System Sandbox and public adapters.
   Exercise the success path and at least one declared Failure Scenario. Finish
   when restart, duplicate delivery, and cleanup behavior match the reliability
   declaration.
6. **Assemble delivery evidence when requested.** Inspect the current service
   delivery commands and produce immutable release, configuration, Contract,
   policy, resilience, and deployment evidence. Planning is not production
   authority.
7. **Verify and report.** Follow [verification](references/verification.md).
   Stop before production deployment, trust changes, destructive cleanup, or
   authority transfer unless the user explicitly authorizes the exact current
   plan.

## Report

Return support evidence, Service and Workload identities, Store ownership,
Contracts, reliability and degraded modes, Sandbox scenarios, artifact
digests, unsupported inputs, Approval Boundaries, cleanup, and delivery state.
