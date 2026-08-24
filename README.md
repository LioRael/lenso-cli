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

## App Plan authoring

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

Create a self-contained Rust or Bun Module project:

```sh
lenso module create greeting --runtime rust
# or: lenso module create greeting --runtime bun
cd greeting
lenso module check
lenso module dev
lenso module verify
```

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
