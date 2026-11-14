# Files integration and App host v2

Status: implementation prepared for App 0.2.0 and module source train v0.4.0
(Auth shared identity; Files 0.1.0). Code remains 0.3.6. The native CLI Runtime and
published SDKs are unchanged. Official Registry/preset publication is a separate
step after branch acceptance.

## Native host protocol

An authored native HTTP module can emit `assembly/host.toml`:

```toml
protocol = 2
requires_identity = true
```

The host mounts modules not requiring identity first, accepts exactly one identity
provider, then mounts dependent modules. Missing or ambiguous identity fails startup
before routes are served. The App host contains no Auth/Files business operations.
Auth resolves its existing sessions and publishes an identity interface. Files uses
that interface and the SDK principal guards for each operation. It never creates a
second Auth store, trusts request owner IDs or queries Auth's SQL tables.

TypeScript's `src/lenso-host.ts` defines the structural native protocol. Module
`mount(migrate, services)` returns close, an optional identity resolver and a stream
handler. Requests carry a bounded native source, abort signal and lazy JSON reader;
responses can contain JSON or a native byte stream. Pipeline ownership provides
backpressure and closes streams on cancellation. Legacy JSON-only handlers remain
supported. Raw bytes never enter Wasm or CLI JSON messages.

Rust's `src/lenso_host.rs` defines native Identity/Services/Mounted types. The build
script reads host descriptors, generates typed module references and passes explicit
services into v2 mounts. Ordinary Axum routers handle native streams. Modules without
a descriptor keep their v1 router entry. There is no dynamic DI system or cross-language
runtime bridge; language-native compilers verify composition.

## Reviewed upgrades

App initialization still hands application source to the user. It never uses a
repeat init to overwrite an existing host. Upgrade an App 0.1.0 project explicitly:

```sh
# After the matching versions are published:
lenso ext add app --constraint '^0.2.0'
lenso app upgrade-preview
lenso app upgrade
lenso code update auth --input '{"reconfigure":true}'
lenso app check
```

Upgrade reconstructs the known 0.1.0 host baseline, merges disjoint changes and
returns conflicts through normal JSON/terminal questions. Conflict answers bind
the current/proposed content digest; --yes does not choose a replacement. File
approval remains separate. Existing native manifests and module records are
preserved; Rust adds required stream features to the existing Tokio dependency.
The new protocol files and project host convention are applied together. App 0.2.0
upgrade is idempotent. Unsupported baselines and existing customized protocol files
stop for explicit manual review.

Then install Files through Code. Language/runtime/output conventions remain project
facts, while metadata/content providers remain module choices. Schema migration
and app startup keep their existing separate native-command authorization. Updating
Auth preserves its data format and existing sessions; Code reviews its catalog
reconfiguration and owned generated changes. Files updates preserve Auth ownership
and local customizations.

## Verification

Build App Wasm and package App/Code under `dist/mainline` and `dist/author`, as in
`verify-app.py`. Run:

```sh
python3 scripts/verify-files.py --binary target/debug/lenso \
  --modules ../lenso-modules --storage both --out dist/files-verification
```

DATABASE_URL must point to a disposable PostgreSQL database. To verify a real old
host upgrade, provide `--legacy-app DIR` containing the published App 0.1.0 manifest
and Wasm. The test HTTP origin keeps a stable extension source while advancing the
version. It tests the actual extension upgrade, preview/denial, host merge, Auth
reconfiguration, preserved session, and subsequent private Files operations.

Metadata and blob implementation, migrations, interfaces and fixture ownership
remain in lenso-modules. App owns the native host protocol; Code supplies its existing
selection/generation/ownership machinery. No Files registry is added to the CLI Host.
