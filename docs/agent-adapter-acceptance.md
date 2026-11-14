# Agent/MCP acceptance record

Source implementation, 2026-11-15. The optional `crates/agent` adapter derives tools
from the existing catalog and invokes the existing CLI JSON interface. No Core
command, workflow model, SDK or Module implementation was replaced.

## Actual cross-platform acceptance

On macOS arm64, `scripts/test-agent-adapter.py` passed 22 recorded scenario groups
against the real Acme, Agent and Ops-enabled Lenso binaries. All projects started
as isolated temporary directories. The Acme tests ran with empty PATH, verifying
that its embedded Framework works without a standalone Lenso process.
The [recorded JSON report](agent-acceptance/macos-arm64.json) includes every group
and the actual binary/toolchain versions.

The same source at `8df0bd5528d3781913ba622be721a3066e46dd20` passed both remote
Agent jobs in [Check CLI run 43627439136](https://github.com/LioRael/lenso-cli/actions/runs/43627439136).
Both downloaded CI reports contain the same 22 ordered scenario groups as the
local report. No assertion was removed or relaxed, and no implementation fix was
needed for this cross-platform run.

| Environment | Architecture | Node | Cargo | Groups | Retained evidence |
| --- | --- | --- | --- | --- | --- |
| Local macOS | arm64 | 26.10.0 | 1.99.0 | 22 passed | [Local JSON](agent-acceptance/macos-arm64.json) |
| Ubuntu 24.04 CI | x86_64 | 26.13.1 | 1.99.0 | 22 passed | [CI JSON](agent-acceptance/linux-ci-x86_64.json), [job log](https://github.com/LioRael/lenso-cli/actions/runs/43627439136/job/132546382056) |
| macOS 15 CI | arm64 | 26.13.1 | 1.99.0 | 22 passed | [CI JSON](agent-acceptance/macos-ci-arm64.json), [job log](https://github.com/LioRael/lenso-cli/actions/runs/43627439136/job/132546382280) |

| Boundary | Observed |
| --- | --- |
| Discovery and identity | Only exact selected aliases; original catalog records/schema preserved; no note/lock created by help; fixed Host program/project |
| Shell / JSON / Agent / MCP | Same Acme status output; shared command definitions; default Agent binary has no MCP entry |
| Validation | Wrong types, required/unknown fields and fake grants rejected without echoing input; HTTP/file schema references denied; no loopback schema request occurred |
| Effects | Host file grant required; preview writes nothing; false confirmation cancels; true digest-bound confirmation changes only `acme-note.txt` |
| Stale authority | Changed file preimage yields a new question; revoked Host grant blocks an existing MCP connection; changed catalog record blocks execution |
| Diagnostics | Native discovery and process failure codes retained; rejected secret-bearing parse input excluded from responses and stderr |
| Evidence | Summary and full resource have the same execution ID; resource reads do not repeat changes; arbitrary file URIs rejected; oldest reports evicted at retention limit |
| Node and Rust | Real HTTP 200; exit 7 retained; request cancellation, EOF, adapter deadline and SIGTERM leave no recorded process/listener; actual cleanup evidence retained |

Four native Rust regressions cover actual process status/stderr separation,
bounded stdout without pipe deadlock, escalation and direct-child reaping for an
interrupt-ignoring process, and report-storage failure after completed execution.
Standard toolchain `cargo check --locked --workspace --all-targets --all-features`,
workspace all-feature tests (100 passed, one existing live credential/network test
ignored), no-default-feature CLI tests (29 passed), Clippy with warnings denied,
rustdoc, rustfmt and diff checks passed. Existing real Wasm M0, compact-protocol
and onboarding acceptance passed. Release-helper tests and both modified workflow
YAML files also passed their checks. Each remote Agent job additionally passed
all four native adapter regressions and the Core dependency-isolation check.
The JSON acceptance report and clean-project terminal record are emitted with
`LENSO_AGENT_REPORT` and the reproducible commands in the
[adapter README](../crates/agent/README.md#acceptance).

The Check CLI workflow adds Linux/macOS Agent jobs, each building both feature
variants, running the native regressions and real Acme/Node/Rust acceptance, and
uploading `agent-acceptance-{Linux,macOS}` JSON/log artifacts. The jobs above passed
with both artifacts retained. The committed JSON copies remain available after
CI artifact expiration. [PR #14](https://github.com/LioRael/lenso-cli/pull/14/checks)
tracks the complete CLI/Agent/Ops/extension checks on subsequent commits.
No package publication or deployment was performed.

## Cost and scope of the evidence

Command authors supply one `CommandSpec`/workflow/contract. Acme's new status and
note commands contain no MCP handlers or transport dependency. The separate
adapter supplies selection, validation, process supervision and report resources;
its `mcp` feature adds the official SDK transport. Core's normal Cargo dependency
graph contains no Agent, MCP or JSON Schema adapter dependency.

The process design adds one metadata invocation before each execution, plus one
at adapter startup. Reading a retained report uses one resource request and zero
additional command executions. Supplying an answer is a fresh native invocation,
so this is not cross-call Session continuation. These are observable interface
costs; no hand-written CLI+MCP baseline was built. Code-size savings, model token
cost, independent Agent completion rates, latency gains and human intervention
rates were not measured, and no comparative performance gain is claimed.

Only Unix foreground/local stdio use is implemented. No Windows, Linux arm64,
remote authentication, GUI MCP client integration, arbitrary third-party CLI
protocols, persistent sessions or published-package compatibility was validated.
The package remains source-only and requires a compatible future Framework release
before publication. The release graph orders it after Framework; no package
versions or global toolchain configuration were changed for this work.
