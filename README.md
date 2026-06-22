# lenso-cli

Command-line interface for the Lenso backend framework.

## Install

```sh
npm install -g @lenso/cli
# or
cargo install lenso-cli
```

## Scaffold a host application

```sh
lenso host init my-app
cd my-app
cp .env.example .env
lenso serve
```

The package name defaults to the target directory name and can be overridden with
`--name`. Pass `--force` to scaffold into a non-empty directory.
Release builds of `lenso-cli` also copy the bundled Runtime Console into the
new project, so the API serves it at `/console` without requiring Node.js or
pnpm in the host application.

Update the hosted console later by upgrading `lenso-cli` and running:

```sh
lenso host update-console
```

The generated host depends on the Git-pinned `lenso` crate with the `host`
feature, which is the current narrow host API for booting API, worker, and
migration entrypoints. See
[`docs/architecture/framework-public-surface.md`](https://github.com/LioRael/lenso/blob/main/docs/architecture/framework-public-surface.md)
for the host-facade roadmap.

`lenso serve` is a local development wrapper for the generated host. It starts
the template Postgres service, runs migrations, then keeps the API and worker
running until Ctrl-C. New hosts run them in one local process; pass
`--separate-worker` when you want two child processes. Use `--skip-db` or
`--skip-migrate` when you already have those steps covered.

## Scaffold a module

```sh
lenso module create billing
```

Add `--with-console` when the linked module should also get a Runtime Console
workspace package:

```sh
lenso module create billing --with-console
```

For a standalone remote package:

```sh
lenso module create billing --remote --output-dir ../module-packages
```

The Runtime Console package generator is available directly as:

```sh
lenso console-package create billing
```

## Install a module

```sh
lenso module install https://example.com/lenso/module/v1/manifest
lenso module install ./lenso.module.json
lenso module install auth
```

`module install` reads `source` from the module descriptor when one is present.
Remote modules update `REMOTE_MODULES`, copy declared Runtime Console bundles to
`.lenso/console/extensions`, update `.lenso/console/extensions/registry.json`,
and record `.lenso/module-installs.json` in one step. Linked modules update the
host `Cargo.toml`, `src/lib.rs`, `.env` toggle, and the same install receipt
from the descriptor's `linked` section. `module add` remains a compatibility
alias for remote installs.

Install descriptor profiles let a module expose optional setup without baking
module-specific choices into the CLI. For Redis-backed auth sessions:

```sh
lenso module install auth --profile redis-session-cache
```

The `auth` descriptor applies that profile by enabling the
`lenso-module-auth` dependency's `redis` Cargo feature, writing
`REDIS_URL=redis://localhost:6379/0` to `.env`, and recording
`auth.session_cache=redis` in `.lenso/runtime-config-defaults.json`. Provide a
Redis service separately; the starter Docker Compose file only starts Postgres
by default.

Reapply an installed module from `.lenso/module-installs.json` with:

```sh
lenso module update auth
lenso module update billing --base-url https://example.com/lenso/module/v1
```

`module update` reuses the recorded `manifestReference` and source. Remote
updates refresh `REMOTE_MODULES`, service state, install receipts, and copied
Runtime Console bundles. Linked updates reapply the recorded descriptor or
builtin module entry.

Use `--no-console-extension` when you want to skip Runtime Console extension
registration.

Remote manifests may also declare `install.env` values and `install.commands`.
Env values are written to `.env`; commands are run only when you pass:

```sh
lenso module install https://example.com/lenso/module/v1/manifest --run-install-commands
```

For long-running remote module backends, declare `install.services`. These are
stored in `.lenso/module-services.json` and started before the host loads remote
modules on API/worker startup. Services started by the host are tracked with
`.lock`/`.pid` files and stopped when the owning API/worker process exits;
services that are already ready before startup are treated as external and are
not stopped by the host.

Diagnose installed remote-module service state with:

```sh
lenso module doctor
lenso module doctor billing
```

The doctor reads `REMOTE_MODULES` and `.lenso/module-services.json`, checks
service `readyUrl` endpoints, and points to stale `.lock`/`.pid` files when a
host-started service did not become ready.

Remove the local remote-module source, install receipt, service state, Runtime
Console extension registry entry, and copied bundle files with:

```sh
lenso module uninstall billing
```

Use `--source linked` only when you need to force the loading source. Prefer
descriptors with a `source` field for new installs.

```sh
lenso module install auth --source linked
lenso module uninstall auth --source linked
```
