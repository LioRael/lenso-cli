# Authoring code generators

Implemented in the CLI repository, independently of the main Lenso SDK and official
Registry. Requires a source build of native CLI 0.2.4 and the unpublished Code 0.2.0
extension. Existing published packages do not acquire these commands automatically.

## Author workflow

Keep a local catalog, adapters and templates in a project-relative author directory:

```text
my-project/
  author/
    catalog.toml
    adapters/
    templates/
  src/
    business.rs                 # User-owned
    main.rs                     # User-owned stable entry
    generated/                  # Owned generated outputs
    composition.json            # Existing composition/ownership artifact
```

After the new Code package is published, normal onboarding uses the existing
reviewed extension setup, with no manual configuration editing:

```sh
lenso ext add code --input '{"author_root":"author","target":"src"}'
lenso code validate
lenso code compose-preview --sample memory
lenso code compose --sample memory
```

For development before publication, build and declare the local package instead:

```sh
python3 scripts/configure-code.py /path/to/my-project --author-root author
```

The helper needs the Rust Wasm development toolchain. It configures a
`project:author` resource source with explicit reads under `author` and `src`, and
writes under `src`. It grants no repository access or native process execution.
The normal extension setup supports the same `author_root`, `catalog` and `target`
inputs. The source declaration, grants and generated code are separate reviews.

Remote authors distribute the same catalog and templates in a GitHub repository:
configure `repo`, `ref`, `catalog`, `target`, `format = "lenso-compose-v1"` through
extension setup. Host snapshot pins, source hashes, cache, credentials and locked /
offline execution apply to templates just as they do to adapter source files.
Local sources use current working-tree bytes; all observed reads are bound into the
reviewed Project plan. A local catalog digest is not a Git commit or a file lock.

## Pure generator interface

`lenso-codegen` is an optional reusable library linked into the Code Wasm extension.
The generic runtime does not parse templates or know native application contracts.

```rust
pub trait Generator {
    fn generate(&self, input: &GeneratorInput) -> Result<GeneratorOutput, Error>;
}
```

Version 1 input contains original composition intent and a resolved graph:
`schema_version`, `request`, `resolution`. The graph contains instance addresses,
module identities/versions, effective typed configuration, bindings and a
dependency-first instance order. Callers establish contract compatibility through
the composition resolver before generation.

Output contains `schema_version`, exact UTF-8 `files` with scope/module/instance
owners, and selected native `dependencies`. It is a proposal. The implementation
must not write projects or invoke installers. A custom executable extension can
reuse the Rust interface and emit its own Project-compatible proposal; catalog
metadata cannot load arbitrary generator code or invoke additional Host functions.
Custom generators need their own reviewed extension package and normal grants.

