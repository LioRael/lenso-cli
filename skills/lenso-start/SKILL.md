---
name: lenso-start
description: Choose one Lenso vNext development workflow.
---

# Lenso Start

Route the request by its owner. This is the human-invoked index; the selected
skill owns the work.

## Route

1. State the requested outcome without framework nouns.
2. Choose exactly one primary workflow:
   - unclear product behavior, ownership, or boundaries ->
     `lenso-business-planning`
   - Capability identity, Operations, Schemas, compatibility, or generated
     consumer/provider bindings -> `lenso-capability-authoring`
   - product behavior implemented as a Rust, Bun, Web, stateful, Auth, Story,
     Audit, OpenTelemetry, Secrets, or other Module ->
     `lenso-module-authoring`
   - package selection, keyed Module Instances, configuration, bindings,
     placement, Web profiles, or Resolved App Plan ->
     `lenso-app-composition`
   - scheduling, clocks, Module generation, endpoint mechanics, process or
     transport integration, execution classes, or Runner orchestration ->
     `lenso-runtime-extension`
3. Treat portable graph, lifecycle, invocation, admission, supervision,
   readiness, and diagnostic semantics as Kernel work. Read the core
   repository's `CONTEXT.md` and relevant ADR rather than routing it through a
   product skill.
4. Name a secondary workflow only when the request crosses a real ownership
   boundary. Continue with the primary skill when it is available; otherwise
   report the missing catalog entry.

Routing is complete when one owner and one observable completion state are
unambiguous. Ask one boundary question only when two owners still fit.
