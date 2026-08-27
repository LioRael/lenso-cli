# lenso-cli

The authoring CLI for installable Lenso Plugins, built-in Modules, and App Plans.

This repository also owns the `lenso-authoring` library extracted from
`LioRael/lenso` under ADR 0064. The library validates authoring inputs,
resolves immutable `ResolvedAppPlan` artifacts, and assembles the selected
Runtime Driver and Execution Adapters when a plan is run.

Project-wide agent workflows live in the
[`LioRael/lenso` skill pack](https://github.com/LioRael/lenso/tree/main/skills).

## Install

```sh
npm install -g @lenso/cli
# or
cargo install lenso-cli
```

## Plugin authoring golden path

Harness extension authors use one Plugin namespace from source creation through
immutable packaging:

```sh
lenso plugin new uppercase
cd uppercase
lenso plugin dev
lenso plugin check
lenso plugin pack
```

The generated Rust/Wasm project contains one Plugin ID/version and one source
declaration. That declaration produces both runtime behavior and the static
descriptor evidence consumed by packaging. Authors do not write a Module,
Manifest template, contribution array, digest, execution class, trust level, or
Plan. `pack` builds and reopens the exact `.lenso-plugin` directory it writes;
the Harness verifies received bytes again during installation. There is no
normal `plugin verify` step.

The first public Plugin shape is one request-style Rust-authored Wasm Component.
`plugin dev` runs the packaged Component through the production Wasm Execution
Adapter. Bun, QuickJS, process, and native-dylib Plugin scaffolds are not yet
claimed.

Users upgrading from the earlier dual Module/Plugin workflow should read the
[Plugin authoring migration guide](docs/migration-plugin-authoring.md).

## Advanced built-in Module authoring

App owners who intentionally compile behavior into their Host retain the
existing workflow under an explicit namespace:

```sh
lenso module new greeting
cd greeting
lenso module check
lenso module dev
lenso module verify
```

The Rust starter uses the public `lenso` facade and generated Capability
contracts. Module Descriptor, binding, Plan, and Runner stages remain available
for this advanced built-in path.

Maintainers can run `scripts/measure-dx.sh` to capture comparable millisecond
timings for scaffold, initial source generation, a fresh check, an incremental
source check, and verification. The script reports measurements rather than
embedding an unsupported performance threshold.

## Source-derived App Definitions

Modules authored by a derivation macro embed a package-owned Module Descriptor
in their compiled Cargo artifact. An App Definition selects packages and keyed
Instances without repeating Capability IDs, operation tables, bindings,
execution classes, or lifecycle policy:

```json
{
  "schema_version": 1,
  "manifest": "Cargo.toml",
  "host_package": "example-host",
  "packages": {
    "example.text-tools": "example-text-tools-module"
  },
  "app": {
    "name": "example",
    "modules": [
      { "key": "text-tools", "package": "example.text-tools" }
    ],
    "decisions": []
  }
}
```

```sh
lenso app add example-text-tools-module \
  --definition lenso.app.json \
  --version '^1.0' \
  --configuration '{"prefix":"docs"}'

lenso app check --definition lenso.app.json
lenso app resolve --definition lenso.app.json \
  --output .lenso/resolved-plan.json

# Remove only this App-local Instance; the Host may keep the dependency.
lenso app remove text-tools --definition lenso.app.json

# Remove the dependency too when no other Instance uses it.
lenso app remove text-tools --definition lenso.app.json --uninstall
```

`host_package` names the Cargo package that statically links the selected
Modules; `app add` and `app remove --uninstall` edit that package's
dependencies. `app add` delegates dependency and lock ownership to Cargo,
discovers the runtime package id from the package-owned Descriptor, chooses a
useful default Instance key, and updates the small App Definition. Use `--path`
for a local package or `--git` with `--rev`, `--branch`, or `--tag` for Git.
`--dry-run` performs the complete build and resolution check, reports touched
files, and then restores them byte-for-byte.

Every edit is transactional: dependency files and the App Definition are
restored when Descriptor discovery or composition fails. The CLI builds only
the selected locked Cargo packages, reads Descriptor bytes from their artifacts
without executing package code, derives unambiguous bindings, and writes the
same immutable Plan format consumed by the Kernel.
`one` and `optional` ambiguities require an explicit App Definition decision;
`many` providers are ordered deterministically.

For non-trivial static Module settings, keep one reviewed TOML file per
Instance and reference it from the App Definition:

```json
{
  "key": "text-tools",
  "package": "example.text-tools",
  "configuration_file": "config/modules/text-tools.toml"
}
```

```toml
prefix = "docs"
max_items = 100
```

The path is intentionally fixed to `config/modules/<instance>.toml`.
`configuration` and `configuration_file` are mutually exclusive. The TOML
table is treated as the ordinary App-owned Module configuration overlay, merged
with package defaults and validated against the package-owned Schema before a
Plan is produced. Stateless Modules need neither field. The Kernel and resolved
Plan have no file-reference concept.

Products may keep additional App-owner intent in the optional top-level
`extensions` object. Keys must be product-namespaced, and each product owns the
value's schema and meaning. `lenso-authoring` preserves these JSON values and
exposes them to product Hosts without interpreting them or adding them to the
generic App Composition. Transactional `app add` and `app remove` edits retain
unrelated extensions.

## Plugin packages

`plugin pack` derives one schema-V2 entry from source and Cargo metadata,
componentizes the exact release Wasm, calculates its digest and size, and
publishes a non-overwriting directory:

```sh
lenso plugin check
lenso plugin pack
```

Packaging reads descriptor evidence without instantiating publisher code. It
does not grant permissions, admit a Release, or switch a running App
Generation; those remain Harness-owned operations.

## Scope

The CLI exposes user intent rather than its internal check, resolution, recipe,
Plan execution, and Adapter assembly stages:

```text
lenso plugin new
lenso plugin dev
lenso plugin check
lenso plugin pack
lenso module new
lenso module dev
lenso module check
lenso module verify
lenso app add
lenso app remove
lenso app check
lenso app resolve
```

`app check` and `app resolve` remain explicit advanced commands for App owners
and Hosts that exchange canonical Plan bytes. Deprecated top-level
`new/check/dev/verify` and template-based Plugin Bundle commands remain hidden
for one compatibility window.

Runtime extensions, product Modules, deployment systems, and Console operations
belong to their owning repositories rather than this authoring CLI.
