# Agent-native implementation status

P0 protocols, P1 Ops and optional Agent/MCP source implementation, 2026-11-14–15. At the start, the CLI, SDK, Modules
and Registry local main branches matched origin/main after fetching. The Site repository was not
present alongside those checkouts; no Site changes were made.

## Baseline and delivered change

Existing static manifest discovery, project locks, offline policy, Wasm/process
adapters, JSON execution, questions and reviewed file plans were retained.
P0.1 now has a shared Framework catalog API, `help ... --json`, optional workflow
contracts, extension versions, and an embedded Acme example. No new business
commands were added to Core. Missing contracts/versions remain unknown.

P0.2 adds typed public diagnostics with stable codes, safe details, retry safety
and recovery instructions. Session failures preserve completed calls. File and
repository evidence changes reject stale plans; failed command applications
retain changed files, completed command indices and the actual child status.
GitHub publishing failures also retain release IDs and verified upload counts;
only loopback mock services were used in publication tests. Arbitrary legacy
error strings and guest exception text are excluded from terminal/JSON diagnostics.

P0.3 adds opt-in `--json --summary` and `--report NEW_FILE`. Default JSON retains
`outputs`; both forms include execution receipts and explicit validation-hook
outcomes. Plan/apply adapters provide actual affected paths and plan IDs.
Application health remains `not_reported`; a passed plan hook does not prove a
working application. Same-invocation full reports avoid replaying mutations just
to inspect an omitted payload. These are local report files, not persistent
sessions or cross-call resume tokens.

The derived CLI schema describes the parser's existing Text/Flag/List behavior.
Optional workflow schemas can declare required/default/enum constraints, but are
author metadata and do not add runtime validation. Descriptions exclude host
capability values and credential bindings. Authors must keep public metadata free
of secrets. `Extension` gains a public `contracts` field: serialized old manifests
remain accepted, but downstream Rust struct literals need to initialize that field.
`Argument` and `CommandSpec` struct literals remain unchanged.

## Validation

On macOS arm64 with Rust 1.99.0, standard toolchain Cargo passed:

```sh
cargo check --locked --workspace --all-targets --all-features
cargo test --locked --workspace --all-features
cargo test --locked -p lenso --no-default-features
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo doc --locked --workspace --all-features --no-deps
cargo fmt --all --check
git diff --check
```

Workspace tests with Ops and Agent: 100 passed, one existing live GitHub credential/network test
ignored. New regressions exercise secret-bearing legacy errors, parser/input
redaction even in debug mode, typed Wasm Host denials, guest error redaction,
actual Wasm deadlines/fuel exhaustion, failed/skipped validation, stale reads,
declined approvals, command partial failures, report write failure, overwrite
refusal, private Unix report permissions and same-invocation retrieval. Existing
alias/lock/permission/plan/process cancellation tests remain covered.

Real binary/Wasm acceptance commands:

```sh
python3 scripts/test-demo.py
python3 scripts/test-agent-protocol.py
python3 scripts/test-dependencies.py
python3 scripts/test-onboarding.py
python3 scripts/test-code-project-config.py
python3 scripts/test-code-initialization.py
python3 scripts/test-native.py
python3 scripts/test-shadcn-registry.py
python3 scripts/test-github.py
python3 scripts/test-publish.py
```

