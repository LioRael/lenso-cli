# lenso-cli

Command-line interface for the Lenso backend framework.

This repository also owns the `lenso-authoring` library extracted from
`LioRael/lenso` under ADR 0064. Its `add`, `check`, `resolve`, and `run`
workflows are exposed through the same `lenso` executable as the rest of the
CLI; the library does not publish a second binary.

Project-wide agent workflows live in the
[`LioRael/lenso` skill pack](https://github.com/LioRael/lenso/tree/main/skills),
so this repository owns executable authoring without becoming a second skill
source of truth.

## Install

```sh
npm install -g @lenso/cli
# or
cargo install lenso-cli
```

## Plan authoring

The same `lenso` executable validates authoring projects, resolves immutable
App Plans, and runs those Plans through the selected Driver and Execution
Adapters:

```sh
lenso check --project lenso.json \
  --execution-class lenso.native-rust@1
lenso resolve --project lenso.json \
  --execution-class lenso.native-rust@1 \
  --output .lenso/resolved-plan.json
lenso run --plan .lenso/resolved-plan.json --root .
```

Use `lenso add` to add a Module package to the authoring project. These
top-level commands are the only public authoring binary surface; the
`lenso-authoring` crate is a library.

## Module inner loop

Create either official execution shape as a self-contained authoring project:

```sh
lenso module create greeting --runtime rust
# or: lenso module create greeting --runtime bun
cd greeting
lenso module check
lenso module dev
lenso module verify
```

`module dev` infers the execution class from `lenso.json`; the older `--bun`
flag remains a compatibility override. Native Rust scaffolds include a
statically linked development Runner, while production Runner composition
remains App-owned. `module check --json` emits owner/path/fix diagnostics and
`module verify --json` records behavior probes plus a real removal-resolution
proof in `.lenso/module-verification.json`.

Use `--recipe stateless`, `stateful`, `web-console`, or `managed-work` to seed
the generated `MODULE.md` card. Preview Capability evolution and known affected
Instances with `lenso capability diff OLD NEW --project lenso.json`.

## Application lifecycle

```sh
lenso app compose ./support-desk \
  --blueprint support-desk \
  --implementation support-api=linked \
  --apply
cd support-desk
lenso system dev
```

`lenso app compose` writes the exact `lenso.app.json` App Composition and lock.
Its `--apply` flag atomically materializes that composition in the same command;
it is not a separate lifecycle stage and does not create a plan artifact. The
public application path is Compose, Run locally, Connect, and Status. Connect
and Status happen in Lenso Console through a signed enrollment exchange. The
CLI does not select an environment, deploy the application, release it, or
connect it to Console.

`lenso system check` remains a contract-validation command. `lenso host init`
and `lenso serve` remain lower-level host development tools, not alternative
application lifecycle stages.

The former App Plan, Apply, Verify, Diff, Repair, Next, Upgrade, and Explain
commands and the former System Init, AddService, AddModule, Plan, Diff, Apply,
Doctor, Release, Runbook, and Graph commands are no longer public entrypoints.
`lenso app compose` also no longer accepts `--write-plan`, `--explain`, or
`--addon`. Its surviving `--apply` option is only the atomic materialization
flag described above, not an Apply stage.

## Provision Lenso Console independently

Lenso Console is installed as an independent Service, not embedded into a
business Service. Provisioning Console is separate from connecting an
application and does not grant Console production deployment or release
authority. Obtain an official GitHub-attested Console Release Manifest,
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

For the complete local development path, start the generated Host, every
auto-start Service in `lenso.workspace.json`, and a connected Console with one
command:

```sh
cd ./my-lenso-host
lenso dev up
```

The first run securely prompts for the local Operator password. Automation can
pass `--operator-password-file` with an owner-only regular file. The command
creates loopback-only enrollment evidence, starts and migrates both Stores,
starts the pinned Console release, installs every workspace Provider export from its exact
Module Release, configures or reuses the durable Operator, reconciles
Module-owned UI artifacts, and connects the exact topology. Provider releases
and Service Installation state are written before the authoritative Host starts,
so `connected` and a callable Host business route refer to the same locked
export. Ctrl-C stops the Host, Console, and only the Services started by that
invocation. Story is a Console-owned linked surface; it does not require a
separate Module install.

By default, the CLI runs the released Console Service from an immutable,
multi-architecture OCI image and keeps its database and UI artifacts under
`.lenso/console-service`. Framework contributors can replace it with a source
checkout using `--console-root /path/to/lenso-console`. Use `--no-console` only
when intentionally running the Host and workspace Services without Console.

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

After upgrading the CLI, reconcile an existing Operator with the current
minimum scopes idempotently:

```sh
lenso console operator configure \
  --console-root ../lenso-console \
  --identifier admin@example.com
```

Both commands persist the user in the Console Access administrator store and
maintain the compatibility Auth scope configuration. The first administrator
is the durable Console superadmin; later configured users are administrators.

This preserves unrelated operators and explicit extra scopes while adding the
System read/connect, artifact reconciliation, Surface Gateway, Auth, and Story
capabilities required by the current Console workflow.

Apply prepared signed connection evidence through one public, idempotent CLI
entrypoint instead of a sequence of custom HTTP calls:

```sh
LENSO_CONSOLE_TOKEN='<operator-session-token>' \
  lenso console connect \
  --console-url http://127.0.0.1:3030 \
  --bundle .lenso/console-connect.json
```

The `lenso.console-connect.v1` bundle contains signed enrollment receipts, an
optional exact `console_composition` artifact effect, and the digest-bound
System Connection request. The command reuses existing enrollments, reconciles
artifacts, connects the System, and fails unless Console returns `connected`.
Use `--token-file` with a private regular file for non-interactive operation;
tokens and signing material are never printed or stored in the bundle.

The generated host depends on the crates.io `lenso` crate with the `host`
feature, which is the current narrow host API for booting API, worker, and
migration entrypoints. See
[`docs/architecture/framework-public-surface.md`](https://github.com/LioRael/lenso/blob/main/docs/architecture/framework-public-surface.md)
for the host-facade roadmap.

`lenso serve` is a local development wrapper for the generated host. It starts
the template Postgres service, runs migrations, then keeps the API and worker
running until Ctrl-C. New hosts run them in one local process; pass
`--separate-worker` when you want two child processes. Use `--skip-db` or
`--skip-migrate` when you already have those steps covered. `lenso dev up`
creates a missing private `.env`, choosing free loopback ports when the template
defaults are occupied; an existing `.env` remains authoritative.

## Scaffold a module

Create a complete Bun Module authoring project with a Capability Descriptor,
generated Rust and TypeScript bindings, a typed Provider, `lenso.json`, and a
locked workspace:

```sh
lenso module create greeting --runtime bun
cd greeting
lenso module dev --bun
```

The scaffold installs `@lenso/bun-module` and validates its TypeScript surface
before publishing the new directory. Use `--capability example.greeting@1` to
select the Capability identity, `--dir PATH` to select the project directory,
or `--no-install` for an offline scaffold. Existing targets are never replaced.

The original linked Rust scaffold remains available as the default inside a
framework workspace or starter host:

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
instead of layering generated state. The blueprint and implementation overrides
are composition inputs; `lenso.app.json` is the only application composition
and lock, with immutable Module release digests, implementation bindings,
resolved dependency selections, and an optimistic revision:

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

The adapter state under `.lenso/local-control-adapter/<appId>/state.json`
advertises its `adapterId`, exact `adapterWorkload`, loopback `endpoint`,
`workloadControlProtocol`, `workloadControlSchemaDigest`, and exact
`capabilities`. Without a server-side
`LENSO_WORKLOAD_CONTROL_TOKEN` override, startup creates an owner-only local
credential file and records only its `credentialFile` reference; the bearer
token is never serialized into adapter state or HTTP responses.

The Local Control Adapter advertises and accepts only `suspend` and `resume`.
Each accepted mutation returns an asynchronous Operation Record identified by
`operationId`; callers poll that handle for the Adapter's final result.

Until the Workload Control contract from
[LioRael/lenso#530](https://github.com/LioRael/lenso/pull/530) is published,
the CLI keeps a private, frozen mirror of its wire DTOs and constants. The
advertised schema digest and protocol conformance tests pin that bounded mirror
to the reviewed contract without adding an unpublished dependency to the CLI
package.

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

Generated TypeScript services expose the current Provider contract directly:

```sh
pnpm check
pnpm module:release > lenso.module-release.json
pnpm start
```

`pnpm start` serves the exact `lenso.provider.v1` descriptor, invocation,
recovery, and acknowledgement endpoints. The descriptor digests are the same
ones emitted in `lenso.module-release.json`.

Install that release into the Host runtime inputs with the Provider URL, not
the legacy Service discovery URL:

```sh
lenso module install ./lenso.module-release.json \
  --base-url http://127.0.0.1:4100/lenso/provider/v1 \
  --repo-root ../my-lenso-host
```

This writes `lenso.modules.json`, `lenso.modules.lock.json`, the Module Planning
Context, and the local Service Installation Set consumed by Host startup. It
does not write `SERVICE_MODULES` or treat an old install ledger as runtime
truth.

Before handing a legacy Service manifest to another app or deployment
pipeline, package-check the project and emit a compatibility package:

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
Compatibility packages remain available for older Service discovery consumers:

```sh
lenso service install dist/lenso-service/support-suite-provider/lenso.service-package.json \
  --base-url http://127.0.0.1:4100/lenso/service/v1
```

Install current Provider Modules from an exact release. The endpoint must end
in `/lenso/provider/v1`:

```sh
lenso module install ./lenso.module-release.json \
  --base-url http://127.0.0.1:4100/lenso/provider/v1
```

`lenso.module-release.v1` is the current Module release channel. It records a
fully qualified Module ID, canonical Manifest digest, exact delivery, governing
contract digests, and optional release-bound `console_ui_esm` artifact.

- Service delivery resolves to a locked Provider export and Service Installation.
- Linked delivery resolves to an immutable crate release and Host binding.
- A Console Surface exists only when the same exact release carries a
  `console_ui_esm` artifact and Console has reconciled its receipt.

`lenso service install` remains the lower-level provider/process command. It
connects a service, but it does not mean every module inside that service is
the user-facing install target.

The Service scaffold uses published `@lenso/service-kit` and `lenso-service`
dependencies by default. Framework contributors can opt into local checkout
dependencies explicitly with `lenso service create --local-framework-root PATH`.

### Module development

For a Bun Module project, resolve the entire authoring project and start the
production Bun Adapter wire with source watching:

```sh
lenso module dev --bun
```

Every relevant source or lockfile change stops the current child process,
rechecks generated contracts and package locks, resolves a fresh immutable App
Plan, and then starts again. Runtime output directories are excluded from the
watch set.

For Console development, run the released local Console Service independently:

```sh
lenso console dev
```

On first run, this command creates `.lenso/console-service/.env` with available
loopback ports, starts an isolated Postgres Compose project, applies migrations,
and serves the digest-pinned multi-architecture Console image. Existing local
state remains authoritative. Console contributors can instead pass
`--console-root ../lenso-console` to build and run a source checkout.

Run the Console UI artifact owned by the current Module:

```sh
lenso module dev --console-ui
```

The UI artifact remains bound to the same immutable Module Release. The CLI no
longer copies packages into a Console checkout or maintains extension registries.

## Install a module

```sh
lenso module install ./releases/auth/lenso.module-release.json
lenso module install ./releases/auth-password/lenso.module-release.json
```

Prefer an exact Module Release reference. Name-based catalog entries are accepted
only as a compatibility path and may describe a legacy linked install; they do
not prove that a current Console Surface artifact exists.

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

Exact Provider installs update the App Module lock and local Service
Installation Set in one step. Console UI artifacts remain immutable members of
their Module Release and are bound only by an applied Console composition.
Legacy linked descriptors still update host source and their compatibility
receipt, but that receipt is not an App Composition or a Surface grant.

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

The legacy doctor still reports compatibility Service discovery state. For a
current Provider, verify `lenso.modules.lock.json`, the local Service
Installation Set, and the live `/lenso/provider/v1` descriptor at Host startup.

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
