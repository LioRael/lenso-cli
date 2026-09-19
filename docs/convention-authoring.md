# Extensible file conventions

Create a CLI App with Bun and the prebuilt Lenso executable; Rust is unnecessary
unless you select Rust source Plugins:

```sh
lenso app create my-app --cli
cd my-app
lenso app dev -- hello --name Ada
lenso app build
lenso app start --from dist -- hello --name Ada
```

The scaffold installs local CLI support and creates `app/local.hello/cli.ts`:

```ts
import { command } from '@lenso/cli';
export default command({
  name: 'hello',
  description: 'Say hello',
  args: { name: { type: 'string', default: 'world' } },
  run({ args, output }) { output.text(`Hello, ${args.name}!`); },
});
```

`app dev -- ...` reruns the command after each successful rebuild. Failed builds
remain watched for corrections. `app start` returns nonzero for invalid arguments
or failed commands and shuts down the Host. Ctrl-C cancels command execution.
The distribution includes Bun, selected bundles, codecs, and its Host; running it
needs neither source trees, package managers, Cargo, nor an installed Bun.

An existing App can use `lenso app add @lenso/cli`. This installs support bundled
with Lenso into `app/lenso-terminal-cli`; it is not a marketplace lookup or an
npm installation of the executable wrapper with the same name. Adopt a custom
support source with `lenso app add ../my-support-plugin`. Shared local sources
use the existing optional `plugin_sources` setting and explicit Plugin Root
intent. Ordinary App-owned projects require no new configuration file.

## Packages and languages

```sh
lenso app plugin new example.tasks --language ts
lenso app plugin new example.native --language rust
```

A simple Plugin has one `package.json` or `Cargo.toml`. CLI scaffolding adds its
entry beside the ordinary core Plugin source. Rust `cli.rs` uses the helper SDK
available to its selected generated contribution:

```rust
use lenso_cli_support::command;

/// Say hello from Rust
#[command(name = "hello-rust")]
async fn hello(
    #[arg(long, default = "world")] name: String,
) -> anyhow::Result<String> {
    Ok(format!("Hello, {name}!"))
}
```

One `#[command]` function per entry generates the `command()` factory used by the
compiler. Keep the function's own name; do not also declare a `command()` factory
in that file. Without `name`, the function name becomes the command name with
underscores converted to hyphens. Doc comments supply help text. The macro
supports synchronous and asynchronous functions. Arguments use `FromStr` for
typed conversion; `Option<T>` is optional, `bool` is a flag, and other arguments
are required unless `#[arg(default = "...")]` supplies a fallback. `#[arg(long)]`
is optional, and `#[arg(long = "display-name")]` renames an option. Returns may
be text, `()`, or a `Result` of either. Invalid values and business errors retain
the existing terminal domain errors.

For progressive output, inject `#[context] output: CommandContext` and call
`output.text(...)` or `output.error(...)`. Output is bounded to 256 queued messages
and 16 MiB per invocation. Async functions are polled by the Stream; cancellation
drops their future. Synchronous work must return cooperatively. The macro crate
lives in the selected Rust contribution's dependency closure; unused Rust entries
do not add a Rust toolchain requirement to TypeScript Apps.

`Command::new(...).string_arg(...).run(...)` remains available for programmatic
commands. Advanced Plugins can implement the generated terminal Provider directly.
This preserves the lower-level SDKs, dependency bindings, and Stream lifecycle.
Standalone `plugin dev` invokes Request operations; exercise terminal Stream
commands through `app dev -- ...`. Generated Rust entries see the CLI helper SDK;
use an explicit independent surface package when the entry needs additional Cargo
dependencies or a shared business library.

For an App-local command without a core Plugin, put `cli.ts` or `cli.rs` in a bare
directory such as `app/status/`. No manifest is required there. The directory
supplies the default TypeScript command name; Rust macros default to the function
name. An explicit command name overrides either default. Synthetic owner
and contribution identities are stable hashes of relative paths. Renaming paths
changes these identities. Explicit package identities remain preferable for
reusable products. A plain shared directory must be packaged to carry an identity.

A logical Plugin may have multiple independently buildable packages when it needs
optional dependencies. Use `surfaces` metadata, or the optional composite
`plugin.json` described in [local discovery](local-plugin-discovery.md). An
unselected surface package is not parsed, installed, compiled, or bundled by this
workflow. Dependencies alreadyru imported by the core or included in its workspace
cannot be removed by convention selection. Shared source text itself is cheap;
independent package boundaries isolate heavyweight runtime dependencies.

## Add another convention

An ordinary support Plugin declares this in its existing Lenso metadata:

```json
{
  "conventions": [{
    "id": "example.tui",
    "entries": ["tui.rs", "tui.ts"],
    "compiler": {"program": "bun", "args": ["compile.mjs"]}
  }]
}
```

The compiler is a trusted local build tool, executed only when its support Plugin
is adopted and an active owner has a matching file. Discovery and `app inspect
--json` never execute it. Unknown files do nothing. Known optional entries without
adopted support report `support_not_adopted`; a required entry fails. Conflicting
recognizers fail deterministically. There is no hardcoded TUI filename list.

The compiler runs in its support package directory without a shell. It reads one
JSON value from stdin:

```json
{
  "schema": "lenso.convention-compile.v1",
  "entry": "/absolute/product/tui.ts",
  "owner_project": "/absolute/product",
  "plugin_id": "example.product.surface-<assigned-suffix>",
  "release_version": "1.0.0",
  "convention": "example.tui",
  "output": "/absolute/temporary-output"
}
```

Write an ordinary Cargo/Bun Plugin package or verified Bundle to `output` with
the exact assigned identity/version. Write exactly this response to stdout:

```json
{"schema":"lenso.convention-compiled.v1"}
```

Diagnostics go to stderr. Limits are 16 KiB input, 1 MiB per output stream,
60 seconds, and 4096 generated entries / 16 MiB / 32 directory levels. Generated
symlinks and special files are rejected. These validation limits are not a
sandbox for compiler side effects. Recursive convention activation is rejected.
Selected source outputs use the normal package managers, contract generation,
Bundle validation, and Host admission. Selection and source freshness are checked
before publishing an output. Generated package-manager lockfiles belong to build
staging; use a checked-in independent package or verified Bundle when requiring
an externally pinned dependency closure across separate builds.

Each generated contribution is an ordinary separate runtime Plugin. It does not
inherit permissions, configuration, or arbitrary bindings from its owner. The
current composition profile requires one active owner instance. Owner/support
selection is a build-time operation; changing it requires rebuilding. Distribution
Plugin Root edits do not rerun compilers. `.lenso/conventions.json` records the
selected ownership and support provenance.

Bundled CLI support uses the terminal-owned catalog Request and execute Stream
contracts. The precompiled Host includes their generated codecs. A third-party
convention can emit any already-supported Plugin format, but a new typed native
Host ingress or previously unsupported Stream contract still needs the ordinary
Host/codec extension path. Filename recognition alone cannot grant a new Host
integration or permission.
