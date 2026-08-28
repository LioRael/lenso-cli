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
lenso plugin new uppercase
cd uppercase
lenso plugin check
lenso plugin dev --operation execute \
  --request-json '{"name":"uppercase","arguments_json":"{\"text\":\"hello\"}"}'
lenso plugin pack
```

The generated Rust project keeps business behavior in `src/plugin.rs` as
ordinary typed Rust. Lenso owns the generated Wasm Component lowering, WIT,
Capability descriptor, schema projection, and wire dispatch.

By default, a Rust Plugin project has one editable `src/plugin.rs` and produces
both portable Wasm and trusted Process implementations. `pack` places both in
one V3 `.lenso-plugin` Release; the Host selects one implementation before Plan
resolution and never falls back after startup. Legacy single-output projects
remain readable.

`pack` validates and reopens the exact `.lenso-plugin` Bundle it creates. A
receiving Host independently validates those bytes again during installation.

## Change an App

The current Host supplies useful defaults and a generated Host Catalog. An App
owner writes only differences under `plugins/`:

```sh
lenso plugins list
lenso plugins add dist/uppercase.lenso-plugin
lenso plugins configure company.uppercase default --file uppercase.toml
lenso plugins disable company.uppercase default
lenso plugins enable company.uppercase default
lenso plugins remove company.uppercase default
lenso app check
lenso app show
lenso run
```

Configuration lives at `plugins/<plugin-id>/<instance>.toml`; an empty file
enables package defaults. `<instance>.disabled` is the explicit absence marker.
Installed non-embedded behavior carries one exact `plugin.lenso-plugin` Bundle
inside its Plugin directory.

The Host Catalog at `.lenso/host-catalog.json` is generated and locked to the
current Host build. It is read-only execution authority, not App intent.
`app check`, `app show`, and `run` derive the App directly; there is no Plan
file for an App owner to generate or manage.

Runtime Drivers and Execution Adapters remain separate because they implement
Host mechanics, not application behavior.
