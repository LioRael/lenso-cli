# TypeScript Host authoring

This is the first complete Host authoring and prepared-runtime implementation.
It builds from existing verified Plugin bundles, uses the shared Rust resolver,
and produces an offline distribution started through generated `host.js`.
It does not compile arbitrary Plugin source. The commands below describe the
coordinated source branches, not a published npm/runtime release. No Rust
application project is needed to consume prebuilt bundles.

## Declare the Host

The build-time helpers currently live in the CLI's authoring export,
`@lenso/cli/host`. This avoids a new package/repository before the standalone Host
SDK and lifecycle implementation exist. Product helpers remain in product SDKs.

```ts
// store.ts — a reference to an already packed Plugin, not a live instance.
import { pluginBundle } from "@lenso/cli/host";

export default pluginBundle("./dist/company.store-1.0.0.lenso-plugin");
```

```ts
// app.ts
import { defineHost, pluginBundle } from "@lenso/cli/host";
import store from "./store";

export default defineHost({
  id: "company.notes-app",
  plugins: [store, pluginBundle("./dist/company.notes-1.0.0.lenso-plugin")],
});
```

The ID is stable Host identity; keep it across ordinary builds. Each bare
reference produces the exact Plugin ID plus Instance key `default`. Bundle paths
are resolved relative to the source file containing `pluginBundle`, not the
launch directory. An existing bundle directory is also accepted.

Execution defaults to `bun`. Select a prebuilt Rust Process implementation with
`pluginBundle(path, { execution: "process" })`. The bundle verifier and exact
target policy select one implementation before Plan resolution. Native-only,
missing, ambiguous, or incompatible implementations are errors; the builder never
invokes Cargo or tries a fallback execution class. Authoring validation does not
prove that a future Host runtime supports a bundle's operating-system facilities.

## Build and inspect

With this CLI build installed in the project:

```sh
lenso app build --source app.ts --target aarch64-apple-darwin --out build/dev
lenso app check --root build/dev
lenso app show --root build/dev --json
```

The `--target` value is the exact target used for bundle implementation selection.
For Linux packaging input use the target the Plugin publisher actually supplies,
for example `x86_64-unknown-linux-gnu`; this is not a claim of Linux runtime
deployment support. Bun-only invocation can use `bun --bun run lenso ...` so the
npm launcher and extractor run under Bun rather than the Node shebang.

Build requires the npm CLI's shipped extractor and its pinned parser dependency.
The Cargo-only executable reports a missing compiler rather than evaluating TS
or downloading tools. The parser uses TypeScript 5.9's JS compiler API under an
explicit dependency alias; the repository keeps TypeScript 7 for compiling the
CLI itself. The supported declaration grammar below is intentionally bounded.

The output contains the generated `.lenso/host-build.json`, staged immutable
bundle archives, and their `bundles.json` provenance inventory. It contains no
Host executable, Bun distribution, business data, or credentials. `lenso run`
cannot run this authoring output yet. The inventory is not an App-owner input or
a claim that a runtime has loaded those artifacts.

Build publishes only after bundle verification, default resolution, and shared
loader validation pass. Existing output is rejected; use a fresh output directory
for a new candidate. The current working App and its Plugin Root are never
rewritten. Defaults must resolve with an empty Root in this first implementation;
configuration requiring an App-owner decision is not silently materialized.

## Prepare an offline distribution

`app prepare` closes one authoring output over explicit, precompiled target
artifacts. Until the release cohort supplies those artifacts automatically, pass
their exact paths:

```sh
lenso app prepare \
  --build build/dev \
  --target aarch64-apple-darwin \
  --runtime /absolute/path/lenso-host-runtime \
  --owner /absolute/path/lenso-process-owner \
  --resolver /absolute/path/lenso \
  --bun /absolute/path/bun \
  --notices /absolute/path/THIRD_PARTY_NOTICES.txt \
  --out dist/notes-macos-arm64
```

