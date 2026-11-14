# Automatic dependency preparation and JSON ABI v1

## Static command discovery (source development)

The source checkout supports `lenso help --json` to list registered aliases and
`lenso help app run --json` to describe an installed command or group. The same
interface works in embedded CLIs, for example `acme help hello --json`. No handler
is called. Human `help` and `--help` retain terminal help; `--help --json` still
prints Clap help text. Only the explicit `help ... --json` form returns a catalog.
The generic `run` and `help` frontend controls are not workflow aliases and are
not catalog entries. An unknown or uninstalled path fails rather than returning
an empty successful selection.

Catalogs use `type: command_catalog`, `schema_version: 1`, and a `commands` array
sorted by command path. Each entry contains the original `CommandSpec` fields,
`provider_version`, `cli_input_schema`, and `contract`. The CLI schema is derived
from existing Text/Flag/List arguments: string, boolean, and an array of strings
with `minItems: 1`. All CLI options remain optional; absence is passed through to
the workflow rather than inventing a default or required input. Workflow-level
required/default/enum constraints belong in the optional declared input schema.
Additional `--input` keys remain accepted, so the derived CLI schema does not
set `additionalProperties: false`.

`Cli::describe(path)` exposes the same catalog without a terminal or an installed
`lenso` executable. `Cli::contract(workflow, contract)` attaches an optional
`CommandContract` to a registered workflow; aliases share that declaration.
`Cli::provider_version(provider, version)` records a native provider's version.
Old `Argument` and `CommandSpec` Rust literals and serialized manifests remain
valid without adding fields.

Extensions may declare a `contracts` map keyed by an owned workflow ID:

```toml
[contracts."example/check"]
input_schema = { type = "object", properties = { mode = { type = "string", enum = ["fast", "full"] } } }
capabilities = ["host.reads"]
effects = []
verification = ["Check the returned state against the project's expected state"]
```

Other optional fields are `output_schema` and `questions` (the existing Question
shape). Omitted contract fields serialize as `null`, meaning unknown; `[]`
explicitly declares none. A wholly absent contract is also `null`. Provider
versions come from resolved extension metadata, or remain `null` for native
commands whose host has not declared a version. Contracts referencing a workflow
outside the extension's definitions are rejected before publishing a lock or
changing the host registry.

Contracts are public author declarations, not sandbox proofs, schema validators,
permission grants, or evidence of verification. Do not include secrets, secret
defaults, environment values, credential URLs, or private diagnostic data in them.
Discovery does not serialize host grants, extension options, answers, or credential
bindings. Execution and its independent authorization checks are unchanged.

Machine help uses the existing read-only discovery load mode: it respects aliases,
disabled commands, `--project`, `--offline`, and `--locked`, and does not write a
project lock or hydrate/execute Wasm. First discovery may resolve source metadata
under the existing network policy. Warm locked discovery can run without the
original module or manifest files. This additive manifest field is implemented
in the source checkout only; older releases reject unknown fields. Publish a
compatible Host release and declare its minimum version before distributing a
manifest using `contracts`.

## Dependency preparation

`lenso` reads `<project>/lenso.toml` for explicit sources and project policy, and
`<project>/lenso.lock` for pinned command/workflow/handler metadata. An explicit
`--project` is exact; otherwise the nearest ancestor with `lenso.toml` is selected.
There is no package scanning, executable discovery, or global extension scope.

```toml
[cli]
schema_version = 1

[[cli.extensions]]
id = "example"
source = "lenso/extensions/example/extension.toml"
files = ["greeting.txt"]
reads = ["input.txt"]
http = ["https://api.example.com/v1/"]
discover = true
env = ["API_TOKEN"]
credentials = { api = { prefix = "https://api.example.com/v1/", headers = { Authorization = "Bearer ${API_TOKEN}" } } }
disabled_commands = ["old-name"]
aliases = { "toolchain version" = ["rust", "version"] }
processes = [{ program = "rustc", args = ["--version"] }]
```

