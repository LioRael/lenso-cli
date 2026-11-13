# Lenso CLI

A native CLI that loads extension commands and runs reviewed workflows. Use the
`lenso` executable in your projects, or embed the Rust engine in your own CLI.

Extensions provide behavior such as code distribution, generation and shadcn
integration. Lenso handles dependency resolution, Wasm execution, permissions,
previews and user input. The same workflows support interactive terminals and
structured JSON for Agents and CI.

## Install

Prebuilt binaries are available for macOS Apple Silicon and Intel, Linux ARM64 and
x64, and Windows x64. Installation requires no Node.js, Git or Rust.

**Homebrew (macOS / Linux)**

```sh
brew install LioRael/tap/lenso
```

Upgrade with `brew update && brew upgrade LioRael/tap/lenso`.

**Native installer (macOS / Linux)**

```sh
curl -fsSL https://github.com/LioRael/lenso-cli/releases/latest/download/install.sh -o install.sh
sh install.sh
export PATH="$HOME/.local/bin:$PATH"
lenso --version
```

**Windows PowerShell**

```powershell
Invoke-WebRequest https://github.com/LioRael/lenso-cli/releases/latest/download/install.ps1 -OutFile install.ps1
& .\install.ps1
$env:PATH = "$env:LOCALAPPDATA\Lenso\bin;$env:PATH"
lenso --version
```

**Cargo**

```sh
cargo install lenso --version 1.1.2 --locked
```