Preparation reopens every Plugin bundle, verifies its manifest, selected
execution class, and artifact against the Host authority, rejects unsupported
target triples, stages each selected artifact by Instance, and publishes into a
new directory atomically. Bun is required only when an
included implementation selects `lenso.bun-process@1`. Runtime, owner, Bun, and
the same-cohort resolver are regular explicit inputs; ambient executables are
never substituted.

The output contains the Host authority, bundle inventory and archives, selected
Plugin artifacts under `artifacts/`, native executables, the resolver, generated
control library, `host.js`, notices, and
`.lenso/distribution.lock.json`. The lock records the exact target and SHA-256,
size, role, and executable status of every immutable file. It deliberately does
not contain a Plugin Root, secrets, business data, or durable control state.

The generated entrypoint verifies the complete lock before spawning anything:

```sh
./host.js --root /absolute/path/to/app
```

It also exports `start({ root, registry? })` for embedding. The default ownership
registry is a sibling of the App root, outside the replaceable root itself.
Startup passes only the lock and explicit Root to the managed Rust runtime.
The bundled resolver combines the immutable distribution Host authority with
that external Root and returns one resolved Plan plus digests binding both
inputs. It rejects an App root that contains a competing Host authority.
`lenso-host-runtime` consumes this lock, assembles Bun and native Process
Adapters, activates the resolved Generation, and recovers the same exact
Generation after a crash or clean suspension. Release automation still needs to
publish one matching target-artifact cohort before these source branches become
an installable public distribution.

## Explicit Instances and Slots

```ts
export default defineHost({
  id: "company.notes-app",
  plugins: [
    { plugin: store, instance: "source", configuration: { namespace: "source" } },
    { plugin: store, instance: "destination", configuration: { namespace: "dest" } },
    pluginBundle("./dist/company.copy-1.0.0.lenso-plugin"),
  ],
  slots: [{ id: "store", cardinality: "many" }],
  dependencies: [{
    consumer: { plugin: "company.copy" },
    requirement: "source",
    allow: [
      { plugin: "company.store", instance: "source" },
      { plugin: "company.store", instance: "destination" },
    ],
    default: { plugin: "company.store", instance: "source" },
  }],
});
```

The Slot ID must already match the Descriptor's offered root Slot. One unique
default can omit Slot policy and gets a required, non-replaceable `one` Slot.
Multiple defaults sharing a Slot require explicit `many`; canonical Instance
identity determines ordering. Duplicate identities or competing exact Releases
for one Plugin ID are errors. `dependencies` is the Host's exact authorization
for an App-selectable named requirement: `allow` is the complete provider set and
`default` is the initial App choice written by the build. The App may later switch
only within that set. A compatible provider that the Host did not list cannot be
selected. Source ordering never chooses a provider. Plugins publish stable,
consumer-local requirement identities independently of Capability identity.
This first authoring profile accepts Request Capabilities and at most 256
Instances/Slots. Stream and Event declarations are rejected before publication.

## Select named dependencies

The App owner changes the exact provider Instance for one requirement that the
Host explicitly made selectable:

```sh
lenso plugins bind company.copy source company.store \
  --consumer-instance default \
  --provider-instance source
lenso plugins bind company.copy destination company.store \
  --consumer-instance default \
  --provider-instance destination
```

For an optional requirement, explicit absence is also stable intent:

```sh
lenso plugins bind company.copy cache --absent
```

If several requirements are already ambiguous, apply them as one candidate:

```json
{
  "schema": "lenso.plugin-dependencies.v1",
  "selections": [
    {
      "consumer": { "plugin_id": "company.copy", "instance_key": "default" },
      "requirement_id": "source",
      "provider": { "plugin_id": "company.store", "instance_key": "source" }
    },
    {
      "consumer": { "plugin_id": "company.copy", "instance_key": "default" },
      "requirement_id": "destination",
      "provider": { "plugin_id": "company.store", "instance_key": "destination" }
    }
  ]
}
```

