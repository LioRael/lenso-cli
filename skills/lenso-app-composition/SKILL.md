---
name: lenso-app-composition
description: Define an explicit App Composition and Resolved App Plan.
---

# Lenso vNext App Composition

Make composition explicit and deterministic.

- Name each Module Instance with a stable key.
- Bind each required Capability to exactly one declared instance unless the
  composition explicitly permits one or many.
- Resolve the plan before execution and preserve its identities.
- Keep Runtime Drivers and Execution Adapters outside product composition.
- Reject duplicate keys, missing bindings, and ambiguous selections early.

Do not introduce runtime discovery, hidden global registries, or mutable plan
resolution as a substitute for App Composition.