The native installers verify SHA-256 and the executable version before installing.
Use `--force` on Unix or `-Force` on Windows to replace an existing installation.
Native installations can subsequently run `lenso self update` or
`lenso self update --check`. Older/manual installations require `--adopt` once;
Cargo and Homebrew installations use their package manager to upgrade.
macOS requires 13.0 or newer; Linux requires glibc 2.35 or newer. See
[installation](docs/install.md) for manual downloads, custom destinations and
platform details, or browse the [latest Release](https://github.com/LioRael/lenso-cli/releases/latest).

## Start with a project

Create an empty project, then add the extensions you need:

```sh
lenso new my-app --preset empty
cd my-app
lenso ext add code
lenso ext tree
```

The Code extension asks which GitHub repository and catalog to use. Review its
configuration and requested permissions, then browse that catalog:

```sh
lenso code search
lenso code preview ITEM
lenso code add ITEM
lenso code diff ITEM
lenso code update ITEM
```

Replace `ITEM` with a name from your catalog. Preview shows the proposed file diff;
applying changes requires approval. Fetching and running extension packages needs
no Node.js or Git. Workflows that build an application or install its native
packages require that project's toolchain and separate process authorization.

For an existing project, use `lenso init --preset empty`, followed by `lenso ext add`.
Onboarding writes the configuration for you; editing TOML is a secondary option.

### Use a preset

Presets select extensions and can provide a project creation or adoption recipe.
The official shadcn preset creates a Vite/React/TypeScript/Tailwind project:

```sh
lenso new my-web-app --preset shadcn
cd my-web-app
lenso shadcn preview button
lenso shadcn add button
```

In a prepared frontend project, use `lenso init --preset shadcn`. The extension
supports component search, updates, differences, custom Registry URLs, namespaces,
authentication and monorepo routing. See [shadcn usage and scope](docs/shadcn.md).

### Manage extensions

```sh
lenso ext add code                      # Official package name
lenso ext configure code                # Review installed extension configuration
lenso ext tree                          # Inspect resolved dependencies
lenso update                            # Refresh extension version pins
lenso ext remove code                   # Remove a direct selection
lenso preset apply NAME_OR_URL          # Apply a preset to this project
```

Official short names resolve through [Lenso Registry](https://github.com/LioRael/lenso-registry).
Custom namespaces use explicit project routes; direct GitHub Release sources also work:

```sh
lenso ext add @acme/tool --constraint '^1'
lenso ext add github-release:acme/lenso-tools/tool --id tool
```

The first command requires an `@acme` route in the project's configuration.
Extensions can declare transitive dependencies with semantic version constraints.
Resolution supports shared providers, backtracking and cycle/conflict diagnostics,
with one version per provider per project. Removing a selection preserves packages
still required by another extension or preset. Dependency edges grant no permissions.

The Project provider is optional. File-changing workflows can depend on it; other
commands can run without it. Extensions are selected per project, with no global
extension installation.

## Configuration, locks and cache

| Location | Purpose |
| --- | --- |
| `lenso.toml` | Editable extension selections, options, Registry routes and permissions |
| `lenso.lock` | Resolved dependency graph, manifests, versions and source/module hashes |
| User cache | Verified metadata and Wasm bytes shared across projects |

Commit `lenso.toml` and `lenso.lock`. Dependencies prepare automatically on first
execution, and Wasm downloads when a handler needs it. Later invocations reuse
locked metadata and cached bytes. There is no `install` prerequisite.

Changed declarations resolve automatically while retaining unchanged pins.
`lenso update` explicitly refreshes dependencies. `lenso code update ITEM` instead
updates a code resource through the Code extension.

- `--project DIRECTORY` selects a project explicitly; otherwise Lenso finds the
  nearest ancestor containing `lenso.toml`.
- `--locked` requires an existing lock matching the declarations.
- `--offline` prepares dependencies using only local sources and cached data.
- `lenso cache path` prints the shared cache location. An absolute
  `LENSO_CACHE_DIR` overrides it.

Cache defaults are `~/Library/Caches/lenso` on macOS, `$XDG_CACHE_HOME/lenso`
(or `~/.cache/lenso`) on Linux, and `%LOCALAPPDATA%/Lenso/Cache` on Windows.
Cached bytes are content-addressed and checked against pinned hashes. Cache stores
no project permissions or answers. Deleting it does not change locked versions.

See [configuration and Host protocol](docs/protocol.md) and
[extension resolution](docs/registry.md) for declaration formats and lock behavior.

## Interactive and Agent workflows

Terminals provide searchable choices, text inputs, confirmations, progress and
result summaries. File-changing workflows show a diff before approval. External
commands require their own grants and confirmation.

```sh
lenso code preview ITEM --json
lenso code add ITEM --json --answers answers.json
lenso code add ITEM --json --yes
```

`--json` disables prompts, progress and terminal colors. Missing required answers
return `needs_input` with exit code `2`; supply answers keyed by the returned
question IDs and invoke the command again. `--input '{...}'` supplies workflow
input directly. `--yes` answers confirmation questions only: it neither selects
options nor grants new permissions.

`--no-interactive` disables terminal prompts. Human output supports
`--color auto|always|never`, respects `NO_COLOR` in auto mode, and includes the
complete report on stderr with `--debug`. Both modes run the same workflows and
authorization checks.
See [the protocol](docs/protocol.md) for result envelopes, questions and Host APIs.

## Publish an extension

Authors distribute prebuilt Wasm packages through GitHub Releases. Lenso packages
the declared files, validates identity and hashes, uploads a draft, verifies the
assets and publishes after approval:

```sh
lenso --project AUTHOR_PROJECT publish --github \
  --repo acme/lenso-tools --id tool --dry-run

lenso --project AUTHOR_PROJECT publish --github \
  --repo acme/lenso-tools --id tool --target FULL_COMMIT_SHA
```

Authors build Wasm before publishing. Consumers download prebuilt packages through
GitHub's HTTP APIs, with no npm or Git checkout involved. Code resources can live
in a GitHub repository catalog and resolve to pinned commit snapshots.

The extension Registry maps names and versions to packages. Resource catalogs and
business-specific formats belong to extensions. Authors can configure release tags,
asset names and resource layouts. See [GitHub distribution and authoring](docs/github-distribution.md)
for the package format, authentication and GitHub Actions publishing.

## Code generation

Code 0.3.2 is available as a [GitHub Release](https://github.com/LioRael/lenso-cli/releases/tag/code-v0.3.2)
and runs on CLI 1.0.0. Both `lenso ext add code` and the official `code` preset
resolve Code 0.3.2. It provides:

- Capability-based composition with independently selected adapters and explicit
  instance bindings, rather than a preset for every possible combination.
- Target language/runtime choices supplied by functional presets, with explicit
  Agent input and protected installed selections.
- Strict templates, isolated generated directories and named author samples.
- Generated-file ownership, incremental updates, three-way merging and explicit
  conflict handling. Switching adapters proposes cleanup of obsolete outputs.
- Cargo/npm manifest reconciliation, shared dependency ownership and separately
  authorized installation plans.

A local example generates a Rust application with memory and file-backed adapters:

```sh
cp -R examples/codegen-demo /tmp/lenso-codegen-demo
cd /tmp/lenso-codegen-demo
lenso init --preset empty
lenso ext add code --input '{"author_root":"author","target":"src"}'
lenso code validate
lenso code compose-preview --sample memory
lenso code compose --sample memory
cargo run
```

This uses prebuilt CLI and extension packages; Cargo is needed to compile the
generated example. See [the two-adapter example](examples/codegen-demo/README.md),
[generator authoring](docs/code-generation.md), [native dependencies](docs/native-dependencies.md)
and [composition design](docs/module-composition.md). The
[official module delivery plan](docs/official-modules-plan.md) builds on the
published TypeScript/Rust SDKs. The first [Auth module](https://github.com/LioRael/lenso-modules)
provides TypeScript/Node.js and Rust/Tokio implementations with SQLite and scrypt.
In a prepared native application, run `lenso init --preset code`, then
`lenso code preview auth` and `lenso code add auth`. Code resolves its own module
directory and asks for project language/runtime and adapter bindings. See [target variants](docs/code-target-variants.md)
for selection, native project requirements and current boundaries.

## Embed the Rust engine

The published `lenso-cli` crate provides command registration, async handlers,
workflow steps, hooks and resumable questions. A minimal native CLI needs no
project configuration or Wasm runtime:

```toml
[dependencies]
lenso-cli = "1.0.0"
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

```rust,ignore
use lenso_cli::{Cli, CommandSpec, Error, Outcome, serde_json::json};

#[tokio::main]
async fn main() -> Result<(), Error> {
    let mut cli = Cli::new("acme");
    cli.command(CommandSpec {
        path: vec!["hello".into()],
        workflow: "acme/hello".into(),
        about: "Say hello".into(),
        provider: "acme".into(),
        arguments: vec![],
    }, |_context| async {
        Ok(Outcome::complete(json!({"message": "Hello!"})))
    })?;
    std::process::exit(cli.run().await);
}
```

See [Acme CLI](examples/acme-cli/src/main.rs) for questions and optional extension
loading. Native handlers are trusted application code.

| Package | Responsibility |
| --- | --- |
| [`lenso`](https://crates.io/crates/lenso) | Standalone executable, command name `lenso` |
| [`lenso-cli`](https://crates.io/crates/lenso-cli) | Embeddable command and workflow engine |
| [`lenso-runtime`](https://crates.io/crates/lenso-runtime) | Generic Host, Wasm execution, distribution, locks and cache |
| [`lenso-project`](https://crates.io/crates/lenso-project) | Optional file plans, previews and reviewed application |
| [`lenso-composition`](https://crates.io/crates/lenso-composition) | Pure capability and instance resolution |
| [`lenso-codegen`](https://crates.io/crates/lenso-codegen) | Pure generation contracts and template rendering |

**Migrating from the previous implementation:** `lenso` and `lenso-cli` 1.0 move
these crate names from the main Lenso repository to this standalone CLI project.
This is a breaking product/API migration. Existing SDK consumers should keep their
0.x constraints. Host versions are independent: CLI 1.0.0 uses Host 0.2.4 and JSON
ABI 1. See [crate releases and migration](docs/crates-release.md). The previous CLI
repository is preserved at [lenso-cli-previous](https://github.com/LioRael/lenso-cli-previous).

## Develop

The Rust workspace requires Rust 1.99 or newer. Core validation:

```sh
cargo fmt --all -- --check
cargo check --locked --workspace
cargo test --locked --workspace
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
```

Extension development also needs `wasm32-unknown-unknown` and Python 3. Frontend
fixtures require their own Node.js/package-manager toolchain. The
[CI workflow](.github/workflows/check.yml) builds both Wasm extensions and exercises
onboarding; focused acceptance scripts live under [`scripts/`](scripts/).

For release maintenance, see [native binary releases](docs/install.md#publish-the-cli),
[crate publishing](docs/crates-release.md) and the
[native release workflow](.github/workflows/release-cli.yml).
