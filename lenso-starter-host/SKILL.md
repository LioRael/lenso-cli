---
name: lenso-starter-host
description: Use when creating or running a blank Lenso host app from the starter template, especially when wiring API, worker, migrations, local Postgres, or the first linked module.
---

# Lenso Host Starter

## Overview

Use the starter host when you want a runnable Lenso backend from a blank Rust project.
It is the public pressure test for host setup.

## Start Here

Scaffold with `lenso host init <dir>`, then from the generated project:

```sh
cp .env.example .env
lenso serve
```

For a generated business app, prefer the App Composer path:

```sh
lenso app compose ./acme-support --blueprint support-desk --addon support-sla --apply
lenso app next
lenso app explain
```

For a generated app with a reusable capability pack:

```sh
lenso capability init support-sla --dir ./capabilities/support-sla --lang ts --for-blueprint support-desk
lenso capability library add ./capabilities/support-sla
lenso capability fit support-sla --repo-root .
lenso app compose ./acme-support --blueprint support-desk --pack support-sla --apply
```

Use separate processes only when debugging service boundaries:

```sh
docker compose up -d postgres
cargo run --bin migrate
cargo run --bin api
cargo run --bin worker
```

## What It Covers

- API entrypoint
- worker entrypoint
- migration entrypoint
- local Postgres
- linked module wiring
- service-provided module proxying
- capability pack composition state

## Guardrails

- Use `lenso = { features = ["host"] }` as the host facade.
- Keep app-owned data in the starter, not in the auth anchor.
- Keep the starter thin and explicit.
- Keep generated hosts runnable without requiring the framework monorepo.

## Checks

```sh
cargo check --bins
```

## Agent Output

When creating or fixing a starter host, leave:

- the scaffolded project path
- the command used to start it
- the `lenso app next` or `lenso app explain` result when the host is a composed app
- the `lenso capability fit <pack>` result when the host was composed with a capability pack
- the URL for `/console` when the API is running
- one focused check result

## Keep Out

- Do not add product-specific CRUD helpers to `lenso::host`.
- Do not add service orchestration beyond the starter's local process shape.
