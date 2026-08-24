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
lenso module create
lenso module dev
lenso module check
lenso module verify
```

Runtime extensions, product Modules, deployment systems, and Console operations
belong to their owning repositories rather than this authoring CLI.
