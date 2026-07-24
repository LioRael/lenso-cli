---
name: lenso-autonomous-service-authoring
description: Use whenever creating, changing, or validating a Lenso Autonomous Service, Workload, Service Store, Service Contract, configuration, reliability profile, or local System Sandbox proof. Keep Provider authoring distinct and use only exact packages supported by the GA Support Manifest.
---

# Lenso Autonomous Service Authoring

Create one independently authoritative logical Service from public Lenso
packages without treating a Provider as an Autonomous Service.

## Start

1. Load the released `lenso.ga-support-manifest.v1.json`.
2. Run `lenso ga support-check` for the exact CLI, runtime, Contract, adapter,
   and state-format combination.
3. Start outside framework workspaces from the released starter.
4. Declare stable `serviceId`, owned Modules, API/Worker/Migration Workloads,
   isolated Service Stores, Tenancy Mode, regions, configuration, Service
   Contracts, reliability profile, and Degraded Modes.

## Build

- Keep Service identity stable across Workload replicas and deployments.
- Inject business routes and migrations from owned Modules.
- Keep Inbox, Outbox, Workflow, timer, Story Segment, and health state in the
  Service Store.
- Preserve Story Context, Workload Identity, delegated actor, tenant, deadline,
  idempotency, causation, and region across Contracts.
- Keep production adapters behind public seams; use the deterministic System
  Sandbox for local proof.
- Produce exact release, Contract, configuration, reliability, and evidence
  digests.

## Verify

Run the public check and one failure scenario. Report changed files, commands,
evidence identities, cleanup, unsupported inputs, and next actions.

## Boundaries

- Provider v1 remains the Host-managed compatibility path.
- Do not use sibling source checkouts or deep imports.
- Do not invent a distributed transaction or central Data Plane dependency.
- Stop before production deployment, trust changes, destructive cleanup, or
  authority transfer; name the Approval Boundary.
