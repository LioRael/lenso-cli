# Optional Agent and MCP adapter

Unreleased source implementation. `lenso-agent` is a separate native executable
and library. The `lenso` binary and Framework do not depend on it or on MCP.
MCP support is an opt-in Cargo feature; a default Agent build provides JSON tool
descriptions and calls without an MCP command.

The adapter runs an explicitly configured trusted native CLI. It obtains the
existing `help --json` catalog, derives tools from selected aliases, and calls
the same CLI with `--json`, `--input` and `--answers`. It needs no standalone
`lenso` process when adapting a third-party executable such as Acme. It does not
introduce a second command/workflow definition or execute a shell.

## Build and use from source

```sh
cargo build --locked -p acme-example -p lenso-agent
demo=$(mktemp -d)
target/debug/lenso-agent --program "$PWD/target/debug/acme" \
  --project "$demo" --offline --allow status --allow note tools
target/debug/lenso-agent --program "$PWD/target/debug/acme" \
  --project "$demo" --offline --allow status call --name status
```

`tools` emits `agent_tools`, schema version 1. Every tool contains its original
catalog record under `_meta["lenso/command"]`, including provider version,
capabilities, effects, questions, verification and any unknown values. The input
wrapper contains `input`, `answers` and `mode`. `input` uses the author's workflow
input schema when present, otherwise the derived CLI argument schema. Both paths
use a JSON Schema validator; no HTTP/file schema resolver is enabled. Defaults
inside authored schemas remain annotations and are not silently inserted into
workflow input. Rejected values and validator error text are not returned.

Each `--allow` selects one exact alias, such as `--allow 'ops run'`. Empty selects
nothing. Prefixes, wildcards, workflow IDs and management commands not selected
by the host cannot be called. Paths become unambiguous tool names joined with
`__`; names longer than 128 bytes and duplicate selections are rejected.
The adapter rechecks the selected catalog record before every execution. A
changed/removed record requires reconnecting and reviewing the new description.
This checks metadata, not the executable's cryptographic identity.

## Acme's restricted side effect

The [Acme example](../../examples/acme-cli/src/main.rs) defines `status`, `note`
and `hello` once for Shell, JSON and tool use. Its native Host permits `note` to
write only `acme-note.txt` when the project's `acme-policy.json` explicitly
contains `{"write_note":true}`. The default denies writing. The existing Project
provider creates the plan and digest-bound confirmation; a selected tool alone
has no file authority.

```sh
printf '{"write_note":true}\n' > "$demo/acme-policy.json"
target/debug/lenso-agent --program "$PWD/target/debug/acme" \
  --project "$demo" --offline --allow note call --name note \
  --arguments '{"input":{"text":"Reviewed note"}}'
```

This exits 2 with the existing `needs_input` question and writes no note. Review
the preview, then supply that exact question ID with a boolean answer in
`arguments.answers`, keeping the original input. `false` cancels. A changed file
preimage produces a new question ID, so an old approval does not apply. Host
policy revocation applies even to an existing MCP connection. There is no
adapter `--yes`, automatic consent, caller-selected project, or caller-supplied
capability map.

## Results and report resources

Calls return `{exit_code, result, report_complete}`. `result` retains the native
CLI's result/question/diagnostic, execution receipt and actual exit status. A
missing answer remains exit 2; cancellation remains 130; a process exit 7 remains
7. Author output schemas describe handler output and stay in the original
metadata. The tool's `outputSchema` describes the invocation envelope instead.
An empty declared effects list produces a read-only annotation; unknown effects
stay unknown. Annotations are author hints, never grants.

MCP calls default to `mode: "summary"`; `mode: "full"` requests step outputs.
`result.report` becomes an opaque `lenso-report://...` resource for the same
invocation's full report. `resources/read` does not rerun the operation and cannot
read arbitrary files. Reports last for this server process only: at most 16
reports and 16 MiB of serialized report data, evicting oldest first. A missing, oversized or invalid
target report sets `report_complete: false` and retains only the public result,
also marked incomplete in the resource. A failed report capture is not silently
presented as full evidence.