Code/shadcn fixture artifacts were rebuilt from current source. The native test
uses actual offline Cargo/npm packages and exercises failure/retry and preserved
user changes. Registry tests install/build single-project and monorepo npm Vite
apps against a local authenticated Registry. GitHub/publish tests use local mock
HTTP endpoints and fake credentials, not real GitHub mutation. The Check CLI
workflow includes the real M0 and compact-protocol checks. Linux CI and its
follow-up fixes are tracked in [PR #13](https://github.com/LioRael/lenso-cli/pull/13).
No release or package publication was performed.

The fixed `test-agent-protocol.py` task generates `Hello, Rust!`, validates the
file plan with a real Wasm hook, and applies it with explicit confirmations:

| Metric | Observed |
| --- | --- |
| Default JSON bytes | 2178 |
| Summary JSON bytes, including the report reference | 1243 |
| Saved full report bytes | 2293 |
| CLI calls per completed task in either mode | 1 |
| Extra execution needed to read the full result | 0 |

Sizes can vary with temporary path lengths. This compares current full versus
compact output for one fixture, not pre-change binary performance. Model token
counts, independent Agent completion rate, human interventions and execution-time
savings were not measured; no general cost reduction is claimed.

Reproduce the embedded CLI check from a clean project:

```sh
cargo build --locked -p lenso -p acme-example
demo=$(mktemp -d)
target/debug/lenso --project "$demo" help --json --offline
target/debug/acme --project "$demo" help hello --json --offline
target/debug/acme --project "$demo" hello --name Agent --json --summary --report "$demo/execution.json"
target/debug/acme --project "$demo" hello --json
```

Observed terminal record:

| Invocation | Exit | Output |
| --- | --- | --- |
| `lenso help --json --offline` | 0 | `command_catalog`, 15 management aliases, no Ops |
| `acme help hello --json --offline` | 0 | `command_catalog`, one alias, Acme version and declared empty effects |
| `acme hello --name Agent --json` | 0 | `result`, `complete` |
| `acme hello --json` | 2 | `result`, `needs_input` |

Neither a project lock nor a dependency cache was created in that clean project.
The Acme binary embeds the Framework; it does not invoke the Lenso binary.

## P1 Ops delivery

The source-only `lenso-ops` crate supplies native `ops-host` handlers. The
`ops` binary feature is disabled by default, and only the separately installed
Ops manifest adds aliases. `inspect` checks the declared workflow's exact process
grant, executable/cwd, environment names, HTTP grant/config and port availability.
`run` supervises a foreground Unix process group, checks actual HTTP or exit-code
readiness, and handles lifetime/readiness deadlines and SIGINT/SIGTERM cleanup.
`verify` reads the configured health endpoint and explicitly leaves managed-process
identity unknown. Full declarations and scope: [Ops README](../extensions/ops/README.md).

On macOS arm64, Node 26.10.0/Rust 1.99.0, `scripts/test-ops.py` passed 20 recorded
Node/Rust lifecycle cases plus preflight/permission/discovery checks:

| Both fixture languages | Observed |
| --- | --- |
| Declared exit 0 / unexpected exit 7 | Actual exit verification / `process_failed`, exit 7 |
| Zero exit before HTTP readiness | `verification_failed`, no invented readiness |
| HTTP 503 / 302 / 404 / unavailable listener | Actual not-ready status/reason; redirects not followed |
| Healthy foreground process past its lifetime | Observed readiness then `process_timeout` |
| SIGINT / SIGTERM while healthy | `cancelled`, direct child reaped and streams drained |
| Independent verify during/after the run | Actual HTTP ready/not-ready evidence; process identity remains unknown |

Recorded child PIDs, including the Node descendant, disappeared; ports could be
rebound with ordinary server `SO_REUSEADDR` semantics. No existing source files
changed. Separate bounded stdout/stderr contained masked injected values; ambient
environment was absent. Exact-argv denial, literal shell metacharacters, missing
program/cwd/env, occupied port and invalid/unauthorized endpoints were checked.
`--yes` and input-supplied capability objects did not create grants. Static help
created no execution lock or process; locked/offline discovery kept its pins.
A separate binary built without Ops rejected the native dependency with a typed
`dependency_unavailable` diagnosis. The variants were copied before testing to
avoid feature builds replacing a test's executable midway through acceptance.

Four Ops unit tests cover config guards, numeric-loopback rules, bounded secret
redaction and graceful-exit reaping before group escalation. The dependency-free
Rust fixture also passed native offline check/test/clippy/build. The Linux Ops job
passed the same 20 lifecycle cases and additional checks with both binary variants
and both fixtures. Reports, platform versions, job evidence and Linux fixes are in
the [cross-platform record](ops-acceptance.md). Windows process management is unsupported in this slice.
No Ops package/preset release or Registry publication was performed.

## P1 optional Agent/MCP delivery

The separate source-only `lenso-agent` library/binary selects exact aliases from
the existing catalog and calls their native JSON entrypoints. MCP is a disabled-
by-default feature using the official Rust SDK and local stdio protocol 2025-11-25.
Core has no Agent/MCP dependency or command. The adapter retains native questions,
approval/grant semantics, diagnostics, exit codes and summaries, and serves bounded
same-invocation report resources. Calls do not retain Sessions across invocations.

Acme now defines a read-only status command and a restricted note workflow through
the existing Project plan/apply provider. Actual Shell/JSON/MCP, authority, stale
approval, schema, report and Node/Rust lifecycle checks passed in 22 local scenario
groups. Four native regressions cover process/output/storage boundaries. Scope,
reproduction, evidence costs and unrun remote CI are recorded in
[Agent acceptance](agent-adapter-acceptance.md). No adapter release was performed.

## Remaining implementation

P0.2/P0.3 and their related regressions are implemented in source. They retain
execution exit codes and existing result fields, but error message matching must
migrate to codes/details: legacy arbitrary messages are intentionally no longer
public. Native authors should use `Diagnostic`; old guest failures without a typed
Host cause conservatively report `wasm_failed`. Other authored outputs, questions
and child logs remain public content and are not automatically scrubbed.

SDK Action/Module metadata, Files capability prechecks and Action integration, readable generated
exports, Registry discovery and Site updates remain open. Auth/Files application
consumer acceptance was not rerun; no SDK/Modules sources were changed.
Windows, Linux arm64, downstream Rust consumers and published-package compatibility
have not been validated for the new interface. New manifest contracts require a
compatible future Host release before publication; no supported release version
is asserted yet.
