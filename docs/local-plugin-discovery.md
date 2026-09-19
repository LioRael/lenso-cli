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
