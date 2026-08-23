---
name: lenso-business-planning
description: Turn a product request with unclear behavior or ownership into a vertical Lenso Module map and first executable slice. Use before Capability, Module, or App Composition work when the owner is not settled.
---

# Lenso Module-first Planning

Turn a product outcome into the smallest set of Modules whose removal also
removes their product complexity. Planning stops at an implementation handoff.

## Workflow

1. **Frame the outcome.** Identify the actor, useful result, authoritative
   facts, and first observable success and failure. Finish when the outcome can
   be stated without naming Lenso machinery.
2. **Apply the Module test.** Read
   [the Module test](references/module-test.md) when a concern could belong to
   product behavior, composition, or runtime infrastructure. Classify every
   concern by its actual owner rather than its package or process shape.
3. **Draw vertical boundaries.** Group behavior while data ownership,
   lifecycle, authorization, failure policy, and change cadence align. Give
   every mutable fact one Module owner. A process split is an Execution Adapter
   choice, not a new product type.
4. **Name collaboration roles.** Introduce a Capability only where one Module
   needs a stable role from another. Name its consumer goal, provider
   responsibility, Operations, interaction kinds, and cardinality; leave
   private implementation details inside the Module.
5. **Cut one executable slice.** Select the fewest Module Instances and
   bindings that deliver one useful transition, its authorization, one honest
   failure, and observable evidence. Finish when removing any selected piece
   makes the slice unusable or unprovable.
6. **Hand off.** Use [planning output](references/planning-output.md). Route
   contract work to `lenso-capability-authoring`, behavior to
   `lenso-module-authoring`, selections and bindings to
   `lenso-app-composition`, and host mechanics to
   `lenso-runtime-extension`.

Planning is complete only when every concern has one owner, every cross-Module
edge has an explicit Capability, and the first slice can become one immutable
Resolved App Plan.
