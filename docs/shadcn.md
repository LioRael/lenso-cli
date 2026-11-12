# shadcn Registry consumer

The preset consumes Registry data through restricted Rust host functions and runs
as Wasm. Source files, dependency declarations, CSS, and environment-file defaults
become one reviewed plan. The host owns reads, HTTP, credentials, approval, writes,
and package-manager processes. It does not execute commands found in Registry docs.

## Supported resources

- Official names such as `button`, `input`, `dialog`, and `badge`; selection reads
  the live official catalog instead of maintaining a component allowlist.
- Explicit HTTPS Registry-item URLs, configured `@namespace/name` addresses, and
  project-local JSON items. HTTP is allowed for numeric loopback development servers.
- Multiple item arguments, recursive dependencies across those address types,
  cycle detection, and bounded depth/item counts. Bare dependency names always refer
  to the official Registry. Relative JSON dependencies follow their parent source.
- `registry:ui`, `registry:lib`, `registry:component`, `registry:hook`,
  `registry:block`, `registry:page`, `registry:file`, `registry:item`,
  `registry:style`, and `registry:theme` items.
- Complete file content, explicit targets, `~/` workspace-root targets, and
  `@components/`, `@ui/`, `@lib/`, and `@hooks/` target placeholders.
- Runtime/development npm dependencies, including tags and Node-style semver
  ranges. Missing dependencies resolve to exact versions before review. Existing
  dependency/peer declarations are preserved. Conflicting new requirements fail.
- Tailwind 4 `css` and `cssVars`, reviewed `.env` defaults that preserve existing
  values, and post-install `docs`. Reapplying identical CSS is idempotent.

Dependencies are processed before parents. Later Registry files can override earlier
files targeting the same destination, matching the upstream dependency ordering.
Existing differing application files still produce explicit overwrite/keep questions.
Merged package/CSS/env files appear in the final diff and require plan approval.

The target must already have a TypeScript configuration and a Tailwind 4 CSS entry.
`init` can create `components.json`; it does not scaffold a framework, CSS theme, or
monorepo. The preset preserves `rsc` and existing source directives; it requires
`tsx: true`, CSS variables, and no Tailwind class prefix. Conventional source imports and references to files in the resolved Registry graph
are rewritten using destination aliases, relative imports, and source exports.

## Search, differences, and component updates

```sh
lenso search --query button
lenso list @private --type registry:ui --limit 20 --offset 20
lenso search @private @public --query navigation
lenso search https://registry.example.com/r/registry.json --query card
lenso diff button @private/dashboard --cwd apps/web
lenso components update button @private/dashboard --cwd apps/web
```

`search` and its `list` alias default to the official catalog. Namespaces use the
active workspace's `components.json`, fetching `{name}` = `registry` with the same
HTTP/environment grants and authentication as installation. Search needs no valid
Tailwind, React, or package-manager setup. A namespace still needs a readable
`components.json`; ambiguous workspace configurations prompt for a choice.

Static catalogs filter names, titles, descriptions, and categories locally, with
all query words required and optional comma-separated item types. Dynamic catalogs
receive `q`, `type`, `limit`, and `offset`. A valid `pagination` response marks
server-filtered results: their ranking and matches are preserved. Configured auth
params cannot collide with those four search keys. Single-registry searches use
the server page. Multi-registry searches request `offset + limit` rows starting at
zero, concatenate in registry argument order, and paginate the combined results.
Servers that cap their page size may leave deeper combined pages unreachable.
Limits are 8 registries, 1–1000 returned rows, offset 0–10000, and bounded responses.
The JSON `search` output contains `results` with installable `add` references and
`pagination: {total, offset, limit, hasMore}`. Catalog URLs assume sibling
`<name>.json` item URLs; use a configured namespace for other layouts or credentials.

`diff` resolves the current upstream graph and compares fully rewritten destination
files, package declarations, CSS, and env defaults. It never asks overwrite/keep
questions, applies files, or executes commands, and needs only file-read grants.
The `plan` output includes the full diff and proposed dependency commands.
Cold dependency preparation can still create `lenso.lock` and fill the user cache.