The reference `TemplateGenerator` receives a declaration and an explicitly loaded
source map. It uses [MiniJinja](https://docs.rs/minijinja/2.24.0/minijinja/), strict
undefined values and explicit JSON/Rust-literal filters. There is no filesystem or
network template loader, environment lookup, clock or process API. Template
includes/imports are not dynamically loaded. Every referenced source file must be
listed and supplied explicitly.

## Catalog declaration

Add a generator and complete named samples to a composition catalog:

```toml
[generator]
schema_version = 1
directory = "generated"
files = [
  { scope = "composition", template = "templates/app.rs.j2", target = "app.rs" },
  { scope = "instance", module = "greeter", template = "templates/greeter.rs.j2", target = "factory.rs" },
]

[samples.memory]
instances = { primary = "greeter" }
context = { language = "rust" }
bindings = { "primary.store" = { type = "module", module = "memory" } }
```

Existing module `resources` remain exact source copies. If a generator is declared,
all copied and rendered sources go below its generated directory. Without a
generator, the existing `<target>/modules/` layout remains compatible.

| Scope | Output path under `<target>/<directory>` | Owners |
| --- | --- | --- |
| Copied resource / module template | `modules/<module>/<target>` | All effective instances of that module |
| Instance template | `instances/<instance-address>/<target>` | That single instance |
| Composition template | `assembly/<target>` | Every effective instance in the composition |

A module filter applies to module or instance declarations. Omit it to render for
all selected modules/instances. Composition declarations cannot have a module
filter. Targets are static relative paths; instance addresses and source identities
supply the scope directories. Targets cannot traverse, use reserved metadata or
Windows device names, or collide by case or ancestor/descendant relationships.
Conflicts between copied files and templates are errors, not overwrite ordering.

Templates receive:

- `schema_version`, `request`, `resolution`, sorted selected `modules`, and selected
  `dependencies` in every scope.
- `module` in module and instance scope; `null` in composition scope.
- `instance` in instance scope: `id`, `symbol`, `module`, `version`, effective
  `configuration` and `bindings`; `null` in other scopes.

Use `resolution` and `instance.configuration` for effective values/defaults.
`request` represents supplied or persisted intent and can include inactive choices.
Use the dependency-first `resolution.order` when emitting factories. Bind to exact
instance addresses; using the same module twice does not imply a singleton.

```jinja
pub fn create() -> MyType {
    MyType::new({{ instance.configuration.prefix|rust_string }})
}
```

`rust_string` emits an escaped Rust string literal, including quotes. `tojson`
serializes values for JSON/JavaScript data. `symbol` converts a logical ID to an
injective identifier consisting of `i_` followed by hexadecimal UTF-8 bytes. These
symbols remain stable across runs and distinguish dots from hyphens. Do not insert
untrusted configuration as raw source. Other languages require appropriate native
literal encoders or language-specific generators; templates are not a type checker.

Native dependency declarations can be attached to a module:

```toml
[[generator.dependencies]]
ecosystem = "cargo"
package = "some-driver"
version = "1"
module = "memory"
```

Only selected modules activate their requirements. Omit `module` for a
composition-wide requirement. Ecosystem-specific fields live in `options` and are
interpreted by the Code extension. Requirements are returned in the proposal and
saved in the composition artifact; templates can also consume them.

With explicit native target setup, Code now reconciles real Cargo/npm manifests,
tracks shared project ownership and proposes authorized installation, updates and
safe retirement. See [native dependency management](native-dependencies.md).
Without native targets, a composition selecting native declarations fails with an
actionable configuration diagnostic. Author validation can still validate them
without installing packages. The two-adapter Rust example itself needs no external
native dependencies.

## Validation and previews

`lenso code validate` is read-only with respect to application files. It:

1. Checks all module definitions, output declarations and source paths, including
   unselected modules and templates.
2. Reads every declared resource and compiles every template. Unknown modules,
   missing resources, invalid syntax and destination conflicts fail validation.
3. Requires at least one complete sample when a generator is present, resolves all
   samples, renders each twice and compares output bytes and native requirements.
4. Returns per-sample counts/digests and `uncovered_modules`. An uncovered module
   is reported; its runtime correctness is not inferred from another sample.

Validation does not execute generated code or prove native typing/semantics.
An incomplete sample is an author error with the unresolved choice in its diagnostic.
It does not ask the author to approve a file mutation. Template/schema failures use
normal structured failure output. Multiple configured profiles use `--registry`.

`compose-preview --sample NAME` uses that sample as fresh intent and returns the
normal combined Project plan. `compose --sample NAME` follows the same resolution
and reconciliation, then requests file approval. With no explicit roots/sample,
existing saved intent is reused; a fresh project can select a named sample through
the normal terminal/Agent question flow. Input maps can override sample maps under
the existing composition input contract.

JSON ABI 1, `--json`, `--answers FILE`, `--no-interactive`, confirmations and exit
statuses are unchanged. `--yes` does not choose a sample/provider/conflict answer.
The receipt records output scope and exact instance owners. Generated factory and
assembly updates use the existing baseline/current/new comparison, line merging,
conflict choices and approved stale-safe deletions. A modified obsolete factory can
be kept and released from ownership. Unrelated business files and entries are never
implicitly adopted or rewritten.

Source and output budgets are bounded: at most 127 generated resource paths plus
the composition artifact, 1 MiB sources/output budgets, and 16 author samples.
Template evaluation has its own 100,000-operation budget and bounded output writer.
Code's compose/validate handlers request 50 million Wasm fuel units. The generic
Host permits declared budgets from 1 to 50 million, defaults to 10 million, and
retains its memory, message and timeout limits. This requires native CLI 0.2.4;
older hosts cannot execute a manifest with these new handler declarations.

## Compilable two-adapter example

See [codegen-demo](../examples/codegen-demo/README.md). The business contract and
entry are handwritten Rust. Memory and file-backed stores implement the same
contract using only the standard library. Templates generate typed per-instance
factories and the application assembly; the shared sample instantiates one store
and passes cloned `Arc` handles to two independently configured greeters.

The CLI/Wasm acceptance script validates local and pinned remote catalogs, denies
and approves writes, compiles and runs all three samples, verifies store behavior,
checks adapter cleanup and no-op replay, and merges a template update with a local
factory customization. Native compile/run steps are explicit acceptance-driver
actions, not catalog code executed by author validation.