Loading the first command automatically resolves local or explicit remote TOML/JSON
manifests, validates their definitions against the embedding host, and saves the
lock. No extension code or process is executed while resolving metadata. Each
module is fetched only when its handler executes. A local manifest may use
`sha256 = "auto"`; a remote manifest must supply exact lowercase SHA-256 pins.
Relative module paths resolve against the source manifest directory/URL.
HTTPS is required for remote sources, with HTTP permitted for numeric loopback
addresses during local development. URL credentials, fragments, redirects,
traversal, and symlink paths are rejected. Metadata is bounded to 1 MiB and Wasm to
16 MiB; downloads have a 20-second timeout.

`[[cli.extensions]]` optionally accepts a semantic `version` requirement. For
paths/URLs it validates the supplied version. For `source = "registry:@acme/code"`,
`cli.registries` explicitly maps `@acme` to an index base URL and the requirement
selects a release. See [extension distribution](registry.md) for the static protocol.
An existing declaration keeps locked metadata until `lenso update`; changed
source/version/used-route declarations resolve automatically. Policy-only changes
such as narrower grants or aliases apply immediately without resolving packages.

`--locked` rejects a missing or mismatched lock. `--offline` prevents dependency
HTTP requests and can use previously cached source metadata and modules. `lenso
update` refreshes all source metadata; it rejects `--locked`. Cached source
metadata is reused until an explicit update. A changed declaration refreshes its
source metadata when an existing entry is being replaced. Failed definition
validation leaves the project lock and host registry intact. Missing/corrupt
module downloads do not replace valid cache entries. Modules referenced by remote
metadata are digest-checked before execution.

The shared user cache defaults to `~/Library/Caches/lenso` on macOS,
`$XDG_CACHE_HOME/lenso` or `~/.cache/lenso` on Linux, and
`%LOCALAPPDATA%/Lenso/Cache` on Windows. `LENSO_CACHE_DIR` overrides it with an
absolute path; `lenso cache path` reports the location. `v1/blobs/sha256/<digest>`
stores verified content, `v1/metadata/` indexes source snapshots, and `v1/locks/`
coordinates writers with OS locks. Projects with identical module bytes reuse the
same blob. Temp files and atomic replacement avoid publishing partial downloads.
Cache contains neither permissions nor interactive session state. Native Wasm
compilation caching is not implemented.

A warm help invocation uses only TOML and lock metadata. A first help may resolve
source metadata (and read local Wasm for `auto` digests) but never downloads remote
modules, executes extensions, or writes a project lock. Execution restores missing
cache entries under the exact locked digest; cache deletion does not upgrade pins.
`--version` bypasses project discovery. Old `.lenso/cache/` directories are unused;
local source packages remain valid and do not need to move.

`lenso config --json` inspects the root declaration without loading extensions.
`lenso migrate` stages and validates the legacy `.lenso/project.json` installation,
copies package sources into visible `lenso/extensions/` directories, writes root
TOML/lock, and removes only recognized old files after successful installation.
Existing root declarations are never overwritten by migration; unrelated old files
are preserved. Individual file replacement is atomic; migration is not a global
filesystem transaction.

All manifest types reject unknown fields. Extension IDs own handler and workflow
prefixes (`demo/...`); aliases also name their provider. Duplicate identifiers,
alias collisions, invalid dependencies, and reserved command names fail discovery.
Discovery applies to a cloned registry and commits only after all definitions pass.
Disabling an alias preserves its namespaced workflow; disabling an installation
removes its handlers, workflows, and aliases from the next invocation.

See [the demo manifest](../examples/m0/lenso/extensions/greeting/extension.toml) for workflows,
hooks, arguments, and command paths. Hooks run in this order: prepare, before,
primary handler, validate, after. Their outputs are available under `context.hooks`;
the primary output is `hooks.result`. Dependencies are available under `context.steps`.
Only declared dependency outputs are sent to each handler.

An Extism export accepts UTF-8 JSON encoding this context:

```json
{
  "abi_version": 1,
  "project": "/canonical/project",
  "input": {"name": "Rust"},
  "answers": {},
  "steps": {},
  "hooks": {},
  "capabilities": {"project.files": ["greeting.txt"]}
}
```

Knowing the project path does not grant Wasm access to it. The host supplies no
WASI filesystem, environment, process imports, or built-in network hosts. All answers
belong to the selected workflow and are visible to its handlers; do not put secrets
in the answer file. Secret-scoped contexts are future work. The engine never adds
the answer map to reports, but a trusted handler can intentionally return an answer
as part of its output.

