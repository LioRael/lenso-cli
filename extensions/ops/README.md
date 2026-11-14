# Optional Ops: foreground processes and readiness

Unreleased source implementation, 2026-11-15. Ops is a separately installed
extension, backed by the optional `lenso-ops` native provider. No Ops commands are
added to CLI Core or projects that have not installed its manifest. The first
slice supports Unix foreground supervision on macOS/Linux; Windows process-tree
management and cross-session resume are outside this version.

Build a Host with the optional provider:

```sh
cargo build --locked -p lenso --features ops
```

An embedding Host can put `lenso_ops::register` and `lenso_ops::VERSION` in its
`LoadOptions.providers` map under `ops-host`. Merely making the provider available
does not install commands. The source [manifest](../../presets/ops/extension.toml)
declares the native dependency, workflows, contracts and aliases. This package
and manifest have not been published to the Registry or crates.io.
The publication graph lists this provider before the CLI; compatible P0 Framework
and Runtime releases must precede it. No release versions were bumped here.

## Declaration and authority

Copy that manifest to an existing project's `ops-extension.toml`, then explicitly
declare its grants in the project's `lenso.toml`. This example uses the real
[Node fixture](../../fixtures/ops/node/server.mjs) copied into the project:

```toml
[cli]
schema_version = 1

[[cli.extensions]]
id = "ops"
source = "ops-extension.toml"
reads = ["lenso.ops.toml"]
processes = [{ program = "node", args = ["server.mjs"] }]
env = ["OPS_PORT", "OPS_MODE", "OPS_PID_FILE", "OPS_SECRET"]
http = ["http://127.0.0.1:43187/health/"]
```

Declare the named operation in `lenso.ops.toml`:

```toml
schema_version = 1

[workflows.dev]
program = "node"
args = ["server.mjs"]
cwd = "."
env = ["OPS_PORT", "OPS_MODE", "OPS_PID_FILE", "OPS_SECRET"]
timeout_ms = 30000
readiness_timeout_ms = 5000
readiness = { type = "http", url = "http://127.0.0.1:43187/health/ready", status = 200 }

[workflows.check]
program = "node"
args = ["server.mjs"]
cwd = "."
env = ["OPS_MODE", "OPS_SECRET"]
timeout_ms = 10000
readiness_timeout_ms = 5000
readiness = { type = "exit", code = 0 }
```

Environment declarations contain names only; values come from the invoking Host.
For the development fixture, set `OPS_PORT=43187`, `OPS_MODE=ready`,
`OPS_PID_FILE=pids.json`, and a nonempty `OPS_SECRET` fixture value. For the check
fixture, set `OPS_MODE=exit0`. Never put credentials in static argv, URLs or config.
Native program/argv grants permit that program's real effects; they are not a
filesystem or network sandbox. The fixture writes a PID record for the acceptance
test; Ops itself does not persist PID files or adopt processes from them.

## Commands

```sh
lenso help ops run --json
lenso ops inspect dev --json
lenso ops run dev --json --summary --report run.json
lenso ops verify dev --json
```

Use the source-built executable above in place of `lenso`. Run `verify` from
another terminal while `run` is active. `run` remains in the foreground until
the child exits, a deadline expires, or the supervisor receives SIGINT/SIGTERM.
There is no implicit detached daemon. Missing workflow names yield a selection;
`--yes` cannot select one or add grants.

`help ... --json` reads only extension metadata. It does not load the Ops config,
look up executables, read environment values, check a port, or start a process.
It retains the existing read-only, locked/offline discovery policy.

`inspect` reads the granted config and reports definitions, conditional required
capabilities and launch issues without executing a program. It checks:

- Executable availability and a project-relative, existing working directory;
  traversal and symlink directories are rejected.
- Exact program/argv membership in `host.processes`. Input objects, answers and
  `--yes` cannot change that grant or substitute another command.
- Environment-name membership in `host.env` and the availability of nonempty
  UTF-8 values. Only PATH and the explicitly granted names are inherited.
- Plain numeric-loopback HTTP, a valid 2xx expected status, an explicit
  `host.http` prefix grant, and a currently bindable port for HTTP launch.

