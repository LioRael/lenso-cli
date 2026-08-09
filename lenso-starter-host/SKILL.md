---
name: lenso-starter-host
description: Create, run, or repair a thin Lenso host application, including its generated API, Worker, Migration, Postgres, Module wiring, local serve loop, and Console Service connection. Use for host-shell work after product capabilities have been separated from host responsibilities.
---

# Lenso Starter Host

Build the smallest runnable composition root around owned Modules and Services.
Keep business behavior out of the host shell.

## Workflow

1. **Resolve the host.** Determine whether this is a new scaffold, a generated
   Launchpad app, or an existing custom host. Inspect the current `lenso host
   --help` and `lenso serve --help`. Finish when the owning repository and
   generated versus user-owned files are known.
2. **Confirm the boundary.** Read [host boundary](references/host-boundary.md).
   List the API, Worker, Migration, database, Module registrations, Service
   sources, shared policy anchors, and configuration the host must compose.
   Finish when every business behavior has a Module or Service owner.
3. **Scaffold or repair narrowly.** Prefer the current CLI scaffold and public
   `lenso` host facade. Preserve existing app-owned files and make the smallest
   wiring change that produces the requested runtime shape.
4. **Bring up local dependencies.** Use the generated environment example and
   repository-owned commands. Start migration before API and Worker when the
   generated project requires it. Finish when startup output identifies the
   actual database, ports, loaded Modules, and declared Services.
5. **Prove the host.** Follow [verification](references/verification.md).
   Finish when a focused check proves the binaries compile, one real host path
   crosses the composition boundary, and the configured Console Service can
   observe the intended capability.

## Report

Return the host path, files changed, startup command, Module and Service wiring,
focused verification, Console URL or reason it is unavailable, and any work
that belongs in a downstream authoring skill.
