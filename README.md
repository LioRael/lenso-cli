# lenso-cli

The authoring CLI for Lenso Plugins and App intent.

## Install

```sh
npm install -g @lenso/cli
# or
cargo install lenso-cli
```

The Cargo and npm packages use independent version lines.

## Create a Plugin

Application behavior uses one Plugin workflow from source creation through
immutable packaging:

```sh
lenso plugin new uppercase
cd uppercase
lenso plugin check
lenso plugin dev --operation execute \
  --request-json '{"name":"uppercase","arguments_json":"{\"text\":\"hello\"}"}'
lenso plugin pack
```

The generated Rust/Wasm project contains one Plugin ID and version, one source
declaration, and the Agent Harness Tool contract. Authors do not write a second
implementation identity, Manifest template, contribution array, digest,
execution class, trust level, or execution Plan.

`pack` builds and reopens the exact `.lenso-plugin` directory it writes. The
receiving Host validates the bytes again when the Plugin is added, so there is
no normal `plugin verify` command.

The first public shape is a request-style Rust-authored Wasm Tool Plugin. Other
execution targets must join this same Plugin workflow rather than introducing a
second application-behavior abstraction.

## App intent

App owners can check and resolve the current source definition:

```sh
lenso app check --definition lenso.app.json
lenso app resolve --definition lenso.app.json \
  --output .lenso/resolved-plan.json
```

The current App Definition schema still contains compatibility-era internal
field names. They are not the target authoring model and will migrate to Plugin
selection after embedded Plugin authoring is available.

## Compatibility

Retired authoring aliases remain hidden for a bounded transition period and
print migration guidance. See the
[authoring migration guide](docs/migration-plugin-authoring.md) when updating
an older project.

Runtime Drivers and Execution Adapters remain separate because they implement
Host mechanics rather than application behavior.
