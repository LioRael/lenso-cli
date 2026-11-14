# Ops cross-platform acceptance

Recorded on 2026-11-15 using the real Node and dependency-free Rust fixtures in
`fixtures/ops`. Both platforms ran `scripts/test-ops.py` against separate native
binaries built with Ops and without default features. No published packages or
production services were used.

| Platform | Node | Rust/Cargo | Node cases | Rust cases | Evidence |
| --- | --- | --- | --- | --- | --- |
| macOS, Darwin 27.2.0, arm64 | 26.10.0 | 1.99.0 | 10 passed | 10 passed | [Local report](ops-acceptance/macos-arm64.json) |
| Ubuntu 24.04 runner, Linux x86_64 | 26.13.1 | 1.99.0 | 10 passed | 10 passed | [CI report](ops-acceptance/linux-x86_64.json), [Ops job](https://github.com/LioRael/lenso-cli/actions/runs/43620323195/job/132524354389) |

The macOS report was generated from `c10b3b6`; the Linux report came from
`81e1dca`. Ops implementation and fixture sources were identical at both commits.
The Linux run's Ops job passed; its separate CLI job exposed the installation
test race described below. These reports establish the Ops job's scope rather
than claiming that every job in that historical run passed.

## Checks performed on each platform

Each fixture passed expected exit 0, unexpected exit 7, exit before HTTP
readiness, HTTP 503, redirect 302, unavailable listener, runtime deadline,
wrong health path 404, SIGINT and SIGTERM. The signal cases verified HTTP 200
independently before cancellation and verified that the endpoint was unavailable
after cleanup. Independent verification keeps process identity explicitly unknown.

All runs asserted direct-child reaping, absence of recorded descendant PIDs
(the Node fixture spawns a child), reusable ports, separate bounded/redacted
stdout and stderr, and unchanged source files. Preflight and discovery also
checked exact argv grants, literal shell metacharacters, missing executable/cwd/env,
occupied ports, rejected endpoints, omitted ambient environment, static help,
locked/offline discovery, missing Ops support and the inability of `--yes` or
input capability objects to create grants. The Rust fixture passed native
offline Cargo check/test/clippy/build on both systems.

## Linux failures and fixes

- The first Ops run reached the Rust fixture but failed because the minimal
  toolchain lacked Clippy. The Ops job now installs that component explicitly;
  the linked passing job includes the fix.
- The full CLI suite exposed Linux `ETXTBSY` while starting a freshly copied
  installation-test executable. Concurrent process creation can briefly inherit
  an open writable copy descriptor. Installation tests now hard-link the immutable
  test binary inside a temporary directory on its own filesystem, avoiding a
  writable copy and preserving the tested executable path. No retry or skipped
  assertion was added. The 13 command integration tests passed locally after this
  change; the [PR checks](https://github.com/LioRael/lenso-cli/pull/13/checks) track
  the complete Linux rerun.
- The M0 Wasm build script invoked bare `cargo`, which selected the runner's
  default toolchain instead of the explicitly prepared 1.99.0 toolchain and its
  Wasm target. The native check job now sets `RUSTUP_TOOLCHAIN=1.99.0` for its
  Python subprocesses as well as its explicit Cargo commands.

## Reproduction and retained evidence

Follow the [Ops acceptance commands](../extensions/ops/README.md#real-acceptance).
Set `LENSO_OPS_REPORT=/absolute/path/report.json` to retain the successful report
with actual OS, architecture and Node/Cargo versions. The script exits unsuccessfully
on any failed assertion and writes a passed report only after all checks finish.
CI uploads `ops-acceptance.json` and `ops-acceptance.log` as the
`ops-acceptance-linux` artifact; the log is retained on failures too.

This validates Unix foreground supervision on the two tested architectures.
Windows process management, Linux arm64, daemon adoption and cross-session resume
were not tested and are outside this slice. No package release or Registry
publication was performed.
