---
name: lenso-module-authoring
description: Use when implementing or editing a known Rust Lenso module after the business/module boundary is already chosen, including manifests, HTTP routes, runtime functions, events, lifecycle declarations, and console metadata. For product prompts from scratch, use `lenso-business-planning` first.
---

# Lenso Module Authoring

## Overview

Use the public `lenso` crate for first-party Rust module authoring.
Keep declarations serializable and keep host internals out of the module API.

## Start Here

```sh
cargo add lenso@0.3.16
```

Use `ModuleManifest` for declarations:

- module capabilities
- admin surfaces
- HTTP routes
- runtime functions
- event handlers
- lifecycle declarations
- console surfaces

## Guardrails

- Keep module behavior behind the host boundary.
- Keep the module vertical.
- Do not import another module's internals.
- Use the committed OpenAPI and contract artifacts for API-facing work.
- Prefer `lenso module create <name>` before hand-building a module shape.

## Agent Output

For a new or edited module, leave:

- manifest declarations for routes, data, actions, runtime functions, and console surfaces that actually exist
- app-owned behavior in the module, not in platform crates
- one runnable check or smoke path that fails if the module is not wired
- a short note on what appears in the Runtime Console

## Checks

When the change affects contracts or manifests, run the repo checks the host expects:

```sh
just generated-check
just arch-check
```

## Keep Out

- Do not add a generic CRUD framework before the module needs it.
- Do not promote app-specific data access into the public host facade.
