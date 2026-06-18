# lenso-cli

Command-line interface for the Lenso backend framework.

## Install

```sh
cargo install lenso-cli
```

## Scaffold a host application

```sh
lenso host init my-app
cd my-app
cp .env.example .env
docker compose up -d postgres
cargo run --bin migrate
cargo run --bin api
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
Remote modules update `REMOTE_MODULES`, write the local console package install
plan, record `.lenso/module-installs.json`, and apply Runtime Console package
registration when the manifest declares console packages. Linked modules update
the host `Cargo.toml`, `src/lib.rs`, `.env` toggle, and the same install
receipt from the descriptor's `linked` section. `module add` remains a
compatibility alias for remote installs.

Use `--runtime-console-root` when the console app lives outside the host
repository, and `--no-console-plan` when you want to apply the plan later with:

```sh
lenso console-package apply-plan
```

Remote manifests may also declare `install.env` values and `install.commands`.
Env values are written to `.env`; commands are recorded in the plan and run only
when you pass:

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

Remove the local remote-module source, install receipt, and pending console
package plan with:

```sh
lenso module uninstall billing
```

Use `--source linked` only when you need to force the loading source. Prefer
descriptors with a `source` field for new installs.

```sh
lenso module install auth --source linked
lenso module uninstall auth --source linked
```
