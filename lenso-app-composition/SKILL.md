---
name: lenso-app-composition
description: Create, compose, upgrade, repair, run, connect, or inspect an exact Lenso App Composition from a blueprint, addon, capability pack, or agent handoff. Use when the capability boundary is known and the work concerns app-level composition rather than Module implementation.
---

# Lenso App Composition

Compose an application through the current public CLI and leave one exact,
revisioned `lenso.app.json`. Generated state is not a substitute for the
business implementation.

## Workflow

1. **Inspect the App.** Run the current `lenso app --help` and `lenso system
   dev --help`. Inspect `lenso.app.json` and relevant `.lenso` local state when
   present. Finish when the installed command surface, App revision, and
   current implementation bindings are known.
2. **Choose the composition branch.** Read
   [composition choices](references/composition-choices.md). Decide between a
   new blueprint App, an addon, a reusable capability pack, or a targeted
   recomposition of an existing App. Finish when there is one composition
   source of truth.
3. **Compose.** Preview the current `lenso app compose` result before applying
   it. Read [generated state](references/generated-state.md) before repairing
   or upgrading an existing App. Apply only the reviewed effects, preserve
   unrelated files, and validate the exact revision, digests, dependency
   selections, and Linked or Service bindings in `lenso.app.json`.
4. **Hand off implementation.** Use the current agent context or task command
   to produce a Module- or capability-scoped handoff. Route the code work to
   the relevant authoring skill.
5. **Run locally.** Start the exact composition through `lenso system dev` and
   its Local Control Adapter. Keep runtime commands and credentials outside the
   App Composition.
6. **Connect.** Inspect `lenso console connect --help`. Apply one reviewed
   `lenso.console-connect.v1` bundle containing the signed enrollment receipts,
   optional exact Console artifact effect, and digest-bound System Connection.
   Finish only when the command returns a connected projection. If the current
   runtime cannot produce that bundle, report the missing producer instead of
   writing a project-specific signing script or editing Console storage.
   Console does not create, release, or deploy Workloads.
7. **Status.** Follow [App verification](references/app-verification.md).
   Finish when the first useful workflow passes and Console reports direct
   System, Service, Module, Surface, and Workload states with reasons.

## Report

Report the composition source, exact App revision and digests, applied effects,
preserved user files, implementation handoff, local runtime state, Console
connection state, verification result, and remaining manual work.