Capability values originate from the installation record and cannot be replaced by
handler output. The optional project handlers check exact `project.files` paths
and descendants of `project.write_roots` before reading preimages or applying a plan.
Directory grants are opt-in; sibling prefixes, traversal, reserved metadata, and
symlinks do not become accessible. Granting a destination also permits showing its
existing content in a review diff; do not grant secret files unless that is intended.

The host exposes these imports in `extism:host/user`:

- `read_file(path: String) -> String`: a JSON-encoded string or `null` for an absent
  UTF-8 file. Requires an exact `host.reads` grant or descendant of a `host.read_roots`
  directory, rejects traversal/symlinks, and records a digest or observed absence.
- `http_get(url: String) -> String`: response content under an explicit `host.http`
  origin/path-prefix grant ending in `/`. HTTPS is required except numeric loopback
  HTTP for local development. Redirects and URL credentials are rejected. Responses
  are limited to 1 MiB and requests time out after 10 seconds. Evidence omits query
  strings; failures omit URLs containing resolved query values.
- `http_request(request: String) -> String`: a GET request encoded as
  `{url, headers?, params?, credential?, max_bytes?, select?}`. The same HTTP grants,
  redirect rejection, timeouts, request count, and evidence rules apply. Headers and
  params are literal strings; the guest cannot access environment values. Optional
  `credential` names a project-owned binding. Its prefix must match the URL as well
  as the extension's HTTP grant; it supplies host-expanded headers/params using only
  names in `env`. Caller headers/query keys cannot override bound credentials.
  Transport headers cannot be overridden. Credentials are injected only for workflows
  owned by the declaring extension, are never placed in Wasm contexts, and are not
  forwarded to other destinations or redirects. Evidence hashes public request
  identities, excluding expanded credentials and URL query contents.
- Large JSON responses can set `max_bytes` up to 16 MiB only with `select`, a map
  of output fields to `{op: "value" | "keys", pointer: "/json/pointer"}`. The Host
  performs generic JSON projection before returning at most 1 MiB to Wasm; it records
  the original response digest. At most 32 selected fields are allowed. For example,
  `{"versions":{"op":"keys","pointer":"/versions"}}` selects object keys;
  the Host does not interpret those keys as versions or know the response schema.
- `source_resolve(request: String) -> String`: accepts
  `{source: "github-repo:owner/repo", reference?: "HEAD", refresh?: false}`.
  Requires the workflow owner's explicit `sources = ["github:owner/repo"]` grant.
  Returns `{handle, commit, repository}` and prepares an owner-scoped repository
  resolution pin in the root lock. A fresh ref resolves only when not already pinned
  or explicitly refreshed. `--locked` forbids new/refreshed pins.
- `source_read(request: String) -> String`: accepts `{handle, path}` and returns
  UTF-8 source text. Handles are scoped to one Host invocation; guests cannot forge
  or transfer them. Reads use the pinned commit, verify regular-file tree modes,
  cache content, and record per-file SHA-256 in the lock and source evidence.
  Previously unobserved files require ordinary preparation before `--locked` use.
  This capability neither authorizes project writes nor interprets catalog formats.

- `list_files(request: String) -> String`: a JSON array of existing readable,
  project-relative file names. Requires opt-in `discover` (legacy `workspace` remains
  a deserialization alias) and is restricted by existing `reads`/`read_roots` grants.
  Request `{exclude?: ["directory-name"]}` can exclude up to 32 plain names. The
  Host skips Git/cache metadata and symlinks, limits traversal to 4096 directories
  and 16384 files, and returns at most 1 MiB. It never reads file contents; later
  reads still record their own evidence. Format-specific discovery and exclusion
  rules belong to extensions.

Installation `reads`, `read_roots`, `write_roots`, `http`, `env`, `discover`, and
`processes` populate host-owned capabilities. `credentials` stays in trusted Host
state rather than the serialized context. `host.cwd` provides the relative invocation
directory when available. Text/boolean arguments remain compatible; `kind = "list"`
arguments accept one or more strings and become JSON arrays.

