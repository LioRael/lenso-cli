---
name: lenso-host-starter
description: Use when creating or running a blank Lenso host app from the starter template, especially when wiring API, worker, migrations, local Postgres, or the first linked module.
---

# Lenso Host Starter

## Overview

Use the starter host when you want a runnable Lenso backend from a blank Rust project.
It is the public pressure test for host setup.

## Start Here

From `templates/starter-host`:

```sh
cp .env.example .env
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
- remote module proxying

## Guardrails

- Keep `lenso-host` temporary until the public host facade is ready.
- Keep app-owned data in the starter, not in the auth anchor.
- Keep the starter thin and explicit.

## Checks

```sh
cargo check --bins
```
