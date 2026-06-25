# Lenso Starter Host

Minimal host application skeleton for running Lenso as a backend framework from
a blank Rust project.

This template depends on the crates.io `lenso` package with the `host` feature
enabled. Pin the dependency version for reproducible builds.

The binary entrypoints are deliberately thin wrappers around the `lenso::host`
facade. That keeps this template on the current host API without exposing the
boot or platform crates as its user-facing API.

## Start

```sh
cp .env.example .env
lenso serve
```

`lenso serve` runs the API and worker in one local process. For manual or
separate-process startup:

```sh
docker compose up -d postgres
cargo run --bin migrate
cargo run --bin api
cargo run --bin worker
```

If local port 5432 is already in use, change `POSTGRES_HOST_PORT` in `.env`.

The API binds to `HTTP_HOST:HTTP_PORT` from `.env` and serves:

- `GET /livez`;
- `GET /readyz`;
- `GET /v1/app/status`;
- `GET /v1/app/items` for the authenticated user;
- `GET /v1/app/items/{id}` for the authenticated user;
- `PATCH /v1/app/items/{id}` for the authenticated user;
- `DELETE /v1/app/items/{id}` for the authenticated user;
- `POST /v1/app/items` for the authenticated user;
- `GET /openapi.json`;
- `GET /docs`;
- `POST /v1/auth/password/register`;
- `POST /v1/auth/password/login`;
- `GET /.well-known/openid-configuration`;
- `GET /.well-known/jwks.json`;
- `GET /oauth/authorize`;
- `POST /oauth/token`;
- Runtime Console frontend under `/console`;
- Runtime Console admin APIs under `/admin/*`;
- installed remote module HTTP proxies under `/modules/{module}/http/*`.

The starter uses `LENSO_COMPOSITION_PROFILE=demo`, which includes the
first-party auth anchor, password auth, and OIDC provider modules. Local
development can still use `Bearer dev-user:<id>` tokens. Set
`LENSO_COMPOSITION_PROFILE=core` when you want only core platform modules and
then install auth explicitly:

```sh
lenso module install auth
lenso module install auth-password
lenso module install auth-oidc
```

The OIDC provider is loaded but disabled until configured. Runtime Console
sign-in uses this registered redirect URI:

```text
http://127.0.0.1:3000/console/oidc/callback
```

Set `LENSO_MODULE_AUTH_OIDC__ENABLED=true` only with a real issuer, JWKS,
RSA signing key, and `CONSOLE_REDIRECT_URIS` that includes the callback above.
When writing JSON values in `.env`, wrap the whole JSON value in single quotes
so dotenv preserves the inner double quotes.

After registering a password user, grant the first Console admin from the host
root:

```sh
lenso host bootstrap-admin --identifier admin@example.com
# or
lenso host bootstrap-admin --user-id usr_...
```

Restart `api` and `worker` after bootstrapping; Console admin scopes are loaded
at startup.

## Add A Remote Module

Start a module that exposes a Lenso manifest, then install it into `.env`:

```sh
lenso module install http://127.0.0.1:4100/lenso/module/v1/manifest
```

Restart `api` and `worker` after changing module configuration.

User-facing remote-module examples live in
<https://github.com/LioRael/lenso-examples>.

## Add A Linked Module

Local Rust modules are registered from `src/lib.rs`:

```rust
use lenso::host::prelude::*;

HostBuilder::new()
    .linked_module(modules::app::linked_module())
    .build()
```

The included `src/modules/app` module is a project-owned skeleton. Rename it
or add modules beside it as your backend grows. It declares a small status
route, an `app.items` table, and item read/write routes so the module has
visible metadata and a real HTTP/data surface in the host registry;
replace them with your real application capabilities as the module grows.
The item table is intentionally app-owned and keyed by `owner_user_id`, which
comes from `ActorContext::User.user_id`. This is the pattern to use for product
profiles, accounts, and other user data instead of adding profile fields to
Lenso's auth anchor.

When the module owns tables, pass its migration list through
`HostLinkedModule::manifest_only(...)`.

The starter's `app` module already includes a first migration:

```text
src/modules/app/migrations/0001_create_app_schema.sql
```

Add application tables there or create another numbered migration beside it,
then run:

```sh
cargo run --bin migrate
```

Add HTTP routes through `src/modules/app/routes.rs`, declare their manifest
metadata in `src/modules/app/mod.rs`, then restart the API.

Create and read starter data:

```sh
curl -sS -X POST http://127.0.0.1:3000/v1/app/items \
  -H 'content-type: application/json' \
  -H 'authorization: Bearer dev-user:usr_demo' \
  -d '{"title":"first item"}' | jq .

curl -sS http://127.0.0.1:3000/v1/app/items \
  -H 'authorization: Bearer dev-user:usr_demo' | jq .

curl -sS http://127.0.0.1:3000/v1/app/items/1 \
  -H 'authorization: Bearer dev-user:usr_demo' | jq .

curl -sS http://127.0.0.1:3000/v1/app/items/1 \
  -H 'authorization: Bearer dev-user:usr_other' | jq .

curl -sS -X PATCH http://127.0.0.1:3000/v1/app/items/1 \
  -H 'content-type: application/json' \
  -H 'authorization: Bearer dev-user:usr_demo' \
  -d '{"title":"renamed item"}' | jq .

curl -sS -X DELETE http://127.0.0.1:3000/v1/app/items/1 \
  -H 'authorization: Bearer dev-user:usr_demo' | jq .
```

## Files

- `src/lib.rs` is the host-owned module composition hook.
- `src/modules/app` is the first project-owned linked module skeleton.
- `src/bin/migrate.rs` delegates to the host migration runner.
- `src/bin/api.rs` delegates to the host API runner.
- `src/bin/worker.rs` delegates to the host worker runner.
- `docker-compose.yml` starts local Postgres.
- `.env.example` keeps local defaults explicit.