The former `registry_get`, `npm_resolve`, and `workspace_dirs` business imports were
removed. Rebuild affected Wasm extensions and update their pinned package. JSON ABI
v1 context/outcome shapes, questions, approvals, and Agent exit codes are unchanged.
Business extensions parse their own configuration and use the generic imports.

Wasm complete object outputs receive a reserved `_lenso` field, replacing any
guest-provided value. It contains host-recorded reads, response digests, and actual
extension identity/version/module/export/digest. Optional `resources` records
host-owned repository snapshots and observed file hashes. Read contents are not added to
this evidence field.

The optional project planner collects that evidence from its declared dependency
outputs. Preset authors must include every input-producing step in the plan's
dependencies. Evidence from arbitrary transitive steps
or hook return values is not collected automatically. A plan validator must be pure or explicitly include its additional reads in evidence.

Project plans check their repository source pins before application; source changes
invalidate stale approval alongside file/read preimages. Resource pins are resolution
metadata, not installed-file baselines.

`project/plan` accepts a `commands` array alongside `files`, with exact
`program`/`args` objects. Those requests are hashed into the plan. `project/apply`
checks destination/read/process grants and gathers file and command confirmations
before writing any files, then applies files and runs commands in order. Command
failures report completed files and commands; they do not imply global rollback.

Project provider 0.1.1 also accepts `deletions`, an array of project-relative file
paths, alongside `files`. Replacements and deletions share the 128-operation limit,
path guards, exact preimages, read/write grants and consent digest. Duplicate or
ancestor/descendant destinations are rejected across both lists. Preflight checks
all destinations before mutations; replacements are staged, deletions applied,
then replacements persisted. Deletions never recurse into directories. Partial
failures report completed paths; no whole-plan rollback is implied. Extensions
using this field must require a compatible Project provider version.

`project/diff` builds the same read-only preview using file-read grants instead of
write grants. It never applies the plan or executes its command requests; application
still independently requires destination/read/process grants and consent.

Project provider 0.1.2 adds `finalizations`, exact second-phase replacements for
paths already present in prepared `files`. Their contents, paths and prepared
preimages are included in the original plan digest and preview. All file/command
approvals precede any mutations. After successful commands, Project checks the
prepared replacement/deletion postconditions and commits the finalizations.
Failed commands leave prepared state and never commit completion records. The
combined prepared/completion content shares the 1 MiB plan budget; duplicate or
undeclared completion paths are rejected. Empty finalization lists preserve the
existing command behavior. This is a generic completion mechanism, not native
package-manager logic. Native CLI 0.2.4 also supplies `host.offline` to extension
workflows so their own installers can honor the Host access policy.

Exports return exactly one tagged outcome:

```json
{"status":"complete","output":{"files":[{"path":"greeting.txt","content":"Hello!\n"}]}}
```

```json
{"status":"needs_input","question":{"id":"name","message":"Choose a recipient","kind":{"type":"select","options":["Rust","Wasm"]}}}
```

Other question kinds are `{"type":"text"}`, `{"type":"confirm"}`, and
`{"type":"multi_select","options":["button","input"]}`. Multiselect answers
are arrays of distinct declared strings; an empty selection is valid. A handler
must consume a supplied answer rather than request the same answered question
again. Question IDs cannot be shared by different steps. A step's unanswered
question pauses execution; no dependent step runs. An answer change invalidates
the owning step and its descendants. It discards later answers within that step
and all observed downstream answers, preserving earlier questions in the same step.
Applications may use `Session` directly to resume without a terminal.

Default limits: 16 MiB Wasm bytes, 256 memory pages (16 MiB linear memory),
10 million fuel units, 30-second call timeout, 1 MiB context, 1 MiB guest output,
and 256 logical HTTP calls per invocation. Compilation
is outside the call timeout; these limits do not represent a total host-process
memory/CPU quota. A synchronous host callback finishes under its own request timeout.
Module loading and compilation happen only at invocation time.

Native CLI 0.2.3 accepts optional `fuel` on a Wasm handler declaration, bounded to
1..=50,000,000 units. Omission retains the 10,000,000 default. Invalid budgets fail
metadata validation and invocation. Code's template composition and author
validation handlers request 50,000,000 units; these declarations do not add Host
grants or change memory/message/timeout limits. Extensions using this field must
require a compatible Host version.


