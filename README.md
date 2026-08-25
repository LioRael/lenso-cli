# lenso-cli

The authoring CLI for Lenso App Plans and Modules.

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

## Module authoring golden path

Ordinary Module authors use one intent-level workflow:

```sh
lenso new greeting
cd greeting
lenso check
lenso dev
lenso verify
```

Users upgrading from the legacy nested and Plan-oriented commands should read
the [0.4 / 0.12 migration guide](docs/migration-0.4.md).

The Rust starter uses the public `lenso` facade. The Capability owner edits
`capability/src/contract.rs` and runs `lenso check --update-contracts` after a
contract change. Normal `lenso check` fails when the checked-in Descriptor,
Schemas, or Rust projection drift from that source. Its business source contains
`#[module]` and `#[provides(...)]`; Capability lowering, endpoints, the native
factory, link-time registration, and the package-owned Module Descriptor are
generated. Because the starter defines a new Capability, its locked portable
contract lives in a separate `capability` crate rather than beside Module
behavior. `check` emits fast authoring diagnostics, `dev` resolves and starts a
fresh development generation, and `verify` records behavior and removal
evidence. Descriptor, binding, Plan, and Runner stages stay behind those
commands.

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

## Plugin Release bundles

Plugin authors can turn already-built source artifacts into one immutable,
self-verifying Release directory without hand-encoding WebAssembly Components
or calculating digests:

```sh
cargo build --release --target wasm32-unknown-unknown

lenso plugin build \
  --manifest lenso-plugin.template.json \
  --artifact agent-wasm=target/wasm32-unknown-unknown/release/agent_plugin.wasm \
  --output dist/agent-plugin

lenso plugin verify --bundle dist/agent-plugin
```

For a `wasm_component` Artifact, `plugin build` converts the Rust core Wasm
module to a validated Component before recording its exact digest and size.
QuickJS, process, native library, data, and product-metadata files use the same
Bundle closure and verification rules. The command never executes publisher
code, never overwrites an existing output, and does not grant permissions,
admit a Release, or switch a running App Generation; those remain Host-owned
operations.

`dev` infers the execution class from the generated project. Native Rust
scaffolds include a development Runner; production Runner composition remains
App-owned. `verify` records behavior probes and a real removal-resolution
proof in `.lenso/module-verification.json`.

Use `--recipe stateless`, `stateful`, `web-console`, or `managed-work` to
seed the generated `MODULE.md` card.

## Scope

The CLI exposes user intent rather than its internal check, resolution, recipe,
Plan execution, and Adapter assembly stages:

```text
lenso new
lenso dev
lenso check
lenso verify
lenso app add
lenso app remove
lenso app check
lenso app resolve
lenso plugin build
lenso plugin verify
```

`app check` and `app resolve` remain explicit advanced commands for App owners
and Hosts that exchange canonical Plan bytes. They are not ordinary Module
authoring steps.

Runtime extensions, product Modules, deployment systems, and Console operations
belong to their owning repositories rather than this authoring CLI.
