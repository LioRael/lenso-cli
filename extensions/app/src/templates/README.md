# Lenso application

This is editable native application source. The application runs independently of
Lenso CLI. Use Node.js 26 for TypeScript or Rust 1.99.0 for Rust.

```sh
lenso app preview
lenso app check
lenso app run
```

The server binds to loopback and exposes `GET /health`. Set `PORT` in `.env` or
when launching the native application directly. CLI-supervised commands inherit
only PATH; application configuration is loaded by the native host from `.env`.
Environment values are never written to Lenso config/lock files. `.env` is ignored.

## Add Auth

```sh
lenso code config --provider lenso.auth-store=sqlite
lenso code preview auth
lenso code add auth
lenso app migrate
lenso app run
```

The explicit `app migrate` command initializes module schemas and exits.
`app run` starts the server without migrating schemas. Auth adds `/auth/register`, `/auth/login`, `/auth/session` and
`/auth/logout`, using JSON bodies and bearer tokens. PostgreSQL settings use a
runtime environment reference; put the referenced connection in `.env`.

Code owns generated module files and dependency requests. App bootstrap hands
`src/main.*`, manifests and build settings to the application after initialization;
repeating `app init` does not overwrite user changes. `app prepare` retries native
dependency preparation. Existing unrelated native applications are not overwritten.
Preview and separate file/command approvals use the normal Project workflow.

## HTTP module protocol v1

`lenso.code.toml` records `conventions.app_host = "lenso-http-v1"`. Code selects
compatible authored module targets. TypeScript discovers `assembly/host.js` after
compilation; its `mount(migrate)` returns async `handle(request)` and `close()`.
Requests contain method, path, optional authorization and parsed JSON body;
responses contain status and body, or undefined when no route matches.
Rust's build script finds `assembly/host.rs`; each exposes async
`router(migrate) -> Result<axum::Router, Box<dyn std::error::Error>>`.
The application owns its HTTP host; modules own routing, identity, access and
persistence. The initial Auth HTTP variant mounts one root instance named `auth`.

The generated hosts read the current `paths.modules` setting. TypeScript projects
own their import-alias mappings if changing that path after creation. Server logs
are forwarded to stderr by CLI supervision, keeping JSON output machine-readable.