The CLI emits one JSON object on stdout per invocation with `--json`. Result
objects use `type: result`, `status: complete|needs_input`, a workflow ID, completed
outputs, and an optional question. Source development also adds an `execution`
receipt and `code: missing_input` on unanswered questions. Errors use
`type: diagnostic`, `status: failed`, a public message, and the structured fields
described below. Help and version retain normal text output. Process logs go to
stderr. Exit codes: 0 complete, 2 missing input/parser error, 1 host/extension failure,
130 cancellation before application or from a standalone process, or the standalone
child's nonzero code (clamped to 1..255). Compound project application failures use 1
and include partial-application context.

## Public diagnostics (source development)

Diagnostics retain `type`, `status`, and `message`, and add `code`, `details`,
`retryable`, and `recovery`. Retry safety and recovery can be `null` when unknown.
Cancellation retains `status: failed` for compatibility and uses `code: cancelled`;
its execution receipt separately says `cancelled`. Consumers must use codes and
typed details rather than match message wording. Additional fields/codes can be
introduced; handle unfamiliar codes without implicitly retrying.

| Code | Public details / next action |
| --- | --- |
| `invalid_input` | Field/expected shape, JSON line/column, or parser category when known; supply corrected input |
| `command_conflict` | Both alias paths/providers; disable or rename one alias |
| `permission_denied` | Required capability name when known; explicitly review the project grant |
| `invalid_plan` | Correct unsafe paths, limits, or the approval digest and review again |
| `stale_plan` | Changed path or repository evidence; create and review a fresh plan |
| `partial_failure` | Plan digest/files/command indices or release ID/verified uploads, with a safe nested cause; inspect before recovery |
| `process_spawn_failed` | OS error kind; check the granted executable and working directory |
| `process_failed` | Actual child `exit_code`; retry safety is unknown |
| `cancelled` | Explicit user cancellation; no automatic retry |
| `wasm_timeout` | Host deadline and cancellation request; inspect effects before retrying |
| `wasm_fuel_exhausted` | Actual instruction-budget exhaustion; inspect the extension and limits |
| `wasm_failed` | Other Wasm failure without a safe, typed diagnosis |
| `invalid_response` | Invalid Wasm outcome protocol |
| `dependency_unavailable` | Offline policy when applicable; prepare or restore the dependency |
| `integrity_mismatch` | Artifact kind; restore pinned bytes or explicitly review an update |
| `io_error`, `operation_failed` | Safe OS kind or operation/status when known |

The Host never derives a code from error message substrings. Host-function errors
retain their typed diagnosis through Wasm. Fuel classification reads the engine's
consumption counter. A Host-owned deadline starts after plugin initialization,
requests Extism cancellation, and waits for the call worker before returning a
timeout. Existing HTTP callback time limits still apply; this does not promise
immediate interruption of a synchronous Host callback or bound compilation time.

`Diagnostic::new`, `with_details`, `with_retryable`, and `with_recovery` let trusted
native authors declare public errors. Authors must only add public field names,
capability names and receipts, never input values, expanded environment values,
authentication headers, credential URLs, or internal exception/cause strings.
Existing `Error::Invalid/Operation/Permission/Io/Json` values still work; their
arbitrary strings are deliberately excluded from JSON and terminal diagnostics,
including `--debug`. Legacy plugin error strings therefore get a conservative
category and generic message instead of exposing a guest error or stack trace.
Important native paths provide explicit fields; undeclared guest failures require
extension inspection. This intentionally changes message wording, not envelope
field names or established execution exit codes.

`Error::diagnostic()` provides that public projection. Session failures wrap the
original error in `Error::Execution` with a receipt. Rust callers can inspect its
`source`; `Error::exit_code()` preserves standalone child status and cancellation
through wrappers. A compound file/command application remains exit 1, with the
underlying child status or cancellation in `details.cause`. No global rollback or
cross-process exactly-once execution is implied.

These guarantees concern diagnostics generated by the Host. Successful outputs,
questions, native receipt projections, and external process logs remain authored
public content; trusted authors must not put secrets in those streams either.

## Result summaries and same-invocation reports (source development)

