---
name: lenso-runtime-extension
description: Implement or change how Lenso Modules are scheduled, instantiated, connected, isolated, or hosted through a Runtime Driver, Execution Adapter, Runner, execution class, process, transport, or endpoint mechanism. Use for host mechanics rather than product behavior.
---

# Lenso Runtime Extension

Extend how Modules run without turning host machinery into product Modules or
moving product policy into the portable Kernel.

## Workflow

1. **Classify the seam.** Read
   [runtime seams](references/runtime-seams.md). State the host facility being
   adapted and why an ordinary Module cannot own it. Route removable product
   behavior back to `lenso-module-authoring`.
2. **Resolve repository ownership.** Find the core Interface version, owning
   runtime or Adapter repository, relevant ADR, conformance package, supported
   targets, and current validation commands. Cross-repository dependencies use
   released packages or an explicitly approved immutable bootstrap reference,
   never sibling path dependencies.
3. **Preserve one-way layers.** Plan data and Kernel Interfaces stay portable.
   Drivers and Adapters implement those Interfaces. Runners assemble them.
   Modules and Apps depend inward; core does not depend on a concrete host,
   protocol, Module, CLI, or example.
4. **Implement the narrow host translation.** A Driver supplies scheduling,
   monotonic time, timers, cancellation, and progress. An Execution Adapter
   supplies Module generations, endpoint mechanics, isolation, and
   process/wire failure translation. A Runner supplies available
   implementations, drives Kernel, translates host shutdown, and handles the
   terminal outcome.
5. **Fail closed before activation.** Reject unavailable execution classes,
   missing factories, incompatible entrypoints, incomplete Endpoint Sets, and
   protocol mismatches before a Module becomes ready. The Adapter cannot
   resolve a second graph or invent bindings.
6. **Prove conformance and host behavior.** Run the product-neutral runtime
   conformance surface, the owning repository's locked gates, target compile
   checks, and one real host smoke when process, browser, WASIp2, wire, or
   shutdown behavior changed.

Return the chosen seam and owner, host facility, core Interface version,
dependency direction, failure boundary, conformance and host-smoke evidence,
and any product behavior routed back to a Module.
