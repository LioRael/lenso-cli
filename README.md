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
lenso service create support-suite-provider --lang rust --output-dir ../services --port 4110
```

The generated provider includes a `lenso.service.json` manifest and a minimal
service process. A service name ending in `-provider` or `-service` provides a
module named without that suffix, so `support-suite-provider` provides
`support-suite`.
`service create` also updates `lenso.workspace.json` unless `--no-workspace` is
set. That workspace file is the local service plane for development:

```sh
lenso service workspace list
lenso service dev
```

`lenso service dev` starts workspace services first, then starts declared
installed services from `.lenso/module-services.json`, then runs the host.
Workspace reads prefer `lenso.workspace.json` and also accept the older
`.lenso/services.json` path for compatibility.

For a System v2 graph containing multiple Autonomous Services, use the
clusterless System Sandbox on macOS or Linux:

```sh
lenso system dev --dry-run --json
lenso system dev
lenso system dev --scenario deadline-timeout --json
lenso system dev --cleanup
```

The System graph remains in `lenso.system.json`. Local-only executable details
live beside it in `lenso.system-sandbox.json`:

```json
{
  "protocol": "lenso.system-sandbox.v1",
  "services": [{
    "serviceId": "support",
    "workloads": [{
      "workloadId": "support-migrate",
      "command": ["cargo", "run", "--bin", "support-migrate"]
    }, {
      "workloadId": "support-api",
      "command": ["cargo", "run", "--bin", "support-api"],
      "scenarioCommand": ["cargo", "run", "--bin", "support-scenario-driver"],
      "endpoint": "http://127.0.0.1:4110",
      "healthUrl": "http://127.0.0.1:4110/health/ready"
    }, {
      "workloadId": "support-worker",
      "command": ["cargo", "run", "--bin", "support-worker"]
    }]
  }],
  "scenarios": [{
    "scenarioId": "deadline-timeout",
    "fault": {
      "kind": "timeout",
      "serviceId": "support",
      "workloadId": "support-api",
      "delayMs": 100
    },
    "callPolicy": {
      "deadlineMs": 100,
      "maxAttempts": 2,
      "idempotent": true
    }
  }]
}
```

Failure controls are inert during ordinary startup and dry-run. They are read
only from the local System Sandbox definition and activated only when an
explicit `--scenario <scenarioId>` is supplied. Supported fault kinds are
`timeout`, `slow_dependency`, `workload_crash`, `overload`, and
`partial_unavailability`. Timeout and slow-dependency decisions use controlled
scenario time; overload uses declared `capacity` and `demand`, never machine
pressure.

Timeout, slow-dependency, and overload scenarios require the affected
Workload's `scenarioCommand`. The Sandbox invokes that Workload-owned adapter
only for an explicit scenario, supplies `LENSO_SANDBOX_*` controlled-time,
fault, capacity, and Call Policy inputs, and accepts one
`lenso.sandbox-workload-observation.v1` JSON result. The adapter exercises the
Service's real local call or dependency path; normal Workload startup never
receives those failure-control inputs.

Each run emits `lenso.failure-scenario-result.v1` JSON, performs Sandbox-owned
process and state cleanup, and writes the equivalent durable Story Segment to
`.lenso/system-sandbox-results/<systemId>/<scenarioId>/story-segment.json`.
Results include the injected fault, affected Service and Workload, attempt and
retry evidence, Call Policy and health transitions, outcome, cleanup, and next
actions. Repeating the same declared scenario overwrites that evidence with an
equivalent result.

Dry-run performs the same definition, cwd, executable, URL, graph, and
dependency validation as launch without creating Store directories, state, or
processes. Launch assigns each Workload an explicit `local-dev://` identity,
allocates one sandbox-owned Store path per Service, waits for declared health,
and records correlated endpoint and process state under
`.lenso/system-sandbox/<systemId>`. This identity is development-only and
does not claim production authentication. Ctrl-C and `--cleanup` terminate
only token-proven sandbox processes or their process groups, and remove state
only when its ownership marker
matches; Kubernetes, a Host, service mesh, external broker, System Plane, and a
production identity provider are not required.
Host and Provider declarations may remain in the System graph for topology
validation, but the sandbox neither starts nor contacts them.

## Assess linked Module extraction

Report whether one Host-owned linked Module is ready to move behind an
Autonomous Service boundary:

```sh
lenso module extraction readiness support-ticket \
  --module-manifest modules/support-ticket/lenso.module.json \
  --system-file lenso.system.json \
  --evidence-file support-ticket.extraction-evidence.json \
  --json
```

The CLI scans Rust Module Cargo dependencies, imports, and fully qualified
in-process calls under `modules/`. The evidence file supplies authoritative
Service/Event Contract mappings and active Consumer compatibility results;
omitting or supplying ambiguous evidence produces a blocked report. Human and
JSON output come from the same `lenso.extraction-readiness-report.v1` artifact.
Blocked reports exit non-zero so the command can gate CI.

