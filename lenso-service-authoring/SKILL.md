---
name: lenso-service-authoring
description: Create, implement, package, install, or verify an out-of-process Lenso Provider Service that exports one or more Modules through lenso.provider.v1, including exact release digests, business routes, runtime functions, Events, local process declaration, and Host-managed lifecycle. Use Autonomous Service authoring only when the Service owns independent authority and Stores.
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
2. **Inspect public tooling.** Read the installed `lenso service create --help`
   and `lenso module install --help`, the active Service Kit package exports,
   and the scaffolded example closest to the target language. Compare the
   installed skill catalog with `skills/README.md`; update a stale pack before
   authoring. Finish when the commands, package version, and selected skill
   names are present in the active environment.
3. **Scaffold the service.** Prefer the current service scaffold. Keep the
   package runnable and publishable without sibling source checkouts.
4. **Define the contract.** Follow
   [manifest and provided Modules](references/manifest-and-modules.md). Declare
   only real Business API routes, runtime functions, Events, Console Surfaces,
   and local processes. Finish when the exact Provider descriptor, every
   Module release/manifest digest, health endpoint, invocation recovery, and
   acknowledgement validate against the Host's locked input.
5. **Preserve host-managed rails.** Follow
   [host-managed responsibilities](references/host-managed-responsibilities.md).
   Keep host authentication, retries, queues, proxy evidence, and installation
   state outside the service implementation.
6. **Develop through the real boundary.** Start the service, inspect its
   `lenso.provider.v1` descriptor, install one exact Module release through the
   current CLI, and exercise one provided Module through the generated Host.
   Finish when the Host loads the locked release without a hand-written
   Provider adapter, generated digest file, or second server.
7. **Package and verify.** Follow
   [packaging and verification](references/packaging-and-verification.md).
   Finish when a packed artifact runs outside the framework workspace, service
   and Module checks pass, lifecycle status is visible, and Console evidence
   identifies the installed provider and host-owned calls.

## Report

Return the Service and Module identities, descriptor URL, implemented surfaces,
local process declaration, install and lifecycle commands discovered from the
current CLI, package proof, focused checks, Console evidence, and delivery
state.
