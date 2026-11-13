# App extension

App 0.1.0 creates native TypeScript/Node.js 26 or Rust/Tokio HTTP applications.
It depends on Project and Code 0.3.6. The `app` preset groups App and Code and uses
`app/init` for creation/adoption. It contains no Auth implementation.

## Commands and ownership

- `lenso new api --preset app`: choose a language, review grants, review files,
  authorize dependency preparation, then finalize the bootstrap receipt.
- `lenso app preview`: calculate the bootstrap plan without running native tools.
- `lenso app init`: initialize an empty target; initialized source is user-owned.
- `lenso app prepare`: retry pending initialization or prepare native dependencies.
- `lenso app check`: authorize `npm run check` or `cargo check`.
- `lenso app migrate`: authorize a native command which migrates module schemas
  and exits. Migration is explicit and can modify an external database.
- `lenso app run`: authorize and supervise the native server until interruption.

Applications run without Lenso CLI using their native package commands. Bootstrap
refuses differing existing native files and opposite-language manifests. It merges
compatible `lenso.code.toml` settings while preserving unrelated conventions and
module options. It rejects changing the installed language/runtime or host ABI.
A pending `lenso/app.lock` becomes ready only after successful preparation;
failures leave a reviewable, retryable project. A ready receipt prevents re-init
from overwriting customized hosts, manifests and build settings.

## Project configuration and Auth

App records the selected language/runtime, defaults `paths.modules` to
`src/modules`, and records `conventions.app_host = "lenso-http-v1"` in
`lenso.code.toml`. Existing compatible paths below `src` are retained. Code owns
module selection, flexible providers/options, generated files, native dependencies
and update conflicts. Auth 0.3.0 supplies transport-specific targets in the module
repository; normal Code consumers continue using base catalogs.

```sh
lenso code config --provider lenso.auth-store=sqlite
lenso code preview auth
lenso code add auth
lenso app migrate
lenso app run
```

To use PostgreSQL, select `lenso.auth-store=postgres`, preview and update Auth,
then set its referenced connection in `.env` and explicitly migrate. Do not put
connection values in Code configuration. The initial Auth transport exposes one
primary instance named `auth`, four JSON/bearer-token routes and no browser UI.
Multi-instance transport and non-HTTP hosts require their own module adapters.

## Native host contract

The TypeScript host compiles source then discovers sorted module directories at
`paths.modules/*/assembly/host.js`. Each exports async `mount(migrate)`, returning
async `handle({method,path,authorization,body})` and `close()`. A handler returns
`{status,body}` or undefined. Rust's build script discovers
`paths.modules/*/assembly/host.rs`, each exporting async
`router(migrate) -> Result<axum::Router, Box<dyn std::error::Error>>`.
Both hosts expose `/health`, bind loopback, load the project `.env` and read PORT
(default 3000). A port of zero is available for automated acceptance. Application
source owns hosting; authored modules own routes, identity and persistence.

Preview performs no npm/Cargo/database work. File and process approvals reuse
Project. Long-running and migration commands use the existing Host Process
supervisor with an explicit confirmation. Native output goes to stderr; JSON
stdout contains only structured workflow results. `--yes` accepts confirmation,
not language/provider choices. Native commands inherit PATH; `.env` is read only
by the native application and is ignored by Git. CLI caches/config/locks never
contain runtime database credentials.

## Development verification

Build App and Code Wasm, stage `dist/author/{app,code}/extension.toml` and their
`{app,code}.wasm`, then package using `lenso package --id ID --out dist/mainline/ID`.
The author workspace is private and consumes the existing Codegen library; the
Runtime and published SDKs have no App-specific changes.

```sh
python3 scripts/verify-app.py --binary target/debug/lenso \
  --modules ../lenso-modules --out dist/app-verification
# Add --storage both and set DATABASE_URL for a disposable PostgreSQL database.
```

The harness installs both real Wasm extensions and the app preset from a local
HTTP origin. It verifies language questions, metadata-only previews, both denial
stages, empty HTTP hosts, generated Auth compilation, migrations, actual auth
requests, persistence, user edit preservation, offline replay and supervised exit.
`--official` switches the same flow to the public Registry after publication;
local verification does not establish public Registry availability.

The `Verify native App consumers` Actions workflow accepts a reviewed module
commit/tag as `modules_ref`, builds the current App/Code Wasm and verifies both
SQLite and PostgreSQL on Linux for each language. It is dispatchable while the
three repositories are being released independently; native extension checks run
on every CLI PR. Publication supports `app-v*` tags and the existing release
checksum/packaging pipeline. Release order is Code, Auth, App, then official
Registry/preset synchronization and `--official` consumer verification.
