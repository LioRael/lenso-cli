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
`lenso app resolve` may export a derived Plan for diagnostics, but that output
is never read back as authoring input.

Runtime Drivers and Execution Adapters remain separate because they implement
Host mechanics, not application behavior.