Readiness analysis is read-only: it does not write repository files, start
Workloads, move data, or change authority. Use `--repo-root` and
`--modules-root` when the Module sources are not under the current repository's
default `modules/` directory.

Generated TS and Rust services also support `--check-release` to print the
development module release descriptor before packaging.
Before handing a service to another app or deployment pipeline, package-check
the project and then emit a local service artifact:

```sh
cd ../services/support-suite-provider
lenso service package --check
lenso service package --output-dir dist/lenso-service
```

The package artifact contains the canonical `lenso.service.json`,
`lenso.service-package.json`, and one
`modules/<module>/lenso.module.json` plus
`modules/<module>/lenso.module-release.json` file for each provided module.
The service package records the provider name, version, and provided module
names; each module release is the business-module install entrypoint.
Operators can install a provider directly. For a local package artifact, still
pass the runtime service base URL:

```sh
lenso service install dist/lenso-service/support-suite-provider/lenso.service-package.json \
  --base-url http://127.0.0.1:4100/lenso/service/v1
```

Install a packaged module release with the module command:

```sh
lenso module release inspect dist/lenso-service/support-suite-provider/modules/support-ticket/lenso.module-release.json
lenso module release check dist/lenso-service/support-suite-provider/modules/support-ticket/lenso.module-release.json \
  --base-url http://127.0.0.1:4100/lenso/service/v1
lenso module install dist/lenso-service/support-suite-provider/modules/support-ticket/lenso.module-release.json \
  --base-url http://127.0.0.1:4100/lenso/service/v1
lenso module enable support-ticket
lenso module disable support-ticket
```

`lenso.module-release.v1` is the module release channel. It records the module
name, version, capabilities, source, and optional provider pointer. V11 keeps
`lenso module install` as the unified business-capability entrypoint:

- `source: service` resolves to a provider service package or service manifest.
- `source: linked` enables linked Rust code in the host.
- `source: bundled` enables a host-bundled module.

`lenso service install` remains the lower-level provider/process command. It
connects a service, but it does not mean every module inside that service is
the user-facing install target.

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

### Runtime Console package development

Preview a console package while editing it:

```sh
lenso console dev --package packages/auth-console
```

From a module repository root, discover every local console package:

```sh
lenso module dev --console
```

Both commands default to standalone mock mode. Add `--host` to proxy real Lenso
host APIs while still loading the local package bundle:

```sh
lenso module dev --console --host http://localhost:3000
```

Set `LENSO_RUNTIME_CONSOLE_ROOT=/path/to/lenso-runtime-console` when the Runtime
Console checkout is not a sibling of the current repository.

## Install a module

```sh
lenso module install auth
lenso module install auth-password
lenso module install auth-oidc
lenso module install auth-device
```

`module install` reads `source` from the module descriptor when one is present.
When the reference is a module name, the CLI resolves it from the official
catalog at `https://catalog.lenso.dev/v1/modules.json` unless `--catalog-url`
points at another registry. If the primary official catalog endpoint is
temporarily blocked by edge security, the CLI falls back to the official
workers.dev mirror at `https://lenso-catalog.lenso.workers.dev/v1/modules.json`.
For V5 service-backed modules, `module install <name>` is the business-capability
entrypoint: the catalog resolves the provider service, installs it when needed,
then enables the requested module.
For module releases, `module install <module-release.json>` resolves the
release by source, then records `moduleRelease` provenance in
`.lenso/module-installs.json` where the source supports a receipt.

Install a service directly when you have a workspace service name or manifest
reference:

```sh
lenso service install support-suite-provider
lenso service install https://example.com/lenso/service/v1/manifest
lenso service install ./lenso.service.json --repo-root ../my-lenso-host
```

When the first argument matches a service in `lenso.workspace.json` or
`.lenso/services.json`, the CLI resolves its manifest and infers `--base-url`
from the service `readyUrl`. Local source manifests registered in the workspace
also infer `--base-url`; package artifacts outside that workspace still need
`--base-url` so the host records the runtime service endpoint rather than the
file path.

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
lenso service dev --workspace-file lenso.workspace.json
```

After the service processes are running, check the workspace from another shell:

```sh
lenso service workspace check
lenso service workspace check support-suite-provider --json
lenso service verify
lenso service verify support-suite-provider --json
lenso service verify ./lenso.service.json --env-file .env --json
```

Use `lenso service dev --no-workspace` when only installed
`.lenso/module-services.json` providers should start.

`lenso service workspace check` verifies that each declared service directory
exists, its manifest is reachable, and its `readyUrl` is responding before the
host tries to load the provider.

`lenso service verify` is the release-readiness entrypoint. With no argument it
checks `./lenso.service.json`; with a provider name it reuses the installed
service doctor checks. Pass `--env-file` to include required/missing service env
in the verification report.

Preview service upgrade impact before writing host-local state:

```sh
lenso service upgrade-plan billing ./lenso.service.json --json
lenso service upgrade billing ./lenso.service.json --dry-run
```

Export workspace services into the host service-start state format when a script
or deployment handoff should consume the same service declarations:

```sh
lenso service workspace export --output .lenso/module-services.json
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
