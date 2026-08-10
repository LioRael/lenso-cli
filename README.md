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

## Install Lenso Console

Lenso Console is installed as an independent Service, not embedded into a
business Service. Obtain an official GitHub-attested Console Release Manifest,
review the deterministic plan, and then approve that exact plan digest:

```sh
lenso console install --manifest lenso-console-release.json \
  --root /srv/lenso-console --output install-plan.json
lenso console install --manifest lenso-console-release.json \
  --root /srv/lenso-console --env-file /secure/console.env --apply \
  --approve-plan-digest sha256:<reviewed-plan-digest>
lenso console doctor --root /srv/lenso-console \
  --live-url https://console.example.com --json
```

The manifest must be attested by the `LioRael/lenso-console` repository's
repository-owned `.github/workflows/release-oci.yml` signer and must pin an OCI image by digest. The
CLI rejects attestations from any other workflow and from self-hosted runners.
The apply environment must set `CONSOLE_RECOVERY_MODE=normal` explicitly. The
adapter pulls that image, runs its migration workload, starts the Console
workload, waits up to two minutes for container health, and then requires the
running `/health/authority` response to identify `lenso-console` in `normal`
mode. It records the canonical deployment and state only after both checks pass.
The secret-free `lenso.console-installation-state.v2` evidence retains that
exact authority probe and its canonical digest, so doctor can detect later
evidence drift without trusting the live endpoint alone.
A failed readiness or authority check preserves the previous canonical files
and state instead of recording the candidate as installed. `lenso console
doctor --live-url <console-url>` independently checks both endpoints before a
retry or intervention. Upgrade uses the same
protocol through `lenso console upgrade`. An upgrade that declares irreversible
migrations additionally requires
`--approve-irreversible`. Secrets remain in the operator-owned environment file
and are never copied into the plan, manifest, or installation state.

Before producing an upgrade plan, the CLI revalidates the installed manifest's
GitHub attestation, requires the manifest and generated Compose deployment to
exactly match the applied state, and requires the target version to be strictly
newer. Run `lenso console doctor` and resolve any local drift rather than editing
the CLI-owned installation files.

Every applied change also updates a secret-free `installation-attempt.json` with
its target release, approved plan digest, current phase, and final status. Doctor
reports an interrupted or failed attempt until a later change commits cleanly;
database credentials and other environment values are never included.

Apply holds an exclusive OS-backed `installation.lock` from the state reread
through readiness and evidence commit. A concurrent install or upgrade fails
without mutation. If the process exits without releasing the lock, the operating
system releases ownership while the active record remains as crash evidence;
doctor reports it as recoverable and the next apply safely claims the same lock.

Create a Recovery Set without ever writing plaintext Store bytes to disk. The
CLI verifies the installed release evidence and attestation, holds the same
installation lock used by upgrade, streams PostgreSQL custom-format output
directly into `age`, and atomically publishes a new output directory:

```sh
lenso console backup --root /srv/lenso-console \
  --env-file /secure/console.env --output ./console-recovery-2026-07-30 \
  --recipient age1example
```

The host must provide `pg_dump` and `age`. The secret database URL is read from
`CONSOLE_DATABASE_URL` in the environment file and is passed only through the
child-process environment. `recovery-set.json` binds the encrypted payload to
the exact release, image, Store schema, composition, contract, and configuration
digests. Live session rows under `auth.sessions` are explicitly excluded and the
exclusion is recorded in the protected manifest. Existing output is never
overwritten.

Restore is also a plan-and-approval operation. Use a current environment with
`CONSOLE_RECOVERY_MODE=normal` and a recovery environment with
`CONSOLE_RECOVERY_MODE=restore` that points to a distinct, empty PostgreSQL
database:

