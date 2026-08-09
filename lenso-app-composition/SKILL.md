---
name: lenso-app-composition
description: Create, compose, plan, upgrade, repair, or verify a Lenso Launchpad application, blueprint, addon, capability pack, generated-state plan, App Proof, or agent handoff. Use when the capability boundary is known and the work concerns app-level composition rather than Module implementation.
---

# Lenso App Composition

Compose an application through the current public CLI and leave generated state
explainable. Treat plans and App Proof as evidence, not as substitutes for the
business implementation.

## Workflow

1. **Inspect the environment.** Run the current `lenso app --help`,
   `lenso capability --help`, and `lenso agent --help`. Inspect existing
   `.lenso` state and the workspace manifest when present. Finish when the
   installed command surface and current generated state are known.
2. **Choose the composition branch.** Read
   [composition choices](references/composition-choices.md). Decide between a
   new blueprint app, an addon, a reusable capability pack, or a planned change
   to an existing app. Finish when there is one composition source of truth.
3. **Preview before mutation.** Use the current plan, diff, inspect, fit, or
   explain command that matches the branch. Read
   [generated state](references/generated-state.md) before repairing or
   upgrading an existing app. Finish when proposed file, Module, and Service
   effects are visible and user-owned files are distinguished from generated
   state.
4. **Apply the bounded plan.** Apply only the reviewed composition effects.
   Preserve unrelated work and stop if the plan reaches outside the chosen app
   or capability boundary.
5. **Hand off implementation.** Use the current agent context or task command
   to produce a Module- or capability-scoped handoff. Route the code work to
   the relevant authoring skill.
6. **Verify the app.** Follow [App Proof](references/app-proof.md). Finish when
   generated state is current, the first useful workflow passes, and the proof
   names the actual Modules, Services, checks, and Console evidence.

## Report

Report the composition source, reviewed plan, applied effects, preserved user
files, implementation handoff, verification result, App Proof location when
written, and remaining manual work.
