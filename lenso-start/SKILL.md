---
name: lenso-start
description: Use when choosing the right public Lenso path from scratch, especially before installing packages, authoring a module, starting a host app, or wiring a remote module.
---

# Lenso Start

## Overview

Use this skill to pick the right public Lenso entrypoint before writing code.
Start from the user goal, then route them to the smallest public surface that fits.

## Public Paths

- Business planning from a vague prompt: use `lenso-business-planning`
- Rust module on the host: use `cargo add lenso@0.3.10`
- Remote module: use `pnpm add @lenso/remote-module-kit@0.1.1`
- Host starter app: scaffold with the standalone CLI via `lenso host init <dir>`
- OpenAPI client work: use the committed `contracts/openapi/app-api.v1.yaml`

## Decision Rule

- If the user has a broad business idea but unclear actors, workflows, data ownership, or module boundaries, use `lenso-business-planning`.
- If the user wants to define manifests, routes, runtime functions, events, lifecycle checks, or console metadata in Rust, use `lenso-module-authoring`.
- If the user wants to build an out-of-process module in JavaScript or TypeScript, use `lenso-remote-module-authoring`.
- If the user wants to run a blank backend host, use `lenso-starter-host`.
- If the user wants to consume or verify HTTP APIs, use `lenso-api-client`.

## Good First Reply

Ask for the target path if it is still unclear.
Keep the next step narrow and public.

## Agent Output

Return the chosen path, the next command, and the follow-up skill:

- vague business idea -> clarify module plan -> `lenso-business-planning`
- host app -> `lenso host init <dir>` -> `lenso-starter-host`
- in-host Rust module -> `lenso module create <name>` -> `lenso-module-authoring`
- remote module -> `lenso module create <name> --remote` -> `lenso-remote-module-authoring`
- API client -> committed OpenAPI contract -> `lenso-api-client`

## Keep Out

- Do not design a new framework surface before choosing an existing public path.
- Do not recommend cloning this repository when a published package or CLI fits.
