---
name: lenso-module-authoring
description: Create or change a Lenso Module in any supported execution shape, including native Rust, Bun, Web UI, stateful, Auth, Story, Audit, OpenTelemetry, Secrets, and other removable product behavior. Use when the Module boundary and Capability roles are known.
---

# Lenso Module Authoring

Implement one vertical product capability as an ordinary Module. The selected
execution mechanism changes the factory and Adapter, not the product type.

## Workflow

1. **Resolve the owner.** Find the Module package, repository instructions,
   Descriptor or Interface source, target Execution Adapter, App Composition,
   generated-file boundary, and current verification commands. Finish when the
   exact behavior and release owner are known.
2. **Confirm the Module card.** State its deletion boundary, owned facts,
   lifecycle, final authorization, provided and required Capabilities,
   configuration, and managed resources. Use `lenso-business-planning` first
   when those responsibilities still compete.
3. **Select the shape.** Read [Module shapes](references/module-shapes.md) for
   the relevant native Rust, Bun, Web, stateful, or cross-cutting branch. Keep
   process and transport mechanics in the Execution Adapter.
4. **Use explicit contracts.** Consume generated or local Capability handles
   for cross-Module collaboration. Use `lenso-capability-authoring` when an
   Interface or Operation must change. Module code does not import another
   Module's private implementation or tables.
5. **Implement one generation.** Publish Module Descriptor data and the
   Adapter-specific factory. For each Resolved Module Instance, create fresh
   state that prepares exact endpoints, activates managed work, and
   deactivates cleanly. Validate opaque configuration during preparation.
6. **Own product semantics.** Keep business rules, state meaning, recovery,
   authorization, and truthful failure behavior inside the owning Module.
   Register no feature-specific Kernel hook.
7. **Compose explicitly.** Use `lenso-app-composition` to select the package,
   keyed Instance, entrypoint, configuration or secret references, execution
   class, and every required binding. Static linking never makes an Instance
   implicit.
8. **Prove removability.** Follow [verification](references/verification.md).
   Finish when the selected behavior works through its Capability and removing
   the Module selection removes its product complexity without Kernel residue.

Return the Module owner and shape, deletion boundary, provided and required
Capabilities, lifecycle and state choices, composition changes, generated
artifacts, focused behavior proof, removal proof, and delivery state.
