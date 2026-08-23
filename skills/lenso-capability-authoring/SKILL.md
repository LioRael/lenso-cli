---
name: lenso-capability-authoring
description: Design or evolve a Lenso Capability contract, including its role, Operations, interaction kinds, Descriptor, Schemas, compatibility, and generated consumer and provider bindings. Use when multiple Modules need an explicit collaboration Interface.
---

# Lenso Capability Authoring

Author one deep role Interface that Module consumers depend on without knowing
the provider implementation.

## Workflow

1. **Resolve contract ownership.** Identify the consumer role, eligible
   providers, current Descriptor or local Interface, package owner, and every
   real consumer. Business Capabilities normally stay with their owning Module
   or domain; only deliberately standardized roles with independent reuse
   belong in a framework protocol repository.
2. **Define the role.** Name the Capability as `namespace.name@major`. State
   what the consumer can rely on and what remains provider-private. Finish when
   two implementations could satisfy the role without sharing internal code or
   storage.
3. **Design Operations.** Read
   [contract shape](references/contract-shape.md). Choose request, stream, or
   event from the interaction semantics. Define domain outcomes separately
   from runtime failures. Record each consumer's requirement cardinality for
   App Composition; cardinality is not part of the Capability contract.
4. **Choose local or portable.** Keep an Interface Adapter-local when every
   supported consumer and provider shares that runtime. For cross-Adapter use,
   make the Descriptor and package-local JSON Schemas the source of truth.
5. **Generate bindings.** Use the owning repository's current
   `lenso-contract-codegen --help` and checked-in generation workflow. Generate
   consumer and provider artifacts from one Descriptor source; custom behavior
   stays outside generated files.
6. **Evolve and prove.** Follow
   [evolution and verification](references/evolution-and-verification.md).
   Finish when the exact Descriptor version, generated artifacts, consumer and
   provider checks, and compatibility result are all observable.
7. **Hand off.** Route provider or consumer behavior to
   `lenso-module-authoring` and exact bindings to
   `lenso-app-composition`.

Return the role, identity and version, Operations, portability choice, contract
and generated paths, compatibility result, consumers and providers exercised,
and remaining handoffs.
