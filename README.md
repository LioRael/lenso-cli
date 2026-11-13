# Lenso CLI

An embeddable Rust CLI framework and a standalone `lenso` executable. Commands come
from native handlers or explicitly declared extension manifests. Project changes
are an optional extension; running a command does not require a file plan.

Source: [LioRael/lenso-cli](https://github.com/LioRael/lenso-cli).
The previous CLI implementation is preserved in
[LioRael/lenso-cli-previous](https://github.com/LioRael/lenso-cli-previous).

Native binaries and SHA-256-verified installers are distributed through
[GitHub Releases](https://github.com/LioRael/lenso-cli/releases/latest) for macOS
(Intel/Apple Silicon), Linux (x64/ARM64) and Windows x64. See
[installation and release instructions](docs/install.md), including private-repository
downloads. Neither installation nor the native CLI requires Node.js, Git or Rust.

The execution framework owns workflows, questions, hooks, and JSON/Agent output.
The runtime owns restricted Host operations, extension distribution, integrity,
locking, and the user cache. Business protocols live under `extensions/`; the
runtime does not parse component catalogs, npm metadata, or workspace formats.

## Start a project

The 0.2 CLI provides native onboarding; editing TOML is optional:

```sh
lenso new my-app --preset shadcn
cd my-app
lenso shadcn add button

# Existing projects
lenso init --preset shadcn
lenso ext add code
lenso code search
lenso ext tree
```

`new` creates a Vite/React/TypeScript/Tailwind project through the shadcn preset.
`init` adopts an existing prepared project. The Code extension asks for its GitHub
repository; `--input '{"repo":"owner/repo","ref":"v1","catalog":"catalog.toml","target":"src"}'`
provides the same settings to Agents. Configurations and proposed grants are reviewed
before saving; application changes have a separate file preview and confirmation.
Frontend development and approved package-manager operations still require the
project's own toolchain. Fetching and running Lenso extensions requires no Node.js or Git.

Official aliases resolve through [lenso-registry](https://github.com/LioRael/lenso-registry).
`lenso ext add @acme/tool --constraint '^1'` uses an explicitly configured namespace;
`lenso ext add github-release:owner/repo/tool --id tool` uses a direct source.
`lenso preset apply NAME_OR_URL` composes packages and runs its adoption recipe.
Private packages and authenticated API quota use explicit `--token-env NAME`;
`--api URL` selects a compatible GitHub API. Credentials remain in the environment,
and only their binding names are saved.
`lenso ext remove ID` and `lenso preset remove ID` retain packages still selected by
another preset or required by another extension. They leave generated code and the
shared cache in place. A new project without extensions uses `--preset empty`.

These commands require CLI 0.2 or a current source build; the earlier 0.1 binary
continues to provide its original command set.

## Configuration and dependencies

The editable project configuration lives in root `lenso.toml`. Commands prepare
their dependencies automatically; no `install` command is required. Commit the
root declaration and generated `lenso.lock`. Keep local source packages available
when using local declarations.

```toml
[cli]
schema_version = 1

[[cli.extensions]]
id = "demo"
source = "lenso/extensions/greeting/extension.toml"
files = ["greeting.txt"]
```

Sources can be local paths, explicit HTTPS manifests, or package references:

```toml
[cli]
schema_version = 1
registries = { "@acme" = "https://packages.example.com/index/" }

[[cli.extensions]]
id = "code"
source = "registry:@acme/code"
version = "^1.2"
```

A Registry reference selects the highest compatible non-yanked release from the
configured static index. Manifest identity/version and content digests are checked
before registration. Direct-source version requirements remain validation checks.
Remote manifests must pin exact Wasm digests; local sources may use `auto`.
See [extension distribution](docs/registry.md) for the protocol, lock semantics,
limits, and command-level acceptance. The official `@lenso` namespace is built in.
GitHub Releases and repository code sources are also supported. See
[GitHub distribution](docs/github-distribution.md) for author configuration,
packaging, source grants, lock/cache behavior, and the optional `code` extension.
The [design record](docs/distribution-design.md) distinguishes later goals such as
repository-hosted executable packages and installed-file receipts.

Extensions declare `dependencies` with provider IDs, sources, and semantic version
constraints. Resolution supports transitive dependencies, shared providers, version
backtracking, cycles/conflicts, and a complete locked graph. Each provider has one
version per project. Dependencies receive no inherited grants. The optional native
`project` provider is activated only when selected or required; a minimal executable
can omit it with `cargo build -p lenso --no-default-features`.
When upgrading an existing 0.1 project, run `lenso update` to obtain packages with
explicit dependencies. For an intentionally pinned older package, select its native
provider explicitly with `lenso ext add project`.

The first command resolves metadata, validates definitions, and saves the lock.
Each Wasm file is downloaded or copied into the user cache only when a handler
needs it. Subsequent commands use locked metadata and verified cached bytes.
Changed declarations are resolved automatically, preserving unchanged pins;
`lenso update` explicitly refreshes sources. Source-file edits become active after
that update. Permissions stay in project TOML and are never shared through cache.

`--locked` requires an existing lock matching the declarations. `--offline` disables
network access during dependency preparation; local sources and cached metadata/
modules remain usable. This flag does not sandbox explicitly approved child processes
or replace a workflow's own network policy. `--json` keeps the same result protocol
during both cold and warm execution.

Help uses locked metadata without touching the cache. Without a lock, help resolves
only metadata and writes no project lock; local `auto` digests require reading the
local source bytes. Remote Wasm is not downloaded by help. `--version` does not load
project dependencies.

## GitHub packages and code resources

```sh
# Author: inspect, then publish an already built extension.
lenso --project AUTHOR_PROJECT publish --github --repo acme/lenso-tools --id code --dry-run
lenso --project AUTHOR_PROJECT publish --github --repo acme/lenso-tools --id code --target FULL_COMMIT_SHA

# Consumer: add the optional Code extension and answer its repository question.
lenso --project PROJECT ext add code
lenso --project PROJECT code search
lenso --project PROJECT code preview button
lenso --project PROJECT code add button
lenso --project PROJECT code update button
```

Release extensions use `source = "github-release:owner/repo[/package]"` with a
semantic version requirement. Code catalogs use repository snapshots selected by
an extension's opaque options and separate `sources = ["github:owner/repo"]` grants.
Node.js and Git are not needed for either consumer path. Repository source pins live
beside extension pins in `lenso.lock`; immutable bytes reuse the user cache. Read-only
commands may prepare these metadata pins, while file application still requires the
existing project grants and review. Official packages use curated Registry entries; custom repositories remain supported.

Publishing validates deterministic packages, asks for approval, uploads to a draft,
verifies checksums and then publishes. Reruns resume matching draft assets and accept
identical published releases without mutation. `--draft` keeps a verified draft;
`--json`, `--answers` and `--yes` use the existing Agent protocol. See the
[author guide](docs/github-distribution.md#publish-from-a-terminal-or-agent) for token
configuration, discovery prerequisites and the included GitHub Actions workflow.
See [live verification](docs/github-release-verification.md) for the real Release
uploads, successful Actions run and Linux/macOS consumer results.

## Shared cache

Downloaded metadata and Wasm use content-addressed blobs under the per-user cache:

- macOS: `~/Library/Caches/lenso`
- Linux: `$XDG_CACHE_HOME/lenso`, or `~/.cache/lenso`
- Windows: `%LOCALAPPDATA%/Lenso/Cache`

`lenso cache path` prints the active location. Set an absolute `LENSO_CACHE_DIR` to
override it, including in CI. The `v1/blobs/sha256/` store is shared across projects;
per-entry OS locks and atomic replacement coordinate simultaneous downloads. Cached
bytes are checked against project pins. Missing/corrupt blobs are restored from the
locked source; deleting cache never changes dependency versions. Cache does not
contain project permissions, answers, reviewed file plans, or `node_modules`.
Wasm compilation is still performed by Extism at invocation time.

The old project `.lenso/cache/` is no longer used. `lenso migrate --project PATH`
moves the experimental `.lenso/project.json` layout into visible sources and root
TOML/lock, preserving permissions and unrelated local files. An existing root
configuration requires a manual merge. Without `--project`, the nearest ancestor
containing `lenso.toml` is selected.

## Terminal and Agent output

Interactive terminals show progress, completed steps, changed paths, external
commands, and a compact result summary. `inquire` handles text, searchable select,
multiselect, and confirmation prompts; `indicatif` handles progress; `console`
styles results and diffs. Clap handles command parsing and help.

- `--color auto|always|never` controls colors. Auto respects `NO_COLOR` and
  nonterminal output; explicit `always` overrides `NO_COLOR` in human mode.
- `--debug` includes the complete result report on stderr in human mode.
- `--no-interactive`, CI, and nonterminal input disable questions and progress
  animations. Missing answers exit 2; they are never chosen implicitly.
- `--json` always disables prompts, progress, and host color codes. The existing
  result/diagnostic schema stays intact; child process logs stay on stderr.

## Other engine examples

```sh
python3 scripts/build-demo.py
cargo run -p lenso -- --project examples/m0 greet
cargo run -p lenso -- --project examples/native-dev dev
cargo run -p acme-example -- hello --name Leo --json
cargo run -p acme-example -- --project examples/native-dev toolchain version --json
```

Without a declared preset, `lenso --help` exposes only generic engine commands.
The native process example and branded Acme CLI do not need a project file plan.

## Embed it

The framework package is `lenso-cli`; the standalone binary package is `lenso`.
No crate has been published. Use a local path dependency while developing.

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

See [the Acme example](examples/acme-cli/src/main.rs) for a native question and
project extension discovery. Native Rust handlers are trusted application code;
they are not sandboxed.

## What is implemented

- Dynamic command paths, string/boolean/list arguments, help with providers, namespaced `run`, and
  explicit alias conflict handling. Installations can disable or rename aliases.
- Async native handlers, validated step dependency graphs, prepare/before/validate/
  after hooks, resumable questions, and dependent-state invalidation.
- Text, select, multiselect, and confirmation questions shared by terminal and JSON modes.
- Local Extism modules pinned by SHA-256, JSON ABI v1, fuel/time/memory/input/output
  limits, no WASI, and explicit host APIs for scoped file reads, Registry credentials,
  workspace discovery, and HTTP URL prefixes.
- Exact process grants, shell-free invocation, stderr logs, nonzero exit propagation,
  and Ctrl-C supervision. Unix cancellation kills the process group.
- Exact file destination grants checked before reading preimages or applying changes;
  calling the optional project extension does not bypass the installation's grants.
- Optional file replacement plans, unified diffs, content-bound approval, preimage
  checks, read-input guards, source/module provenance, staging, and atomic replacement
  per file. Reviewed dependency commands require their own grant and confirmation.

The **shadcn extension** consumes official names, custom Registry URLs, namespaces,
and local JSON resources in prepared TypeScript/Tailwind 4 projects and monorepos.
The extension runs as Wasm; Rust owns reads, credentials, HTTP, review, application,
and dependency processes. The Vite fixture remains the runnable button demo.

## Develop the shadcn extension

The local development workflow requires Rust 1.99+, the `wasm32-unknown-unknown` target, Python 3, Node.js, and npm.
The configuration script builds a local development Wasm artifact and writes the
declaration; it does not prepare the lock or cache. The included fixture pins
React 19, Vite 8, TypeScript 7, and Tailwind 4.

```sh
cargo build --workspace
python3 scripts/configure-shadcn.py examples/vite-shadcn --allow-install

# Static discovery: commands come from the declared preset.
cargo run -p lenso -- --project examples/vite-shadcn --help

# Live Registry resolution and a complete diff, without changing application files.
cargo run -p lenso -- --project examples/vite-shadcn shadcn preview button --json

# Choose from the live catalog, review files, then authorize package installation.
cargo run -p lenso -- --project examples/vite-shadcn shadcn add

# Agent/CI uses the same workflow and explicit consent.
cargo run -p lenso -- --project examples/vite-shadcn shadcn add button --yes --json

# Build and open the installed React component.
cd examples/vite-shadcn
npm run build
npm run dev
```

`--allow-install` records one exact npm process grant; the CLI still requests
consent before writing or executing it. Omit that configuration flag for preview-only
use. `--yes` answers confirmation questions only; it does not grant permissions or
choose a component. An answers file such as `{"item":"button"}` works with
`--answers FILE`. Missing answers in JSON/nonterminal mode return `needs_input`
and exit 2. Terminal questions use arrow keys and type-to-filter selection. Esc revisits the
previous question; Ctrl-C cancels. File review offers a full diff before application;
external commands require a separate confirmation.

The demo is an existing npm project with `@/* -> src/*`, Vite aliases, and a
shadcn-compatible Tailwind 4 theme. The preset also routes configured workspace
aliases and package exports. `lenso new --preset shadcn` supplies the Vite starter;
`shadcn init` reviews `components.json` in an existing prepared project.
Multiple resources, custom registries, and workspaces use the same review flow:

```sh
lenso shadcn search --query button
lenso shadcn preview input dialog
lenso shadcn add @private/dashboard --cwd apps/web
lenso shadcn diff button
lenso shadcn components update button
```

See [Registry, authentication, monorepo scope and verification](docs/shadcn.md).


## Boundaries and next work

Extensions are declared as local sources, remote manifests, or Registry package references and prepared
automatically. The official Registry supplies curated names and preset selection.
Native providers come from the embedding host. Discovery reads
static locked metadata; execution checks shared cached bytes and SHA-256. Root
configuration declares file destinations, readable paths, HTTPS prefixes, and exact
process grants. Shadcn commands use the `shadcn` namespace; core `init` configures
Lenso, and core `update` refreshes extension dependencies.

The initial scheduler is sequential. It validates a static graph of at most 256
steps; dynamically added steps and persistent sessions are not implemented.
Question-yielding handlers and hooks must be safe to repeat until their step
completes. Irreversible work belongs after all interaction and validation.

File plans support UTF-8 create/replace operations, up to 128 files and 1 MiB of
content. They bind destinations, recorded read digests, source/module provenance,
and external command arguments into an approval digest. A changed destination or
read input rejects application. This is optimistic concurrency protection, not a
sandbox against hostile filesystem races. Replacements are individually atomic;
commands are not a global transaction. Failed dependency installation leaves the
already applied files and reports completed work and recovery context.

Process grants authorize exact executable strings and arguments, not an executable
signature or sandbox. Children receive PATH only and can still read tool/project
configuration. Unix process-group cancellation is covered; Windows termination has
not been tested on a Windows host. Secret-scoped answers, shell completion,
dynamic step graphs, and persistent sessions remain future work.

The shadcn preset's supported payloads and remaining limits are described in
[its compatibility scope](docs/shadcn.md). The generic engine has no dependency on
`package.json`, shadcn, Lenso Application, or Console.

## Validate

```sh
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo test --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo doc --workspace --no-deps
python3 scripts/build-demo.py
python3 scripts/test-demo.py
python3 scripts/test-dependencies.py
python3 scripts/test-dependencies.py --registry
python3 scripts/test-github.py
python3 scripts/test-publish.py
python3 scripts/test-shadcn.py
python3 scripts/test-shadcn-registry.py --pnpm
```

Design basis: [CLI architecture](https://chatgpt.com/space/page_1cf88eda8c088191b996b7a66d20bf8a)
and [Lenso product boundaries](https://chatgpt.com/space/page_8d106197e7e08191b3a2ef80dee240e7).