The port check briefly binds/closes a listener. It is a preflight observation,
not a reservation or proof of ownership against racing processes. `run` repeats
preflight immediately before spawning and never invokes an implicit shell.
CLI argument schemas remain derived from the single command definition; actual
per-workflow prerequisites are available through `inspect`.

`verify` performs only the configured HTTP GET. It disables proxies and redirects,
uses a 250 ms request timeout, and records observed status/reason/time without
response bodies or credential values. `state: ready` requires the declared HTTP
status. Failure returns `verification_failed` with actual `not_ready` evidence.
Its result explicitly says `process: unknown` and
`scope: configured_endpoint_only`: an independent invocation does not prove that
the endpoint belongs to a particular managed child. Exit-code readiness is checked
against the actual exit in `run`; `verify` cannot reconstruct that past observation.

## Supervision and evidence

Each launch gets a new Unix process group. SIGINT/SIGTERM and both deadlines lead
through cleanup: request group SIGTERM, allow 100 ms for graceful exit, reap an
exited leader, escalate the remaining group to SIGKILL, then await the direct
child and drain its streams. Normal exit also cleans remaining group members.
Drop guards kill the owned group and abort stream readers if the future is dropped.
Tools that deliberately escape their group or daemonize are outside this scope.
The Host does not attach to arbitrary or previously recorded PIDs.

The direct child wait and stream drain each have a one-second cleanup budget.
Evidence records whether group signalling, direct-child reaping and stream drain
actually succeeded; errno is retained on failed group signalling. A completed
result requires successful cleanup. Orphan descendants are reaped by the OS;
group signalling alone is not reported as proof that every descendant was reaped.

Readiness polling stops once HTTP readiness is observed. `ready_observed` is a
timestamped historical observation, not continuing health monitoring. Cancellation
can happen before the runner's first successful probe even if another client has
already seen the endpoint respond; the missing observation remains null.
Current health is independently queried with `verify`. A zero exit before HTTP
readiness does not become success.

Output contains separate stdout/stderr records with at most 4096 UTF-8 bytes of
text per stream, original byte counts and truncation flags. Readers keep draining
after the capture budget so a verbose child does not block on full pipes. Exact
values of explicitly inherited variables are masked, including across chunk and
budget boundaries. Other child output remains authored content, not a general
secret-detection guarantee. Log values are not forwarded into the CLI's stdout
protocol. Failures retain these bounded records, real exit/signal information,
readiness evidence, and cleanup results in structured diagnostics.

`process_failed` preserves the child's nonzero exit code; cancellation uses 130;
`process_timeout` and `verification_failed` use 1. Handler success, HTTP status and
application correctness remain distinct. Core receipts continue to report
`application_verification: not_reported`; the Ops output supplies the specific
health/exit evidence. Summary/report behavior is the existing P0 protocol.

Config is bounded to 1 MiB/64 workflows, 128 argv entries, 32 environment names,
16 KiB per inherited value, and lifetime deadlines of 10 ms to 24 hours. Readiness timeout
must be positive and no greater than the lifetime deadline. Child memory/CPU and
its own application effects are not sandboxed by these limits.

## Real acceptance

```sh
cargo build --locked -p lenso --features ops
cp target/debug/lenso target/debug/lenso-with-ops
cargo build --locked -p lenso --no-default-features
cp target/debug/lenso target/debug/lenso-core
LENSO_BIN="$PWD/target/debug/lenso-with-ops" \
LENSO_CORE_BIN="$PWD/target/debug/lenso-core" python3 scripts/test-ops.py
```

The script uses isolated Node and dependency-free Rust projects, resolves the
installed Node executable, builds/checks/tests/lints the Rust fixture with native
Cargo offline, and runs actual binaries. Both languages cover readiness, 503,
redirects, a wrong health path, connection failure, premature exit, nonzero exit,
runtime timeout, SIGINT and SIGTERM. It checks exact grants, literal argv, omitted
ambient environment, missing prerequisites, static/locked/offline discovery,
feature absence, distinct redacted streams, unchanged source files, PID absence
and reusable ports. The Node service also creates a descendant process.

The CI Ops job runs this script on Linux with Node 26/Rust 1.99. Local validation
and current limits are recorded in [implementation status](../../docs/agent-native-status.md).