```sh
lenso console restore --root /srv/lenso-console \
  --recovery-set ./console-recovery-2026-07-30 \
  --current-env-file /secure/console.env \
  --recovery-env-file /secure/console-recovery.env \
  --output restore-plan.json
lenso console restore --root /srv/lenso-console \
  --recovery-set ./console-recovery-2026-07-30 \
  --current-env-file /secure/console.env \
  --recovery-env-file /secure/console-recovery.env \
  --apply --approve-plan-digest sha256:<reviewed-plan-digest> \
  --identity-file /secure/console-recovery-identity.txt
```

Before fencing the current deployment, apply verifies the Recovery Set content
digest, proves that the owner-only `age` identity decrypts a readable PostgreSQL
archive, and confirms that the isolated target Store has no relations. It then
stops the previous Console, streams decryption directly into a transactional
`pg_restore`, and starts the Console against the recovery Store. No plaintext
Store file is written. A failed or completed restore writes durable
`recovery-state.json` evidence, and `lenso console doctor` remains failed while
that evidence exists. Successful restore therefore means “awaiting
reconciliation,” not activation: the CLI never changes recovery mode back to
normal or declares the restored deployment authoritative.

After restore, reconcile the passive Store with separately collected deployment,
identity/enrollment, and Outbox evidence. The reviewed input must name every
managed Service exactly as observed in the restored Store, in `serviceId` order,
and reference external evidence without embedding credentials:

```json
{
  "schema": "lenso.console-reconciliation-input.v1",
  "recoverySetId": "rcv_<uuid>",
  "observedAtUnixMs": 1785369600000,
  "reviewedBy": "operator:alice",
  "authorityEvidenceRef": "change:console-dr-42",
  "identityEvidenceRef": "audit:identity-continuity-42",
  "outboxEvidenceRef": "audit:outbox-reconciliation-42",
  "singleAuthoritativeDeployment": true,
  "identityAndEnrollmentContinuityVerified": true,
  "outboxReconciled": true,
  "managedServices": []
}
```

```sh
lenso console recovery reconcile --root /srv/lenso-console \
  --env-file /secure/console-recovery.env \
  --evidence reconciliation-input.json --output reconciliation-plan.json
lenso console recovery reconcile --root /srv/lenso-console \
  --env-file /secure/console-recovery.env \
  --evidence reconciliation-input.json --apply \
  --approve-plan-digest sha256:<reviewed-plan-digest>
```

The command reads the restored Store without mutation, rejects any browser
session predating recovery while allowing newly authenticated recovery
operators, binds a streamed digest of exact Outbox rows plus status counts and
the managed-Service identity set, and writes content-addressed
`reconciliation-evidence.json`. It advances only to
`ready_for_activation`; doctor continues to fail and neither the Worker nor
management mutations are enabled.

Generated deployments set `CONSOLE_RECOVERY_MODE` explicitly. `normal` runs the
API and Worker; `restore` keeps the inspection API available while the Console
Service suppresses background work and rejects management mutations. Recovery
mode can only be changed through the external installation authority. During
activation and activation recovery, the CLI reads the running container's
minimal `/health/authority` response and requires the exact Console Service
identity plus the expected `normal` or `restore` mode before committing durable
evidence. A healthy process with the wrong workload mode is treated as a failed
authority transfer and is fenced again.

After installing and starting the independent Lenso Console Service, create its
first password user and bootstrap that user as the first Console Operator from
outside the Service. In an interactive terminal, the CLI securely prompts for
and confirms the password without echoing it:

```sh
lenso console operator bootstrap \
  --console-root ../lenso-console \
  --console-url http://127.0.0.1:3030 \
  --identifier admin@example.com
```

The command grants only the Console Minimum operator scopes plus explicit
`--scope <name>` additions, writes append-only audit evidence, and refuses to
run after an operator grant already exists. It also verifies the mandatory
System Registry state before writing, so a business Service Store is rejected.
Password-user creation goes through the Console Service's own Auth Module over
HTTPS, with loopback HTTP allowed for local installation. Non-interactive
automation must use `--password-stdin` or a private regular file through
`--password-file`. For recovery after Auth registration succeeded but the grant
did not, rerun without `--console-url` or a password option and select the
existing identity with `--identifier` or `--user-id`. Restart the Console API
and Worker after bootstrapping. Business Service users and Auth state are never
modified.

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

