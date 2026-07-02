---
name: lenso-start
description: Use when choosing the right public Lenso path from scratch, especially before installing packages, authoring a module, starting a host app, or wiring a service.
---

# Lenso Start

## Overview

Use this skill to pick the right public Lenso entrypoint before writing code.
Start from the user goal, then route them to the smallest public surface that fits.

## Public Paths

- Business planning from a vague prompt: use `lenso-business-planning`
- Rust module on the host: use `cargo add lenso@0.3.16`
- Service: use `pnpm add @lenso/service-kit@0.1.0`
- Capability pack: use `lenso capability init support-sla --dir ./capabilities/support-sla --lang ts --for-blueprint support-desk`
- Capability library: use `lenso capability library add ./capabilities/support-sla`, then `lenso capability fit support-sla --repo-root .`
- Composed app: use `lenso app compose ./acme-support --blueprint support-desk --pack support-sla --apply`
- Host starter app: scaffold with the standalone CLI via `lenso host init <dir>`
- OpenAPI client work: use the committed `contracts/openapi/app-api.v1.yaml`

## Decision Rule

- If the user has a broad business idea but unclear actors, workflows, data ownership, or module boundaries, use `lenso-business-planning`.
- If the user wants a generated business app, prefer `lenso app compose`, then `lenso app next`, `lenso app explain`, and `lenso agent task --from-app-plan "add the requested business behavior"`.
- If the user wants a reusable business slice that combines modules, services, and agent handoff, create a capability pack, add it to the local capability library, run `lenso capability fit`, then compose it into the app with `--pack`.
- If the user wants to define manifests, routes, runtime functions, events, lifecycle checks, or console metadata in Rust, use `lenso-module-authoring`.
- If the user wants an independently running service in JavaScript or TypeScript, use `lenso-remote-module-authoring`.
- If the user wants to run a blank backend host, use `lenso-starter-host`.
- If the user wants to consume or verify HTTP APIs, use `lenso-api-client`.

## Good First Reply

Ask for the target path if it is still unclear.
Keep the next step narrow and public.

## Agent Output

Return the chosen path, the next command, and the follow-up skill:

- vague business idea -> clarify module plan -> `lenso-business-planning`
- business app -> `lenso app compose ./acme-support --blueprint support-desk --addon support-sla --apply` -> `lenso app next` -> `lenso agent task --from-app-plan "add the requested business behavior"`
- capability pack -> `lenso capability init support-sla --dir ./capabilities/support-sla --lang ts --for-blueprint support-desk` -> `lenso capability library add ./capabilities/support-sla` -> `lenso capability fit support-sla --repo-root .` -> `lenso app compose ./acme-support --blueprint support-desk --pack support-sla --apply` -> `lenso agent task --for-capability support-sla "add enterprise SLA escalation"`
- host app -> `lenso host init <dir>` -> `lenso-starter-host`
- in-host Rust module -> `lenso module create <name>` -> `lenso-module-authoring`
- service -> `@lenso/service-kit` -> `lenso-remote-module-authoring` -> `lenso service list` and `lenso service doctor <module> --json`
- API client -> committed OpenAPI contract -> `lenso-api-client`

## Keep Out

- Do not design a new framework surface before choosing an existing public path.
- Do not recommend cloning this repository when a published package or CLI fits.
