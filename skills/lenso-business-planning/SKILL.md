---
name: lenso-business-planning
description: Plan a Lenso vNext application around Modules and Capabilities.
---

# Lenso vNext business planning

Describe the intended App in terms of owned Modules, Capabilities, Operations,
and their user-visible outcomes.

- Give every Capability one owning Module.
- Keep Module boundaries vertical and explicit.
- Record the public Interface and Operation identity before implementation.
- Separate product behavior from Kernel and Runtime Driver concerns.
- Use the accepted ADRs in `docs/adr/` as the source of architectural truth.

The output should be a small implementation-ready slice, not a return to a
repository-wide registry, host supervisor, or legacy platform plan.
