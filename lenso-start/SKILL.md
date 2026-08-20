---
name: lenso-start
description: Choose the smallest vNext workflow for a Lenso request.
---

# Lenso vNext start

Use the repository's vNext vocabulary and keep the requested change on the
smallest seam that owns it.

1. Read `CONTEXT.md` and the relevant ADR before planning.
2. Route product or capability design to `lenso-business-planning`.
3. Route explicit App Composition work to `lenso-app-composition`.
4. Route linked Rust Module work to `lenso-module-authoring`.
5. For Kernel, Runtime Driver, or Execution Adapter work, inspect the owning
   crate and the relevant ADRs directly.

The `next` branch contains only vNext. Do not revive legacy host, provider,
service, console, release, migration, or compatibility workflows here.
