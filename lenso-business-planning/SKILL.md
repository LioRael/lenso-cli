---
name: lenso-business-planning
description: Plan a Lenso application from a product or business request whose actors, owned data, capability boundaries, or first useful workflow are still unclear. Use before app composition or Module and Service authoring when the implementation boundary has not been chosen.
---

# Lenso Business Planning

Turn an uncertain product request into the smallest owned Lenso slice that can
be implemented, checked, and observed. Planning finishes at an implementation
handoff; it does not scaffold code.

## Workflow

1. **Frame the outcome.** Identify the user, business result, and first moment
   at which the product becomes useful. Finish when the outcome can be stated
   without naming framework machinery.
2. **Resolve ownership.** Identify the core records, lifecycle authority,
   external systems, tenant boundary, and privileged actors. Read
   [ownership boundaries](references/ownership-boundaries.md) when more than
   one capability or system may own the same fact. Finish when every mutable
   business fact has one owner.
3. **Choose capability boundaries.** Keep behavior together while ownership,
   lifecycle, permissions, data, and deployment needs align. Read
   [Module versus Service](references/module-vs-service.md) before introducing
   an out-of-process boundary. Finish when dependency direction is explicit
   and no capability requires another capability's private tables or code.
4. **Cut the first useful slice.** Follow
   [first useful slice](references/first-useful-slice.md). Include one actor,
   one lifecycle transition, its authorization, one observable failure, and
   the Console evidence that proves it. Finish when removing any remaining
   item would make the slice unprovable or not useful.
5. **Map public surfaces.** For each capability, name only the routes, data,
   actions, Events, runtime functions, lifecycle work, configuration, and
   Console Surfaces required by that slice. Mark unsupported operations as
   later work rather than inventing them.
6. **Select the handoff.** Route generated app work to
   `lenso-app-composition`, linked Rust work to `lenso-module-authoring`,
   out-of-process work to `lenso-service-authoring`, and distinct operator UI
   work to `lenso-console-surface-authoring`.
7. **Return the plan.** Use [planning output](references/planning-output.md).
   Finish only when every planned capability has an owner, collaboration seam,
   first-slice responsibility, verification path, and follow-up skill.

## Boundary

Treat a deployment split as a consequence of hardened ownership, not a
starting assumption. Keep the host thin: composition, shared policy anchors,
and deployment wiring belong there; business behavior belongs in Modules or
Services.
