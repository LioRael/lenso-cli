---
name: lenso-business-planning
description: Use when a user starts from a broad or vague business idea and wants to build a Lenso business system from zero, especially to clarify requirements, decide whether the work should be a host app, one module, multiple collaborating modules, linked Rust modules, remote modules, or a staged implementation plan.
---

# Lenso Business Planning

## Overview

Turn a fuzzy business prompt into a Lenso-ready implementation plan before writing code.
Ask only the questions that change module boundaries, then choose the smallest useful slice and route to the right authoring skill.

## Intake

If the user has not provided enough detail, ask at most five questions in one turn.
Prioritize questions that change ownership, module boundaries, or the first runnable slice:

- Who are the primary actors and who uses the admin or operations surface?
- What is the core business object and its lifecycle?
- What is the first workflow that proves value from trigger to outcome?
- What data is owned by the system, imported from another system, or synced through an integration?
- What tenant, permission, audit, billing, or compliance boundary matters early?

If a detail is missing but a conservative assumption is safe, state the assumption and continue.
Do not ask for exhaustive product requirements before giving a path.

## Planning Workflow

1. Restate the business goal in one sentence.
2. Pick the first useful slice that can be scaffolded, checked, and verified in `/console`.
3. Decide whether the work belongs in a host app, an existing module, one new module, or multiple modules.
4. Decide whether each new module should be a linked Rust module or a remote module.
5. Sketch the required declarations: manifest, HTTP routes, schema-admin data, admin actions, runtime functions, events, lifecycle jobs, console surfaces, config, and dependencies.
6. Identify cross-module collaboration through declared dependencies, events, host-owned queues, remote HTTP/proxy surfaces, or public APIs.
7. Leave the next concrete command and follow-up skill.

## Boundary Heuristics

Keep work in one module when the objects share one owner, lifecycle, permission model, data store, and Console surface.

Split into multiple modules when capabilities can be installed separately, have different owners, can be disabled independently, need a clear dependency direction, or represent a hardened trust/deployment boundary.

Choose a linked Rust module when the capability is first-party, should ship in the same deployable host, needs local transactions, or is the fastest path to prove the product slice.

Choose a remote module when the capability is third-party, team-owned outside the host, JavaScript or TypeScript based, publishable on its own, or already needs an out-of-process service boundary.

Keep the host thin. Put business-owned behavior in modules unless the work is pure host setup, auth/config anchoring, or deployment wiring.

## Agent Output

For a clarified plan, return:

- clarifying questions, only if they block a responsible module decision
- assumptions
- recommended shape: host app, modules, linked vs remote, and dependency graph
- first slice with the smallest testable workflow
- module plan table with each module's owner, data, surfaces, collaborations, and reason
- next commands and follow-up skills

Use these follow-up routes:

- blank host -> `lenso host init <dir>` -> `lenso-starter-host`
- in-host module -> `lenso module create <name>` -> `lenso-module-authoring`
- remote module -> `lenso module create <name> --remote` -> `lenso-remote-module-authoring`
- API client or integration check -> committed OpenAPI contract -> `lenso-api-client`

## Keep Out

- Do not split a vague business into microservices before boundaries are real.
- Do not build a generic CRUD framework before a real module needs it.
- Do not cross-import module internals; use declared seams or host-owned collaboration.
- Do not require cloning the framework monorepo when public CLI, crates, packages, or skills fit.
