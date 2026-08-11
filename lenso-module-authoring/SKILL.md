---
name: lenso-module-authoring
description: Create, implement, or change a linked Rust Lenso Module whose business boundary is already known, including its ModuleManifest, Business API routes, runtime functions, Events, lifecycle work, dependencies, and Console Surface declaration. Use business planning first when ownership is still unclear.
---

# Lenso Module Authoring

Implement one vertical business capability through the public `lenso` facade
and the owning host's supported seams. Keep declaration data serializable and
behavior behind narrow host bindings.

## Workflow

1. **Resolve the owner and environment.** Find the Module crate, host
   composition root, generated-state boundary, repository instructions, and
   current CLI and Cargo package surfaces. Finish when the exact Module and
   verification owner are known.
2. **Confirm the boundary.** Read [Module boundary](references/module-boundary.md).
   List authoritative data, lifecycle, permissions, dependencies, and the first
   useful workflow. Finish when the change does not require another Module's
   private code or tables.
3. **Scaffold from the public surface.** Prefer the installed CLI's current
   Module scaffold for a new capability. In an existing app, inspect the
   generated agent handoff and preserve generated versus user-owned files.
4. **Declare only real surfaces.** Follow
   [manifest and surfaces](references/manifest-and-surfaces.md). Add routes,
   runtime functions, Events, lifecycle work, configuration, dependencies, and
   Console metadata only when corresponding behavior exists. Finish when
   manifest lint has no invented or empty Surface.
5. **Implement vertical behavior.** Follow
   [behavior and collaboration](references/behavior-and-collaboration.md).
   Keep input validation, storage, business rules, and runtime records inside
   the owning capability. Register cross-cutting wiring only in the host's
   composition root.
6. **Delegate distinct Console UI work.** When the change needs a new or
   substantially revised operator experience, use
   `lenso-console-surface-authoring`. The Module remains the source for
   identity, path, capability, and release-bound presentation metadata.
7. **Regenerate owned artifacts.** When public handlers, schemas, Events, or
   manifests change, run the owning repository's current generator before its
   freshness and architecture checks. Never hand-edit generated output.
8. **Verify the capability.** Follow [verification](references/verification.md).
   Finish when one focused path fails without the Module wiring, all changed
   declarations are exercised, and expected Console state is named.

## Report

Return the Module owner, first useful workflow, declarations added or changed,
collaboration seams, generated artifacts, focused checks, Console state,
and delivery state.
