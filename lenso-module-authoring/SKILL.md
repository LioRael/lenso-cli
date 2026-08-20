---
name: lenso-module-authoring
description: Build a linked Rust Module behind a declared vNext Interface.
---

# Lenso vNext Module authoring

Implement one vertical linked Rust Module and keep its product behavior behind
the declared vNext seams.

1. Read the relevant ADRs, especially 0031, 0034, and 0054–0056.
2. Declare the Module, Capabilities, and Operations in the public contract.
3. Keep concrete behavior behind typed handles and the owning Module.
4. Register the Module only through explicit App Composition.
5. Test the contract, deterministic resolution, and the smallest useful
   behavior slice.

Do not add cross-Module imports, legacy registries, Service/Provider
abstractions, or product behavior to the portable Kernel.
