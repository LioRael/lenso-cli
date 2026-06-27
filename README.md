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
lenso console update
lenso serve
```

The package name defaults to the target directory name and can be overridden with
`--name`. Pass `--force` to scaffold into a non-empty directory.
Install or update the hosted Runtime Console with:

```sh
lenso console update
```

The command downloads the latest `lenso-runtime-console` release artifact and
installs it under `.lenso/console`, so the host API can serve `/console`
without requiring Node.js or pnpm in the host application. For local builds,
pass `--artifact <dir-or-tar.gz>`. For a pinned release, pass
`--console-version vX.Y.Z`.

After creating a password user, grant the first Runtime Console admin:

```sh
lenso console bootstrap-admin --identifier admin@example.com
# or
lenso console bootstrap-admin --user-id usr_...
```

`console.admin` is always added. Pass extra `--scope <name>` flags when the
user should also see scoped module data, then restart the API/worker.

The generated host depends on the crates.io `lenso` crate with the `host`
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

For a standalone service provider:

```sh
lenso service create support-suite-provider --lang ts --output-dir ../services
lenso service create support-suite-provider --lang rust --output-dir ../services
```

The generated provider includes a `lenso.service.json` manifest and a minimal
service process. A service name ending in `-provider` or `-service` provides a
module named without that suffix, so `support-suite-provider` provides
`support-suite`.
When this command runs from a framework checkout with sibling `lenso` and
`lenso-runtime-console` repositories, the scaffold uses local path/file
dependencies so `cargo check` or `pnpm install` can run before the packages are
published. Outside that checkout it keeps the future-publish version
dependencies and prints a note to replace them with local paths until
`lenso-service` and `@lenso/service-kit` are published.

The older standalone module package generator is still available as:

```sh
lenso module create billing --remote --output-dir ../module-packages
```

The Runtime Console package generator is available directly as:

```sh
lenso console package create billing
```

## Install a module

```sh
lenso module install auth
lenso module install auth-password
lenso module install auth-oidc
lenso module install auth-device
```

`module install` reads `source` from the module descriptor when one is present.
For V5 service-backed modules, `module install <name>` is the business-capability
entrypoint: the catalog resolves the provider service, installs it when needed,
then enables the requested module.

Install a service directly when you already have a service manifest URL:

```sh
lenso service install https://example.com/lenso/service/v1/manifest
lenso service install ./lenso.service.json
```

Service installs update `REMOTE_MODULES`, copy declared Runtime Console bundles to
`.lenso/console/extensions`, update `.lenso/console/extensions/registry.json`,
and record `.lenso/module-installs.json` in one step. Linked modules update the
host `Cargo.toml`, `src/lib.rs`, `.env` toggle, and the same install receipt
from the descriptor's `linked` section. `module add` remains a compatibility
alias for service installs.

Legacy `lenso module install <manifest-url>` still works for one compatibility
window, but prints a deprecation warning. Use `lenso service install <manifest>`
for process manifests and `lenso module install <module-name>` for business
modules.

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

Service module manifests may also declare `install.env` values and
`install.commands`. Env values are written to `.env`; commands are run only when
you pass:

```sh
lenso service install https://example.com/lenso/service/v1/manifest --run-install-commands
```

For long-running service backends, declare `install.services`. These are
stored in `.lenso/module-services.json` and started before the host loads
service-provided modules on API/worker startup. Services started by the host are tracked with
`.lock`/`.pid` files and stopped when the owning API/worker process exits;
services that are already ready before startup are treated as external and are
not stopped by the host.

During local development, start declared service providers and then the host
with:

```sh
lenso service dev
lenso service dev --skip-db --skip-migrate
```

Diagnose installed service state with:

```sh
lenso service doctor
lenso service doctor billing
lenso service doctor billing --json
lenso service check billing --json
```

The doctor reads `REMOTE_MODULES`, `.lenso/module-installs.json`, and
`.lenso/module-services.json`. It reports whether the service is
installed, configured, whether an HTTP manifest is reachable, whether managed
service `readyUrl` endpoints are ready, and which stale `.lock`/`.pid` files
may be blocking a host-started service.

Export declared service processes as a Compose fragment when handing the
service to deployment tooling:

```sh
lenso service export --module billing --format compose
```

If a manifest declares incompatible `compatibility` metadata, install stops
before writing host-local state. Use `--allow-incompatible` only when an
operator deliberately accepts that override.

Remove the local service source, install receipt, service state, Runtime
Console extension registry entry, and copied bundle files with:

```sh
lenso service uninstall billing-service
```

Use `--source linked` only when you need to force the loading source. Prefer
descriptors with a `source` field for new installs.

```sh
lenso module install auth --source linked
lenso module uninstall auth --source linked
```
