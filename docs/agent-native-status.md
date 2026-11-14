# Agent-native implementation status

P0.1–P0.3 source implementation, 2026-11-14–15. At the start, the CLI, SDK, Modules
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
cargo check --workspace --all-targets
cargo test --locked --workspace
cargo test --locked -p lenso --no-default-features
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo doc --locked --workspace --no-deps
cargo fmt --all --check
git diff --check
```

Workspace tests: 92 passed, one existing live GitHub credential/network test
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
workflow now includes the real M0 and compact-protocol checks; remote CI has not
run for this local change. No release or package publication was performed.

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

## Remaining implementation

P0.2/P0.3 and their related regressions are implemented in source. They retain
execution exit codes and existing result fields, but error message matching must
migrate to codes/details: legacy arbitrary messages are intentionally no longer
public. Native authors should use `Diagnostic`; old guest failures without a typed
Host cause conservatively report `wasm_failed`. Other authored outputs, questions
and child logs remain public content and are not automatically scrubbed.

P1 Ops lifecycle/readiness, optional Agent/MCP adaptation, SDK Action/Module
metadata, Files capability prechecks and Action integration, readable generated
exports, Registry discovery and Site updates remain open. Auth/Files application
consumer acceptance was not rerun; no SDK/Modules sources were changed.
Windows/Linux, downstream Rust consumers and published-package compatibility
have not been validated for the new interface. New manifest contracts require a
compatible future Host release before publication; no supported release version
is asserted yet.