```sh
lenso plugins bind --file dependency-choices.json
```

The Host build writes its initial selectable choices to
`plugins/dependencies.json` with schema `lenso.plugin-dependencies.v1`. The first
bind on an older Root adopts the same choice contract and materializes every
currently unique required or optional single dependency before changing the
requested selection. Subsequent installs and configuration edits preserve that
file. Adding another compatible provider cannot redirect an existing choice, and
a missing, disabled, incompatible, or forbidden selected provider fails
validation instead of falling back. Startup and inspection remain read-only.
Existing Roots without the file retain legacy unique-candidate resolution until
they explicitly adopt the choice contract. Host-fixed bindings, requirements the
Host did not mark selectable, and Host-ordered `many` requirements cannot be
overridden through this command.

Without explicit `allow` rules, Hosts are closed. Existing default Instance configuration may change within
its Schema, but new Instances, root bundles (including inactive ones), replacement
Releases, and disabling required defaults are rejected. `app check/show`,
configuration proposals/publication, bundle installation, and selection mutations
load the same Host authority and admission rule. Configuration proposal digests
include the complete Host authority, so policy changes invalidate stale proposals.

Generated Host authority and legacy `.lenso/host-catalog.json` are mutually
exclusive. Invalid generated metadata fails closed, with no fallback to a legacy
Catalog. This is one generated Host authority, not a second editable App manifest.

## Admit extensions and constrain configuration

```ts
export default defineHost({
  id: "company.notes-app",
  plugins: [],
  slots: [{
    id: "store",
    cardinality: "many",
    maxInstances: 2,
    allow: [pluginBundle("./dist/company.store-1.0.0.lenso-plugin")],
    configurationSchema: {
      type: "object",
      properties: {
        limit: { type: "integer", maximum: 8 },
        network: {
          type: "object",
          properties: {
            hosts: { type: "array", items: { enum: ["api.example.com"] } },
          },
        },
      },
    },
  }],
});
```

The example assumes that the admitted Store's own configuration contract defines
`limit` and `network.hosts`. Product SDKs can supply these typed policies. Generic
authoring does not interpret them as OS facilities or grant access itself.

`allow` verifies and locks each exact bundle and selected implementation at build
time. The Plugin's existing root Slot must match. Every extensible Slot needs an
explicit `maxInstances` in 1..=256; the entire Host is capped at 256 active
Instances. Multiple policies for one Plugin ID are rejected. No version range or
open Capability wildcard is implied. A new release requires a new Host build.

The normal `plugins add` operation can install an admitted bundle. It remains
inactive until an Instance is configured/enabled. Direct Root edits, check/show,
configuration proposals, selection changes, installation, update, and rollback
all use the same generated Host authority. The Root reader verifies the bundle's
exact manifest digest and retains the Host-selected execution implementation;
it does not rerun the legacy machine-wide implementation preference list.

Use `cardinality: "optional"` for an initially empty single-provider Slot.
For a `one` Slot with a default, `replaceable: true` explicitly permits an
admitted candidate to replace that default through the existing resolver. Merely
listing a different Plugin in `allow` does not make the Slot replaceable. Listing
a release of a default Plugin's own ID explicitly permits that exact root release
override. A default Instance itself still cannot be disabled in this profile.
Limits count the final selected Instances, so a replaced default is not counted
as a second active Instance.

`configurationSchema` is an additional Host constraint on the effective selected
configuration after package defaults, Host values, and Root overrides are merged.
The Plugin-owned Schema must pass independently; neither Schema is rewritten.
Thus an inherited out-of-bounds default fails too, unless the resulting explicit
configuration satisfies both constraints. Disabled/absent Instances do not execute;
their effective configuration is checked when they become selected. Omitted
`configurationSchema` adds no restriction beyond the existing Plugin contract.