`components update` accepts the same source references as `add`, for components
with at least one existing destination file in the selected workspace. New files
and dependencies in their current upstream graph join the reviewed plan. Omitting
items asks for a source reference. Sources are explicit because existing generated
files cannot reliably identify their registry; no component tracking file is added.
Core `lenso update` continues to refresh extension dependencies.

Updates retain overwrite/keep choices for differing source files, then collect
file and command approval before application. `--yes` does not choose overwrite.
For example, `--answers` can supply `{"conflict:src/components/ui/button.tsx":"keep"}`;
a changed file still requires its digest-bound approval unless `--yes` is used.
Results include `summary: {checked, changed, unchanged, kept}` and the kept paths.
All three workflows use the existing JSON/Agent protocol and workspace routing.
This compares local files to today's Registry; it does not infer which side changed,
perform three-way merges, discover every installed custom source, or delete files
removed upstream. Existing npm dependency declarations remain unchanged.

## Registry namespaces and authentication

Configure registries in the active workspace's `components.json`:

```json
{
  "registries": {
    "@public": "https://registry.example.com/r/{name}.json",
    "@private": {
      "url": "https://registry.example.com/private/{name}.json",
      "headers": { "Authorization": "Bearer ${REGISTRY_TOKEN}" },
      "params": { "channel": "${REGISTRY_CHANNEL:-stable}" }
    }
  }
}
```

The extension parses these entries. Its development configuration helper copies
auth templates into root `lenso.toml` `credentials` bindings, with opaque names such
as `components.json#@private` and destination prefixes. Review those bindings when
Registry auth or URLs change; changing `components.json` does not silently grant a
new credential destination. Existing source packages using the old business Host
imports must be rebuilt with `configure-shadcn.py`, then resolved/updated.

The host expands `${NAME}` and `${NAME:-fallback}` only for names explicitly granted
in the extension's `env` declaration in `lenso.toml`. Export the values in the CLI
process environment; `.env` and `.env.local` credential files are not automatically
loaded. Resolved values remain in the host and are not serialized into Wasm contexts,
reports, the lock, or the shared artifact cache. Reported configuration omits Registry
headers/params and removes URL queries. Authentication is not inherited by another
namespace, a direct-URL dependency, or a redirect. Redirects are rejected.

Registry requests also require an explicit `http` origin/path-prefix grant. Missing
variables, missing grants, HTTP errors, and invalid payloads fail before application.
Source evidence records response hashes and public request identities, excluding
expanded query credentials. Environment-file payload values are literal defaults;
they do not read or expand authentication variables.

## Monorepos

Declare Lenso at the repository root. The extension discovers npm workspaces and
`pnpm-workspace.yaml` packages through the generic, explicitly granted `discover = true` inventory capability.
The extension interprets workspace metadata and globs; runtime only enumerates
readable file names. Legacy `workspace` is accepted as an alias for `discover`.
Discovery is bounded, respects exclusions, skips reserved/build directories and
symlinks, and exposes file names rather than file contents. Each actual file
read still requires its own read grant.

```sh
# Select explicitly from the repository root.
lenso preview @private/dashboard --cwd apps/web
lenso add button input --cwd apps/web

# Or run inside the app; the nearest ancestor lenso.toml selects the repository.
cd apps/web
lenso add dialog
```

Without an unambiguous active workspace, the CLI asks a workspace question; JSON
mode returns that same question with `needs_input` and exit 2. Answers can be supplied
through `--answers` or `--input` as before.

App components/pages/files use the active workspace. UI components, hooks, utilities,
and libraries route to the shared UI package identified by `aliases.ui` and its
configuration. Dependency declarations follow the owning package; the root install
command handles workspace linking. Cross-package imports use package exports and
package-local `#...` imports; editable source exports are required. Relative local
and workspace-package tsconfig inheritance is supported. The participating UI/app
configurations must agree on style, icon library, and base color.

