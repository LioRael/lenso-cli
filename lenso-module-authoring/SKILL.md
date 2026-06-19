---
name: lenso-module-authoring
description: Use when authoring or editing a Rust Lenso module that depends on the public `lenso` crate, including manifests, HTTP routes, runtime functions, events, lifecycle declarations, and console metadata.
---

# Lenso Module Authoring

## Overview

Use the public `lenso` crate for first-party Rust module authoring.
Keep declarations serializable and keep host internals out of the module API.

## Start Here

```sh
cargo add lenso@0.3.5
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

## Checks

When the change affects contracts or manifests, run the repo checks the host expects:

```sh
just generated-check
just arch-check
```