One-shot `call` defaults to full mode. Summary mode requires `--report NEW_FILE`,
because resource URIs expire when that process exits. Report files use exclusive
creation and Unix mode 0600. Existing destinations are rejected before execution.
Storage failure after execution retains the observed outcome, marks possible
completed effects and disallows automatic retries.

## MCP stdio

```sh
cargo build --locked -p lenso-agent --features mcp
target/debug/lenso-agent --program "$PWD/target/debug/acme" \
  --project "$demo" --offline --allow status --allow note mcp
```

Configure a client to launch that source-built executable and those same
arguments. The optional transport uses the official Rust SDK `rmcp` and pins
protocol negotiation to **2025-11-25**. It implements initialization, ping,
`tools/list`, `tools/call`, `resources/list`, `resources/read` and request
cancellation. It emits SDK `structuredContent` plus the matching JSON text
content. A `needs_input` question is a successful tool result requiring an answer;
execution/validation failures use `isError: true`. Unknown tools/resource URIs
and malformed requests are protocol errors. It does not advertise elicitation,
tasks, sampling, prompts, subscriptions or tool-list changes.

References: [MCP tools](https://modelcontextprotocol.io/specification/2025-11-25/server/tools),
[stdio transport](https://modelcontextprotocol.io/specification/2025-11-25/basic/transports),
[lifecycle](https://modelcontextprotocol.io/specification/2025-11-25/basic/lifecycle).

The identity/authority boundary is the local user launching an explicit canonical
executable and project. Client identity metadata, roots, input and cancellation
reason text do not change either. There is no HTTP listener or remote identity
provider. Native CLI code remains trusted; this is not a native-code sandbox.
The child CLI inherits the invoking Host's environment so its existing explicit
environment grants keep working. Adapter stderr is drained and discarded; raw
target errors, stderr and invalid stdout are not inserted into public diagnostics.
Authored outputs/questions and static descriptions remain public author content.

## Supervision and limits

The first slice supports Unix process supervision. Up to four invocations may
run concurrently, allowing health verification during a foreground run. Each
call starts one read-only catalog check and one execution process. MCP startup
has a 10-second initialization deadline. Each target process has a 30-second
default deadline, configurable by the local Host with `--timeout-ms`, up to 24
hours. Arguments are limited to 1 MiB; target stdout and full report capture are
limited to 4 MiB each, leaving room for MCP's text/structured JSON duplication.
The SDK's stdio input line limit is 16 MiB.

Request cancellation, EOF, SIGINT or SIGTERM forwards SIGINT to the native CLI,
allowing its existing supervisors to clean up and report actual evidence. After
two seconds, an unresponsive runner's owned process group is killed and the
direct child reaped. Output drains have one-second budgets. An adapter timeout
stays `process_timeout` even if the CLI reports cooperative cancellation; the
report retains both dispositions. Forced runner termination cannot prove cleanup
of application processes in independent groups; cleanup remains `not_reported`
without target evidence. No command is automatically retried.

Every call is a new native CLI invocation, including calls that supply an answer.
Completed steps from a previous invocation may run again. Authors must make the
pre-question path safe to repeat; Acme uses pure source/plan steps before approval.
Report retrieval is same-invocation evidence, not a persistent Session, resume
token, idempotency key or exactly-once guarantee. Windows supervision, remote
authentication, background adoption and cross-session recovery remain outside
this version. Neither the adapter nor its new interfaces have been published.

## Acceptance

```sh
cargo build --locked -p lenso-agent --no-default-features
cp target/debug/lenso-agent target/debug/lenso-agent-no-mcp
cargo build --locked -p lenso-agent -p acme-example -p lenso \
  --features lenso-agent/mcp,lenso/ops
LENSO_AGENT_NO_MCP_BIN="$PWD/target/debug/lenso-agent-no-mcp" \
LENSO_AGENT_REPORT=agent-acceptance.json python3 scripts/test-agent-adapter.py
```

The script uses clean temporary projects and actual native binaries. It tests
Acme without PATH or a standalone Lenso dependency, authorization/approval/schema
failures, contract drift, external schema denial, private same-invocation reports,
and real Node/Rust Ops readiness, nonzero exit, cancellation, timeout, EOF and
SIGTERM cleanup. See the [acceptance record](../../docs/agent-adapter-acceptance.md).
