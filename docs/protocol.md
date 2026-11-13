# Automatic dependency preparation and JSON ABI v1

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
outputs, and an optional question. Errors use `type: diagnostic`, `status: failed`,
and a message. Help and version retain normal text output. Process logs go to
stderr. Exit codes: 0 complete, 2 missing input/parser error, 1 host/extension failure,
130 cancellation before application or from a standalone process, or the standalone
child's nonzero code (clamped to 1..255). Compound project application failures use 1
and include partial-application context.

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
stderr. Existing ABI v1 context fields and result documents are unchanged.

GitHub Release transport, author package metadata and current limits are documented
in [GitHub distribution](github-distribution.md). Generic HTTP redirect rules are
unchanged; GitHub transfer uses its own bounded, credential-stripping provider.
