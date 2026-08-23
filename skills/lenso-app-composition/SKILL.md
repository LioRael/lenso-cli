---
name: lenso-app-composition
description: Install or select Lenso Module packages, declare keyed Module Instances and configuration, bind Capability requirements, choose execution classes or placement, and materialize an immutable Resolved App Plan.
---

# Lenso App Composition

Select the exact Modules that make one App and resolve every dependency before
boot.

## Workflow

1. **Inspect the project authority.** Find `lenso.json`, package-manager
   manifests and lockfiles, Capability Descriptors, generated bindings, Web
   profiles, existing Resolved Plan, repository instructions, and the current
   `lenso --help`. Finish when generated, authored, and external package state
   are distinguishable.
2. **Select packages explicitly.** Add reviewable Cargo, Bun, npm, OCI, source,
   or remote UI inputs through their ordinary package managers and immutable
   selections. App Composition records inputs; it does not replace package
   locks or acquire code at runtime.
3. **Declare keyed Instances.** Give every Module Instance a stable App-local
   key, exact entrypoint, execution class, non-secret configuration or secret
   references, provided endpoints, requirements, and optional placement. The
   same package may appear under several keys.
4. **Bind every requirement.** Resolve `one`, `optional`, and `many`
   requirements to explicit provider keys. Preserve deterministic order for
   `many`; reject missing, ambiguous, duplicate, incompatible, or cyclic
   required request and stream edges.
5. **Expand profiles into ordinary Modules.** A Web or other profile is an
   authoring recipe. Its Shell, Browser Adapter, UI Contributions, and business
   Modules still appear as ordinary selections and exact bindings in the
   canonical Plan.
6. **Check and resolve.** Use the installed CLI's current check and resolve
   workflow. Review the project diff and canonical Resolved App Plan. Finish
   when package locks, Descriptor versions, execution classes, Endpoint Sets,
   configuration Schemas, sensitive references, and generated bindings all
   agree.
7. **Run the approved Plan.** Start from the exact canonical bytes. A change to
   packages, Composition, configuration, bindings, execution settings, or
   profiles requires a new resolve and App restart.

Return the project and Plan paths, packages selected, keyed Instances,
bindings and cardinalities, execution classes and placement, validation
results, reviewed diff, run evidence, and whether removing each optional
Module leaves a valid Composition.
