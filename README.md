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

Ordinary Module authors start with three commands:

```sh
lenso new greeting
cd greeting
lenso dev
lenso verify
```

The Rust starter uses the public `lenso` facade. Its business source contains
`#[module]` and `#[provides(...)]`; Capability lowering, endpoints, the native
factory, link-time registration, and the package-owned Module Descriptor are
generated. Because the starter defines a new Capability, its locked portable
contract lives in a separate `capability` crate rather than beside Module
behavior. `lenso module create`, `lenso module dev`, and
`lenso module verify` remain compatible explicit forms.

## Advanced App Plan authoring

```sh
lenso add --project lenso.json \
  --key greeting \
  --package local.greeting \
  --source cargo \
  --version 0.1.0

lenso check --project lenso.json \
  --execution-class lenso.native-rust@1

lenso resolve --project lenso.json \
  --execution-class lenso.native-rust@1 \
  --output .lenso/resolved-plan.json

lenso run --plan .lenso/resolved-plan.json --root .
```

`add` edits authoring inputs. `check` validates packages, descriptors,
generated projections, schemas, and Capability bindings. `resolve` writes a
canonical immutable App Plan. `run` hosts that exact plan; it does not discover
or install Modules dynamically.

## Source-derived App Definitions

Modules authored by a derivation macro embed a package-owned Module Descriptor
in their compiled Cargo artifact. An App Definition selects packages and keyed
Instances without repeating Capability IDs, operation tables, bindings,
execution classes, or lifecycle policy:

```json
{
  "schema_version": 1,
  "manifest": "Cargo.toml",
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

`app add` delegates dependency and lock ownership to Cargo, discovers the
runtime package id from the package-owned Descriptor, chooses a useful default
Instance key, and updates the small App Definition. Use `--path` for a local
package or `--git` with `--rev`, `--branch`, or `--tag` for Git. `--dry-run`
performs the complete build and resolution check, reports touched files, and
then restores them byte-for-byte.

Every edit is transactional: dependency files and the App Definition are
restored when Descriptor discovery or composition fails. The CLI builds only
the selected locked Cargo packages, reads Descriptor bytes from their artifacts
without executing package code, derives unambiguous bindings, and writes the
same immutable Plan format consumed by the Kernel.
`one` and `optional` ambiguities require an explicit App Definition decision;
`many` providers are ordered deterministically.

## Reusable App variants

Large Apps can keep cohesive ordinary Project fragments and assemble named
variants without copying a complete project document for each combination:

```json
{
  "schema_version": 1,
  "root": "..",
  "runner": {
    "program": "cargo",
    "args": ["run", "-p", "example-app", "--"],
    "execution_classes": ["lenso.native-rust@1"]
  },
  "variants": {
    "local-coding": {
      "fragments": [
        "composition/fragments/core.json",
        "composition/fragments/model/fixture.json",
        "composition/fragments/tools/coding.json"
      ],
      "output": "composition/local-coding/resolved-plan.json"
    }
  }
}
```

```sh
lenso compose list --recipe composition/recipes.json
lenso compose check --recipe composition/recipes.json
lenso compose resolve --recipe composition/recipes.json
lenso compose run --variant local-coding
lenso compose dev --variant local-coding
```

Fragment contents use the same `composition`, `packages`, `contracts`, and
`profiles` fields as an ordinary project. A fragment may instead list
`cargo_contracts`; `compose` locates those Cargo packages and reads their
owner-local `capability.json` and generated projections, so an App does not need
to vendor a second contract copy. Paths inside fragments are relative to the
recipe root. Duplicate Module keys and bindings, conflicting package or contract
inputs, path escapes, and invalid resulting Compositions fail before Plan
output. `--variant <name> --without <fragment>` checks a focused removal without
creating a second project document.

Recipes and fragments are authoring inputs only. `compose resolve` expands one
exact ordinary Project in memory and then uses the existing validation and
resolution path; neither recipes nor fragments enter the Kernel or runtime.
`compose run` and `compose dev` resolve to an ignored
`.lenso/compose/<variant>/resolved-plan.json`, export that path as
`LENSO_RESOLVED_PLAN`, and launch the structured product-owned Runner without
shell interpretation. `compose dev` watches the recipe root, excludes ordinary
runtime output directories, and restarts the complete Runner with a fresh Plan
after a source change. Explicit command-line execution classes override the
Runner defaults. Arguments after `--` are forwarded to the product Runner.

## Module authoring

Create a self-contained Rust or Bun Module project using the explicit command
form:

```sh
lenso module create greeting --runtime rust
# or: lenso module create greeting --runtime bun
cd greeting
lenso module check
lenso module dev
lenso module verify
```

The shorter ordinary path is `lenso new greeting`, `lenso dev`, and
`lenso verify`.

`module dev` infers the execution class from `lenso.json`. Native Rust
scaffolds include a development Runner; production Runner composition remains
App-owned. `module verify` records behavior probes and a real removal-resolution
proof in `.lenso/module-verification.json`.

Use `--recipe stateless`, `stateful`, `web-console`, or `managed-work` to
seed the generated `MODULE.md` card.

## Scope

The CLI intentionally exposes only App Plan and Module authoring:

```text
lenso add
lenso new
lenso dev
lenso verify
lenso check
lenso resolve
lenso run
lenso compose list
lenso compose check
lenso compose resolve
lenso compose run
lenso compose dev
lenso module create
lenso module dev
lenso module check
lenso module verify
```

Runtime extensions, product Modules, deployment systems, and Console operations
belong to their owning repositories rather than this authoring CLI.
