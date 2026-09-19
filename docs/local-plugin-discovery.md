# Local App development

The local Host template implements [plan #727](https://github.com/LioRael/lenso/issues/727)
and [ADR 0075](https://github.com/LioRael/lenso/pull/730). These commands require a
CLI build containing the local App workflow; older published CLI versions do not
provide it.

```sh
lenso app create my-app --runtime bun
cd my-app
lenso app dev
# In another terminal, or after stopping development:
lenso app build
lenso app start --from dist
```

No App configuration file, Host declaration, preset, or activation flag is required.
`app/` contains App-owned Plugins. `plugins/` retains instance configuration,
disabled markers, and named dependency choices. `app create --web` creates a native
Rust Web Plugin with a Plugin-owned HTML page. `--runtime process` is the default;
`bun`, `wasm`, `multi`, and `empty` are also available. `--no-install` leaves normal
language dependency installation to the developer.

Add **local discovery sources** only when needed:

```toml
# Optional lenso.toml, relative to this App root.
plugin_sources = ["../shared-plugins", "../packages/*", "../artifacts/example.lenso-plugin"]
```

Shared candidates are discoverable but are not built/admitted until an existing
Root file such as `plugins/example.audit/default.toml` explicitly selects them.
An empty/comment-only TOML file uses the Plugin's defaults. App-owned Plugins get
a disableable `default` Instance. Duplicate identities, ambiguous providers,
invalid configuration, and disabled required providers fail before publication.

## Build, inspect, and run

```sh
lenso app discover --json
lenso app build --out ./dist-release
lenso app check --root ./dist-release
lenso app show --root ./dist-release --json
lenso app start --from ./dist-release --check
lenso app start --from ./dist-release
```

Build creates a new output directory; it never overwrites an existing one. Source
builds use the existing Plugin builders and normal installed language dependencies.
Native Plugins need Cargo and expose the SDK-generated `link_plugin` anchor.
Their normal Cargo contract dependencies supply typed runtime codecs; no parallel
handwritten Capability schema is required. Incompatible codec cohorts fail with
an error. Bun-only Apps use the precompiled CLI runtime and need no Rust toolchain.

The output includes the executable Host, resolver, selected artifacts, Bun when
needed, exact Host authority, Root intent, and integrity metadata. Startup verifies
immutable runtime files and uses the ordinary resolver/Kernel path. The distribution
runs without the source tree, Cargo, Bun on PATH, or runtime downloads. It is a
runtime closure, not a portable source-reproduction archive. `intent/plugins/`
remains the editable Root snapshot; `--root PATH` can select another already
initialized Plugin Root. `--check` performs real activation and clean shutdown.

`app assemble --out PATH` retains the authoring-only path for portable Plugins;
`--executable` requests the runnable closure. Native assembly necessarily generates
a Host. Existing `app build --source host.ts --target TARGET --out PATH`, custom
Hosts, and `app prepare` retain their own contracts. Dynamic terminal Plugin command
names remain available because convenience commands live under `app`.

## Development loop

`app dev` watches App source, Root intent, optional local sources, and native Cargo
path dependencies. Changes are debounced, rebuilt into a separate directory, then
restart the Host with graceful shutdown. Failed builds keep the last running App.
A successful build followed by a startup failure is reported; automatic rollback
or zero-downtime switching is not claimed. Ctrl-C stops the active build/Host.
Generated output and dependency/cache trees are excluded from watching.

Web routes and assets belong to the Web Plugin. The starter embeds its own HTML;
editing it triggers the ordinary rebuild/restart. Native instance resources are
loaded from the Root snapshot. There is no separate frontend framework or implicit
business route registry in the Host.

## Supported local runtime profile

| Source | Build/runtime path | Boundary |
| --- | --- | --- |
| Rust native linked | Generated Host + normal SDK factories | Cargo required at build time |
| TypeScript/Bun | Existing Bun builder + shipped Bun Adapter | Bun required at build time; bundled for deployment |
| Rust Process | Existing release builder + Process Adapter | Native build target |
| Rust Wasm | Existing Wasm builder + Wasm Component Adapter | Existing SDK target/toolchain required |
| Composite Rust/Bun | Existing composite builder and Contract equivalence | One deterministic implementation selected |
| Web | Native HTTP Endpoint Plugin + generated ingress | Assets remain Plugin-owned |
| QuickJS / dylib | Discovery can inspect verified Bundles | Local source/runtime integration deferred |
| Python / other languages | No SDK path established here | Not claimed by this workflow |

The current executable profile supports macOS ARM64 and Linux x86_64. Other
platforms fail explicitly instead of choosing a different runtime. Pure portable
Capabilities support Request interactions through verified generated Descriptor
evidence. Stream/Event boundaries require typed codecs from native contract
projections. Old Bun archives without embedded generated Descriptor evidence need
repacking for the generic portable Host; custom typed Hosts remain available.

## Discovery contract

- Paths are relative to the App root, independent of the calling directory.
  Absolute paths and component globs (`*`, `?`, character classes) are supported.
  A directory is scanned recursively; recursive `**` globs are rejected to keep
  traversal bounded. Missing explicit roots and unmatched patterns are errors.
- `app/` candidates have `app_owned` provenance. Extra roots have `shared`
  provenance. Neither value means enabled or admitted. Scanning never writes
  Plugin Root files or starts a build, package script, Plugin, or Generation.
- Cargo and npm workspace roots select declared members (including Cargo
  excludes). Otherwise directories are containers. Workspace members outside
  their workspace require an explicit source entry. pnpm-only workspace YAML
  parsing is deferred; configure its package directory/glob explicitly.
- Recognized Plugin projects terminate traversal. Composite implementation
  subprojects belong to their parent Plugin, not additional default Instances.
- Ignore `.git`, `.lenso`, `target`, `node_modules`, `dist`, `build`, `.next`,
  `.venv`, `__pycache__`, `plugins`, and hidden directories during traversal.
  An explicitly named root can still point to an artifact in an output directory.
- Canonicalize roots and deduplicate repeated same-role paths. Reject any
  encountered overlap between App-owned and shared roots. Skip nested symlinks;
  an explicit source symlink resolves to its canonical root. Metadata files must
  be regular files. Traversal is limited to 64 levels and 50,000 visited entries;
  metadata is limited to 4 MiB. Existing archive verification bounds still apply.
- Sort candidates by Plugin ID. Distinct projects with the same Plugin ID fail
  even if their versions differ. Filesystem order never selects a Release or
  dependency provider. Unsupported declarations and malformed metadata fail
  with paths/context instead of becoming execution authority.

## Reused metadata and evidence

Cargo projects use existing `[package.metadata.lenso]` (`plugin-id`, `root-slot`)
and optional `[package.metadata.lenso-cli]` implementation declarations. Versions
may inherit `workspace.package.version`. Native projects are recognized from their normal `lenso`/`lenso-native-adapter`
SDK dependency or explicit `runtime = "native-linked"`. Contract-only crates are
not Plugins. Existing Web scaffold metadata remains supported; language,
execution class, and business Capability remain independent.

Bun projects use existing `package.json` `lenso.pluginId`, `lenso.runtime`, and
package version. SDK packages exposing only `lenso.build` are not business
Plugins. A project declaring both Cargo and Bun Plugin metadata is ambiguous;
use the existing composite declaration and separate implementation directories.
Composite identities and versions must agree; generated Contract equivalence
still requires the existing build/pack checks.

Source results say `source_metadata_only`: they identify build locators and
declared runtime choices, not extracted Capabilities, executable availability,
or successful admission. Local Bundle directories/archives use the existing
Bundle verifier and report `verified_bundle_not_admitted`. Bundle implementation
runtime values are exact execution-class IDs; source values are authoring runtime
names. Assembly normalizes these before explicit implementation selection.
No new handwritten Descriptor, Schema, or plugin manifest is introduced.

## Optional surface packages

Local convention selection is an authoring/build feature. It selects existing
Plugin packages or invokes an adopted compiler for standalone entry files.
See [convention authoring](convention-authoring.md) for the complete CLI example.
Run `lenso app inspect --json` to inspect the selection without executing code.

A support Plugin declares recognized filenames in its existing Lenso metadata
(`lenso` in package.json, or `package.metadata.lenso` in Cargo.toml):

```json
{"conventions":[{"id":"example.cli","entries":["cli.ts","cli.rs"]}]}
```

An owning Plugin declares independently buildable contributions in that same
metadata:

```json
{"surfaces":[
  {"entry":"cli/src/cli.ts","project":"cli"},
  {"entry":"tui/src/tui.rs","project":"tui","required":false}
]}
```

The support must have an active App-owned default or an explicit, non-disabled
Plugin Root instance. An unselected shared source grants no support. Recognition
conflicts fail regardless of filesystem ordering. Required surfaces without
support fail before compilation; optional ones report `support_not_adopted`.
Inactive package manifests are not parsed, and their dependencies are not passed
to package managers by this workflow. Dependencies deliberately placed in the
core package or its workspace remain the author's responsibility.

For mixed-language products, an optional `plugin.json` owns nested packages:

```json
{
  "schema":"lenso.plugin-project.v1",
  "core":"core",
  "surfaces":[{"entry":"cli/src/cli.ts","project":"cli"}]
}
```

The core package supplies the logical identity and version. Every selected
surface is an ordinary Plugin with a distinct runtime identity and the same
release version. Its own package manifest controls compilation. Selected
contributions receive disableable defaults and retain normal descriptor, binding,
and permission validation. There is no automatic cross-language source import,
configuration forwarding, permission inheritance, or descriptor merging.

This first profile requires one active owner instance. A multi-instance owner
fails rather than silently sharing a surface instance. Owner/support activation
is evaluated when building; changing it requires rebuilding. Runtime editing of
a distribution's Plugin Root does not recompute the build selection. Generated
`.lenso/conventions.json` records selection provenance. A changed selection during
a build aborts publication of that output.

Simple single-package Plugins need no composite manifest. Existing multi-runtime
implementations remain alternatives for one contract, separate from additive
surface packages. Compiler extensions lower standalone entries to ordinary source packages or
verified Bundles. Bundled CLI support recognizes `cli.ts` and `cli.rs`; bare
App-owned directories also work without a package manifest. Registry installation
is outside this local-source workflow.

Bun packages may set `lenso.source` to a package-relative entry file, for example
`"src/cli.ts"`. The default remains `src/plugin.ts`. The source must resolve to a
file inside its package. This changes the authoring entry location, not the
runtime contract: the entry still exports an ordinary `definePlugin` declaration.