Add `--with-console-ui` when the Module Release should also contain its verified
same-realm ESM Console UI artifact:

```sh
lenso module create billing --with-console-ui
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

For a new product-shaped application, materialize one exact App Composition
instead of layering generated state. The blueprint and addons are authoring
recipes; `lenso.app.json` is the only application composition and lock, with
immutable Module release digests, implementation bindings, resolved dependency
selections, and an optimistic revision:

```sh
lenso app compose ./support-desk --blueprint support-desk --apply
lenso app compose --repo-root ./support-desk \
  --implementation support-api=linked \
  --observed-revision 1 --apply
lenso system dev --system-file ./support-desk/lenso.app.json --dry-run --json
lenso system dev --system-file ./support-desk/lenso.app.json
```

`lenso system dev` realizes service-backed bindings through a persistent Local
Control Adapter. The coordinator may exit after reporting workload identities;
the adapter and its explicitly started local workloads remain active. Use
`lenso system dev --system-file ./support-desk/lenso.app.json --cleanup` to
stop only the adapter-owned workloads. This path does not create a second App
lock or copy process commands into `lenso.app.json`.

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

Install and manage a Module through the stable lifecycle commands:

```sh
lenso module install dist/lenso-service/support-suite-provider/modules/support-ticket/lenso.module-release.json \
  --base-url http://127.0.0.1:4100/lenso/service/v1
lenso module disable support-ticket
lenso module remove support-ticket
lenso module doctor support-ticket
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

The Service scaffold consumes `@lenso/service-kit` from the framework SDK when
used in a sibling checkout. Outside that checkout it uses the published SDK.

### Console and Module UI development

Run the complete local Console Service from its repository:

```sh
lenso console dev --console-root ../lenso-console
```

Run the Console UI artifact owned by the current Module:

```sh
lenso module dev --console-ui
```

The UI artifact remains bound to the same immutable Module Release. The CLI no
longer copies packages into a Console checkout or maintains extension registries.

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

Service installs update `SERVICE_MODULES` and record `.lenso/module-installs.json`
in one step. Console UI artifacts remain immutable members of their Module
Release and are bound only by an applied Console Service Composition. Linked modules update the
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

`module update` reuses the recorded `manifestReference` and delivery form.
Service-backed updates refresh provider state and install receipts; Linked
updates reapply the recorded descriptor or builtin Module entry.

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

The doctor reads `SERVICE_MODULES`, `.lenso/module-installs.json`, and
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

## M6 GA support operations

The GA commands consume exact versioned evidence and never infer compatibility
from nearby semantic versions:

```sh
lenso ga support-check --manifest lenso.ga-support-manifest.v1.json \
  --component cli:@lenso/cli@0.1.30 \
  --component runtime:lenso-service@0.1.4 \
  --state-version service-store.v1 --json

lenso ga manifest-migrate --manifest lenso.ga-support-manifest.v1.json \
  --source lenso.system.json --target-format lenso.system.v2 \
  --identity-pointer /systemId --dry-run --json

lenso ga service-upgrade --manifest lenso.ga-support-manifest.v1.json \
  --input service-upgrade-input.json --json

lenso ga contract-retire --input contract-retirement-input.json --json
lenso ga failure-evaluate --input failure-scenario.json --json
```

Manifest migration and Service upgrade are non-mutating plans by default.
Contract Retirement does not apply without an exact human approval bound to
the current plan digest. Unknown combinations, stale inputs, active Consumers,
incomplete deprecation windows, incompatible state, and unexpected failure
behavior stop with stable issue codes and next actions.
