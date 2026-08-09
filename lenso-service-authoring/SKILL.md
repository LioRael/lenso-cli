---
name: lenso-service-authoring
description: Create, implement, package, install, or verify an out-of-process Lenso service that provides one or more Modules, including its service manifest, routes, runtime functions, Events, actions, query values, local process declaration, and host-managed lifecycle. Use Autonomous Service authoring only when the service owns independent authority and Stores.
---

# Lenso Service Authoring

Build a self-contained service that provides Modules through public Lenso
contracts while the host retains installation, authentication, delivery, and
operational visibility.

## Workflow

1. **Resolve the service boundary.** Read
   [service boundary](references/service-boundary.md). Identify the owning
   repository, language, provided Modules, authoritative data, host dependency,
   and first useful workflow. Finish when this is clearly a host-managed
   service or clearly an Autonomous Service; never blend the two.
2. **Inspect public tooling.** Read the installed `lenso service --help`, the
   current Service Kit package exports, and the scaffolded example closest to
   the target language. Use repository and package metadata for current
   versions.
3. **Scaffold the service.** Prefer the current service scaffold. Keep the
   package runnable and publishable without sibling source checkouts.
4. **Define the contract.** Follow
   [manifest and provided Modules](references/manifest-and-modules.md). Declare
   only routes, runtime functions, Events, actions, query values, Console
   surfaces, and local processes that the service actually serves. Finish when
   the manifest endpoint and each provided Module endpoint validate.
5. **Preserve host-managed rails.** Follow
   [host-managed responsibilities](references/host-managed-responsibilities.md).
   Keep host authentication, retries, queues, proxy evidence, and installation
   state outside the service implementation.
6. **Develop through the real boundary.** Start the service, inspect its
   manifest, install it through the current CLI, and exercise one provided
   Module through the generated host. Fix the provider rather than adding
   workspace-only shortcuts.
7. **Package and verify.** Follow
   [packaging and verification](references/packaging-and-verification.md).
   Finish when a packed artifact runs outside the framework workspace, service
   and Module checks pass, lifecycle status is visible, and Console evidence
   identifies the installed provider and host-owned calls.

## Report

Return the Service and Module identities, manifest URLs, implemented surfaces,
local process declaration, install and lifecycle commands discovered from the
current CLI, package proof, focused checks, Console evidence, and delivery
state.
