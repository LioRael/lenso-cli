# Local Plugin discovery

Status: first implementation slice of [plan #727](https://github.com/LioRael/lenso/issues/727).
Discovery is read-only. Automatic Host assembly and zero-configuration App
creation/development remain subsequent work.

```sh
lenso app discover
lenso app discover --root /path/to/my-app --json
```

No Host, Host Catalog, or App configuration is required. The command discovers
Plugin projects under `app/`. Add optional local sources in `lenso.toml` only
when needed:

```toml
plugin_sources = ["../shared-plugins", "../packages/*", "../artifacts/example.lenso-plugin"]
```

This is tooling configuration, not an App manifest. It accepts no enabled
list, binding map, preset, or activation flag. Existing `plugins/` files remain
the only instance/configuration difference authority. No marketplace source is
configured here.

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
may inherit `workspace.package.version`. Native Web scaffolds use the existing
`root-slot = "web"` discriminator; other projects retain existing CLI runtime
defaults. This is compatibility with current scaffolds, not a universal rule
that Web Capabilities imply native execution.

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
names. A future build adapter must normalize these before implementation selection.
No new handwritten Descriptor, Schema, or plugin manifest is introduced.

## Support baseline and remaining integration

Inspected CLI base: `7fb23fbfa943dcb1e7fa0a97cd684f7059e0d47b`.
Runtime source inspected: `d7eb465baa2e668ed2378fa832aab1769965a3d9`;
Bun: `92fd7e09ddeff87cce6f7b51dc6ec05eee4ced73`;
protocols: `c1c9c0a3d1c8dfc906c70f0e2e5e929122fde1ee`.
Runtime/protocol remote main have advanced; the next assembly slice must inspect
their fresh bases and release cohorts before using new interfaces. This table
is source evidence, not proof of a released mixed-App distribution.

| Source / implementation | Existing evidence | Discovery slice | Assembly follow-up |
| --- | --- | --- | --- |
| Rust native linked | `src/plugin/web_dev.rs`, Web scaffold | Existing Web metadata recognized | Generate a general Host, beyond single Web Plugin dev |
| Rust Process | `src/plugin.rs`, `src/plugin/dev.rs` | Runtime and multi-output metadata | Reuse real Process Adapter for selected target |
| Rust Wasm | Same CLI paths, Wasm Component Adapter | Runtime and multi-output metadata | Preserve declared artifact target and Host admission |
| TypeScript/Bun | `assets/plugin-build.mjs`, Bun dev path | Existing package metadata | Mixed Host dependency/runtime integration |
| Composite Rust + Bun | Existing `implementations` authoring | Parent plus bounded child locators | Build each implementation and validate one Contract |
| QuickJS / dylib | Runtime-owned Adapter crates | Verified Bundle metadata only | Source extractor/build integration and exact target proof required |
| Web resources | Selected Plugin-owned resources | Remain inside owning project | Connect build/dev server lifecycle in later slice |
| Older native projects without identity metadata | Native linked registration | Not guessed from crate names | Source-derived locator/extractor integration required |
| Python / other SDKs | No supported path established here | Not claimed | Separate SDK/Adapter work, outside this slice |

## ADR relationship and subsequent Host contract

ADR 0070 remains unchanged by this read-only command. Enabling automatic App
assembly requires an explicit architectural adoption: discovery is Host-authoring
input, generating explainable defaults; Plugin Root retains configuration,
disabled markers, and Host-permitted named dependency choices. A discovered
shared candidate is not enabled until explicitly adopted through Plugin Root.

The subsequent architecture slice must freeze default Instance identity,
shared-source adoption commands, template policy locking, and custom Host
selection before activation is implemented. It must also settle the proposed
`create/dev/build` convenience roots: `dev` is currently retired and other roots
can belong to dynamic terminal Plugins. This slice uses `app discover` and does
not silently change existing root command routing.

Validation covers mixed source discovery, workspace membership/version
inheritance, composite identity, duplicate/overlapping sources, ignored output
trees, local path configuration, corrupt archives, and symlink cycles. This
does not substitute for real mixed-App invocation and offline build proof.
