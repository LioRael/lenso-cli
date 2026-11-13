# Code 0.3 target variants

Code 0.3 adds extension-owned preset defaults and target language/runtime selection.
It works with published CLI 1.0.0 / Host 0.2.4 and the existing JSON ABI 1. No
language or Auth logic is added to the generic runtime.

A functional preset can provide common setup input and complete implementation
choices through opaque Code options:

```toml
[[extensions]]
id = "code"
source = "registry:@lenso/code"
version = "^0.3.0"

[extensions.options.setup]
repo = "LioRael/lenso-modules"
ref = "v0.1.0"
format = "lenso-compose-v1"
target = "src"

[extensions.options.variants.typescript]
runtime = "node26"
[extensions.options.variants.typescript.setup]
catalog = "auth.typescript.toml"
native = { npm = { manifest = "package.json" } }

[extensions.options.variants.rust]
runtime = "tokio"
[extensions.options.variants.rust.setup]
catalog = "auth.rust.toml"
native = { cargo = { manifest = "Cargo.toml" } }
```

The preset also declares its schema/id/version and `code/ready` adoption recipe.
This is an authoring example; consumers use reviewed `lenso init --preset auth`.
Prepared TS targets need Node.js 26 and ESM/TypeScript settings. The initial Rust
consumer baseline is Rust 1.99 with a Tokio host.

Missing language produces a select question, even with `--yes`. Agents provide
`--input '{"language":"rust"}'` or answer the returned question ID. IDs bind the
variant descriptor so changed options cannot silently reuse an old answer. Source,
output and native command grants still go through the existing configuration review.

Selected defaults merge in this order: common setup, selected variant setup,
explicit user input. An existing selection supplies the language on reconfiguration.
The saved Code options retain `setup`, `variants` and `selection` (language/runtime).
Native-only reconfiguration preserves its existing source/profile behavior. A
single preset-supplied resource profile can also provide its normal source defaults.

An unsupported language or explicit incompatible runtime is an error, not a
fallback. Changing an installed language/runtime requires a new project target;
`--yes` cannot approve an implicit in-place conversion. Composition receives the
saved context and rejects contradictory language/runtime intent before generation.
For a monorepo, initialize each application directory explicitly with `--project`.
Centralized discovery of several native manifests and automatic runtime inference
are not implemented by this extension change.

Catalogs still declare the actual supported implementations and native dependency
requirements. Metadata does not provide a language implementation or prove native
compatibility. The Auth source repository compiles/runs generated TS/Rust consumers
against published SDK 0.1.1, alongside shared behavior and real HTTP tests.