Default `--json` continues to include the original `outputs`. Use
`--json --summary` to omit step payloads and add `mode: summary`,
`schema_version: 1`. Both forms keep type/status/workflow and an `execution`
receipt containing an invocation ID, completed steps, actual handler/hook calls,
and the outcomes of declared validation hooks. The ID is for correlation, not a
persistent session address or an idempotency key. Answer revisions reset current
validation outcomes while retaining past call receipts; they do not undo effects.
`summary` and `report` are now reserved global argument names; extension arguments
with either name must be renamed rather than shadowing a Host control.

`verification_status` aggregates only the listed `hooks.validate` calls:
`passed`, `failed`, `not_run`, or `not_declared`. Ordinary steps named `verify`
are not inferred to be validators. A successful handler/command is not proof of
application health. `application_verification: not_reported` states that limit,
even when a plan-validation hook passed. Check the listed handlers and declared
verification instructions for the specific coverage; run application checks when
required. A failed validation records the preceding successful primary call and
any later checks that did not run.

Trusted native adapters can register `Engine::summarizer(handler, projection)` to
provide small `StepSummary` receipts. The engine does not guess effects from
arbitrary JSON keys. Missing receipts mean unknown. Project plan/diff receipts
include their digest and declare no application changes; apply receipts contain
actual affected paths and completed command counts. Partial application errors
carry completed paths and command indices against the reviewed plan, without
copying command arguments or file contents into diagnostics.

Use `--report NEW_FILE` alongside JSON to save the full result or execution-failure
envelope, execution receipt, and completed-step outputs from the same invocation:

```sh
lenso greet --name Rust --yes --json --summary --report execution.json
cat execution.json
```

Here `greet` is supplied by the [M0 fixture](../examples/m0), not a Core command.
`report` in stdout points to the actual saved file. Relative paths resolve from
the invocation directory. The Host reserves a new file before running handlers,
refuses existing paths/symlinks, and uses mode 0600 on Unix. It does not create
parent directories. A pre-execution error can leave the reserved file empty.
A post-execution write failure returns `io_error` with `report_saved: false`,
the observed outcome and execution receipt; inspect effects before retrying.
The file can contain partial JSON if writing failed. Saving a report is explicit
because full outputs may contain large source/diffs or authored private data.
Without `--report`, omitted payloads are not persisted and cannot be retrieved by
the execution ID; do not rerun a side-effecting command merely to inspect them.

Summary mode preserves the full pending question, including approval review
content. It reduces output payloads, not the information needed for consent.
`--yes` still answers only explicit confirmations, never selections or capability
grants. Error diagnostics always retain safe partial-progress information in
either output mode.

Run `python3 scripts/build-demo.py` and `python3 scripts/test-agent-protocol.py`
after building the CLI for the real Wasm/file-plan compatibility and byte-size
comparison. Measurements are recorded in [implementation status](agent-native-status.md);
this protocol makes no universal token-cost or fixed percentage claim.

## Native terminal adapter

Clap owns command parsing/help, `inquire` renders questions, `indicatif` renders
progress, and `console` styles results/diffs. Native `EventSink` notifications are
bounded and transient; they are excluded from serialized `Context` and JSON results.
Step status notifications are best effort; result documents remain authoritative.
Process logs use backpressure and remain on stderr. Human output gives a compact
summary; `--debug` adds the full report on stderr. Hosts may consume `EventStream`
for a different presentation without changing workflows.

`--json`, `--no-interactive`, CI, and nonterminal input prevent prompts. Only human
TTY sessions show progress animations. Esc goes to the previous question (or
cancels when there is none); revised answers invalidate dependent consent. The
file-review selector offers a full diff, then command approval asks separately.
`--yes` answers confirmation questions only. It grants no capabilities and does
not answer select/text/multiselect questions.

`--color auto|always|never` controls human output and help. Auto respects `NO_COLOR`;
explicit always overrides it. JSON mode never adds host styles or progress even
with `--color always`; it retains one result/diagnostic on stdout and child logs on
stderr. ABI v1 context/outcome fields and existing result fields are retained;
source development adds the receipts and diagnostics described above.

GitHub Release transport, author package metadata and current limits are documented
in [GitHub distribution](github-distribution.md). Generic HTTP redirect rules are
unchanged; GitHub transfer uses its own bounded, credential-stripping provider.
