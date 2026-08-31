# lenso-cli

The CLI for authoring Plugins and changing an App through its `plugins/`
directory.

## Install

```sh
npm install -g @lenso/cli
# or
cargo install lenso-cli
```

The Cargo and npm packages use independent version lines.

## Author one Plugin

```sh
lenso plugin new company.uppercase
cd company.uppercase
lenso plugin check
lenso plugin dev --operation execute \
  --request-json '{"name":"company.uppercase","arguments_json":"{\"text\":\"hello\"}"}'
lenso plugin dev --watch --operation execute \
  --request-json '{"name":"company.uppercase","arguments_json":"{\"text\":\"hello\"}"}'
lenso plugin pack
```

The generated project is ordinary typed Rust in `src/lib.rs`. The portable SDK
owns Wasm Component lowering, WIT, Capability descriptors, schema projection,
and Process wire dispatch at compile time; target-specific generated files are
not checked into the Plugin project.

By default, one editable `src/lib.rs` produces both portable Wasm and trusted
Process implementations. `dev` builds only the fastest declared local
implementation (`Process` for a multi-output Rust project); use
`--implementation wasm|process|all` when selecting or comparing a path.
File notifications with debounce drive `--watch`, with bounded polling only as
a platform fallback. `check` and `pack` still build every declared
implementation, and `pack` places both in
one V3 `.lenso-plugin` Release; the Host selects one implementation before Plan
resolution and never falls back after startup. Legacy single-output projects
remain readable.

## Author one Web Plugin

```sh
lenso plugin new company.greetings-http --web
cd company.greetings-http
cargo test --locked
lenso plugin dev
# In another terminal, call one of the printed HTTP routes.
```

This path generates a linked native Rust Plugin with `#[lenso::plugin]` and a
typed `#[endpoint]` implementation. Its starter operations demonstrate both
`#[post]` and body-carrying `#[query]` requests, typed success responses, and
RFC 9457 Problem responses. The generated test uses `EndpointTest`, so the
Plugin is exercised without opening a socket.

`lenso plugin dev` builds a temporary native Host, mounts the generated Plugin
through Web Ingress, prints its real listener address and route table, and emits
request ID, method, path, status, and latency for each request. Add `--watch` to
rebuild and restart after source changes.

Web Plugins are linked into a Host and mounted through its `web` root slot;
they are not portable Agent Tool bundles, so the generated README does not
direct users to `lenso plugin pack`.

`pack` writes one portable `.lenso-plugin` archive, then extracts, validates,
and reopens its exact contents. `plugins add` accepts that archive and legacy
Bundle directories. A
receiving Host independently validates those bytes again during installation.
`check` and `dev` use development artifacts; `pack` is the
release-profile proof and remains the only distribution build.

New Plugins and catalog Releases use the canonical namespaced Plugin ID v1
grammar (`company.uppercase`) and exact Semantic Versions. Existing
unnamespaced projects remain readable with an explicit migration warning; see
[`docs/migration-plugin-authoring.md`](docs/migration-plugin-authoring.md).

## Change an App

The current Host supplies useful defaults and a generated Host Catalog. An App
owner writes only differences under `plugins/`:

```sh
lenso plugins list
lenso plugins add dist/company.uppercase-0.1.0.lenso-plugin
lenso plugins search uppercase
lenso plugins install company.uppercase --version 1.2.3
lenso plugins update company.uppercase --version 1.3.0
lenso plugins history company.uppercase
lenso plugins rollback company.uppercase --version 1.2.3
lenso plugins configure company.uppercase default --file uppercase.toml
lenso plugins disable company.uppercase default
lenso plugins enable company.uppercase default
lenso plugins remove company.uppercase default
lenso app check
lenso app show
lenso sessions list
lenso run
```

Configuration lives at `plugins/<plugin-id>/<instance>.toml`; an empty file
enables package defaults. `<instance>.disabled` is the explicit absence marker.
Optional structured files live beside it under
`plugins/<plugin-id>/<instance>/`; `app check` validates the bounded regular-file
tree before the Host snapshots it into a Generation.
Installed non-embedded behavior carries one exact `plugin.lenso-plugin` Bundle
inside its Plugin directory.

Catalog installation always requires an exact version. Downloaded archive and
manifest digests are checked before candidate resolution; admitted archives are
retained under `.lenso/plugin-store/` so update and rollback never depend on a
mutable remote. There is no implicit latest-version selection or runtime
fallback.

The Host Catalog at `.lenso/host-catalog.json` is generated and locked to the
current Host build. It is read-only execution authority, not App intent.
`app check`, `app show`, and `run` derive the App directly; there is no Plan
file for an App owner to generate or manage.

Runtime Drivers and Execution Adapters remain separate because they implement
Host mechanics, not application behavior.

### App commands

The CLI keeps its authoring and maintenance roots static: `plugin`, `plugins`,
`app`, `run`, and `doctor`. Any other root command is validated against the
current App and forwarded unchanged to `.lenso/host`. The Host's selected
`lenso.terminal.command` aggregate and `lenso.terminal.cli` surface own dynamic
catalog discovery, help, argument parsing, execution, and the Generation lease.

For example, a Host with a selected Session command provider may expose
`lenso sessions list` and `lenso sessions show --help`. Removing that provider
removes those paths without changing this CLI. Static maintenance roots remain
reserved and cannot be shadowed by App commands.
