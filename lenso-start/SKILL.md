---
name: lenso-start
description: Use when choosing the right public Lenso path from scratch, especially before installing packages, authoring a module, starting a host app, or wiring a remote module.
---

# Lenso Start

## Overview

Use this skill to pick the right public Lenso entrypoint before writing code.
Start from the user goal, then route them to the smallest public surface that fits.

## Public Paths

- Rust module on the host: use `cargo add lenso@0.1.0`
- Remote module: use `pnpm add @lenso/remote-module-kit@0.1.1`
- Host starter app: scaffold with the standalone CLI via `lenso host init <dir>`
- OpenAPI client work: use the committed `contracts/openapi/app-api.v1.yaml`

## Decision Rule

- If the user wants to define manifests, routes, runtime functions, events, lifecycle checks, or console metadata in Rust, use `lenso-module-authoring`.
- If the user wants to build an out-of-process module in JavaScript or TypeScript, use `lenso-remote-module-authoring`.
- If the user wants to run a blank backend host, use `lenso-starter-host`.
- If the user wants to consume or verify HTTP APIs, use `lenso-api-client`.

## Good First Reply

Ask for the target path if it is still unclear.
Keep the next step narrow and public.