npm, pnpm, and bun use these exact root invocations when granted:

```sh
npm install --ignore-scripts --no-audit --no-fund
pnpm install --ignore-scripts
bun install --ignore-scripts
```

Processes receive PATH only. Configure the package manager's own project/user
network settings when needed; Registry credentials are not passed to child processes.
If installation fails, source files may already be applied. The diagnostic reports
partial work. Repeating a dependency-bearing install still offers the install command,
including when its source files already match, so dependency installation can be retried.

## Local development and permissions

```sh
cargo build --workspace
python3 scripts/configure-shadcn.py PROJECT --allow-install
lenso preview input dialog
lenso add input dialog
```

The helper builds an immutable local source package and declares it in root
`lenso.toml`. It gathers workspace metadata, source-directory grants, Registry HTTP
prefixes, and authentication variable names from configuration. It does not save
credential values or prepare runtime artifacts. The first command creates/updates
`lenso.lock` as needed and uses the per-user artifact cache.

`files`/`reads` retain exact-path semantics. Optional `write_roots`/`read_roots`
allow descendants of explicitly named project directories; sibling prefixes,
traversal, Git/cache metadata, node_modules, and symlink destinations are rejected.
The helper accepts additional `--read-root`, `--write-root`, and `--http` grants.
Root files such as `.env` need explicit file grants if requested by a payload. Review
and narrow generated declarations for your project.

Previews do not change application files or run package commands; cold preparation
can create `lenso.lock` and populate shared cache. File and command consent are both
collected before application. Revising an upstream answer invalidates dependent
consent. `--yes` answers confirmation questions only and grants no capabilities.
JSON/Agent mode emits the same result/diagnostic protocol and keeps process logs on
stderr. Terminal mode retains searchable choices, Esc/back, full diffs, and summaries.

## Boundaries

This is a Registry consumer, not complete official CLI parity. It does not build or
publish registries, scaffold projects/monorepos, resolve GitHub shorthand, configure
fonts/base presets/style inheritance, transform TypeScript into JavaScript, rewrite
Tailwind 3 configuration, or support Yarn or icon-library switching. External node_modules tsconfig inheritance,
compiled-only workspace exports, and ambiguous alias targets are rejected. Registry
items must contain built file content. Existing dependency declarations are not
upgraded automatically. CSS additions use stable per-item blocks rather than a full
CSS AST transform. Validation of application behavior remains the project's build/tests.

Generic engine limits still apply: 128 planned files, 1 MiB plan content and ABI
messages, 16 MiB modules/linear memory, 10 million fuel units, 30-second Wasm calls,
and 256 logical HTTP requests per invocation. Registry responses are bounded to
1 MiB; generic Host JSON projection can reduce up to 16 MiB of metadata before
returning it to Wasm, where the extension performs npm version selection. These bounds can reject very large Registry items.

## Validation

```sh
cargo test --workspace
python3 scripts/test-shadcn.py
python3 scripts/test-shadcn-registry.py --pnpm
```

The live official acceptance checks button plus multi-item input/badge/dialog
installation, detection, catalog selection, Registry resolution, permissions,
conflicts, npm installation, and a Vite build. The next-stage
acceptance uses real Wasm with authenticated local Registries, cross-namespace and
URL dependencies, local resources, explicit targets, CSS/env merges, denial without
writes, installation recovery, range conflicts, idempotence, credential isolation,
static/dynamic search and paging, read-only diffs, explicit overwrite/keep updates,
and single-project/monorepo builds with npm and pnpm.
The npm metadata still comes from the live official npm Registry.

Primary references: [Registry item schema](https://ui.shadcn.com/docs/registry/registry-item-json),
[Registry namespaces](https://ui.shadcn.com/docs/registry/namespace),
[authentication](https://ui.shadcn.com/docs/registry/authentication),
[dynamic search](https://ui.shadcn.com/docs/registry/dynamic-search), and
[monorepos](https://ui.shadcn.com/docs/monorepo).
