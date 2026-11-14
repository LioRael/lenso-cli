# Agent-native implementation status

First implementation batch, 2026-11-14. The CLI, SDK, Modules and Registry local
main branches matched origin/main after fetching. The Site repository was not
present alongside those checkouts; no Site changes were made.

## Baseline and delivered change

Existing static manifest discovery, project locks, offline policy, Wasm/process
adapters, JSON execution, questions and reviewed file plans were retained.
P0.1 now has a shared Framework catalog API, `help ... --json`, optional workflow
contracts, extension versions, and an embedded Acme example. No new business
commands were added to Core. Missing contracts/versions remain unknown.

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
cargo fmt --all --check
git diff --check
```

Workspace tests: 79 passed, one existing live GitHub credential/network test
ignored. Six added tests cover legacy argument shapes, shared native contracts,
secret grant exclusion, real CLI help, alias/project isolation, lock preservation,
offline discovery without source/module files, and invalid contract ownership.
Existing file approval/stale plan/process cancellation and Wasm budget tests pass.
The existing Check CLI workflow runs these tests; remote CI has not run for this
local change. No release or package publication was performed.

Reproduce the embedded CLI check from a clean project:

```sh
cargo build --locked -p lenso -p acme-example
demo=$(mktemp -d)
target/debug/lenso --project "$demo" help --json --offline
target/debug/acme --project "$demo" help hello --json --offline
target/debug/acme --project "$demo" hello --name Agent --json
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

P0.2 structured diagnostic codes, safe details and recovery, P0.3 opt-in compact
results and partial-failure/verification evidence, and their additional P0.4
regressions remain open. The existing errors and result envelopes are unchanged.
No JSON-size or Agent cost improvement is claimed by this batch.

P1 Ops lifecycle/readiness, optional Agent/MCP adaptation, SDK Action/Module
metadata, Files capability prechecks and Action integration, readable generated
exports, Registry discovery and Site updates remain open. Auth/Files consumer and
Code/shadcn acceptance scripts were not rerun for this discovery-only batch.
Windows/Linux, downstream Rust consumers and published-package compatibility
have not been validated for the new interface. New manifest contracts require a
compatible future Host release before publication; no supported release version
is asserted yet.