This Host-only constraint uses a bounded JSON Schema Draft 2020-12 profile:
`type`, `const`, `enum`, `required`, `properties`, `additionalProperties`, `items`,
`minimum`, `maximum`, `minItems`, `maxItems`, `uniqueItems`, `minLength`,
`maxLength`, `allOf`, `anyOf`, `oneOf`, `not`, `if`, `then`, and `else`, plus
`title`, `description`, and `$comment`. Boolean schemas are allowed. Unknown
keywords, references, formats, and regexes are rejected, including in inactive
branches. Depth is capped at 64 and schema nodes at 4096. The validator has HTTP
and file retrieval disabled. This does not expand the Plugin Descriptor's
published Schema profile.

Schema failures name the Instance and structural paths without printing values.
Updated Host policy invalidates old configuration proposal digests. These are
real authoring constraints, **not a filesystem/network sandbox**: arbitrary
trusted Bun/Process code still has its actual execution environment's authority.
Enforcing direct OS access needs an appropriate Execution Adapter. The prepared
directory and private lifecycle path now exist; product runtime assembly and
released offline artifacts remain separate delivery work.

## Static declaration grammar

Supported forms are one default `defineHost(...)`, relative default imports of
declaration files, named SDK helper imports (including aliases), `const`
references, arrays/objects, finite numbers, strings, booleans/null, and `as` or
`satisfies` wrappers. Handler modules are not loaded. Imported declarations
must be static too; a reference to arbitrary `plugin.ts` is rejected.

Environment reads, dynamic imports, arbitrary calls, mutation, spreads, computed
properties, cycles, duplicate keys, and unsupported top-level statements report
errors. There is no evaluation fallback. Source count, bytes, expansion depth,
and expression/string expansion are bounded. This is metadata extraction, not
typechecking or compiling all application business code.

## Verification

### Native ownership transport (private)

`src/host-owner.ts` now connects to the native `lenso-process-owner` helper from
`lenso-runtime-rust` using bounded, versioned JSON frames. Node and Bun use the
same implementation. It validates the ownership handshake, joins repeated stop
requests, and returns confirmed or unconfirmed physical termination. The helper
continues cleanup after the launcher disappears.

This is not exported from `@lenso/cli/host`. An owned process is not a ready App.
The private `host-app.ts` layer now waits for the Rust Ready Gate, provides bounded
revision-bound structural inspection, joins stop, and returns both durable
suspension and physical-termination results. It never turns forced cleanup into
a clean business shutdown. Its `registry` must be the shared distribution-owned registry
outside replaceable application roots, not a different directory per start.

The Rust control fixture proves the TS -> owner -> runtime -> Host Controller
transport with Node and Bun. The product `lenso-host-runtime` adds independent
distribution verification, same-cohort resolution, Artifact admission, Adapter
assembly, durable activation, suspension, and exact recovery. Released target
artifact cohorts remain required for public installation.

With a locally built native helper, run the real cross-language check explicitly:

```sh
LENSO_NATIVE_OWNER=/absolute/path/lenso-process-owner \
LENSO_APPLICATION_RUNTIME=/absolute/path/control_fixture \
pnpm check:host-owner-native
```

The check fails if the helper is absent; it has no mocked fallback.

### Authoring and transport checks

Repository contributors run:

```sh
pnpm install --frozen-lockfile
pnpm check:npm-shim
cargo test --locked --workspace
```

The CLI integration test uses Node, Bun, and the generated extractor after the npm
build. It builds two verified Bun bundles with a custom Request binding, runs
`app build/check/show`, and rejects an additional Root Instance. Fixture Plugin
code deliberately throws if executed; declaration extraction and bundle inspection
never execute it. These checks establish authoring behavior, not running Plugin
conformance or a toolchain-free production deployment.

A second integration test builds an extensible Host, installs an admitted bundle
without activating it, and configures an Instance through the shared authority.
It checks configuration/count rejection without writes and rejects a different
bundle payload with the same Plugin identity and version. Unit tests cover
replacement, merged configuration, invalid ceiling schemas, and invalidation of
reviewed configuration proposals when Host policy changes.
