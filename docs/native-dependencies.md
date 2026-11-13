# Code extension native dependency management

Implemented in this CLI repository. Requires native CLI 1.0.0, Project provider
0.1.2 (included in CLI 1.0.0) and Code 0.2.0, available through the official `code`
alias and preset. SDK integration remains separate work.

Cargo and npm behavior belongs to the Code extension. The runtime still exposes
only generic reads, source resolution and explicitly granted processes. The
Project provider handles exact file plans, approvals and generic completion writes.

## Setup and first use

Enable native manifest targets through normal reviewed Code extension setup:

```sh
lenso ext add code \
  --input '{"repo":"owner/modules","format":"lenso-compose-v1","target":"src","native":{"cargo":{"manifest":"Cargo.toml"},"npm":{"manifest":"package.json"}}}'
lenso code compose-preview --sample app
lenso code compose --sample app
```

For a local author source, replace `repo` and `format` with
`"author_root":"author"`. Code extension source development can use the local
manifest produced by `scripts/configure-code.py`
as the explicit `ext add` source. For an installed Code extension, use its reviewed
setup workflow directly:

```sh
lenso ext configure code --input '{"native":{"cargo":{"manifest":"Cargo.toml"},"npm":{"manifest":"package.json"}}}'
```

The supplied `native` map replaces the native target configuration. Include every
ecosystem that remains enabled. Native-only reconfiguration preserves source
profiles and existing grants; newly requested native paths and exact processes are proposed explicitly. Old grants
are retained rather than silently revoked. Editing project configuration remains
the secondary way to revoke individual grants. The configured package is not an
implicit native grant.

There is no native install prerequisite. The first composition proposes native
manifest edits and installation automatically. Setup grants exactly the configured
manifest paths, native lock reads, project ownership record and installer commands.
File approval and command approval are separate; both are gathered before any
application mutation. Missing native configuration is an error when declarations
are selected, not permission to discover and edit arbitrary project manifests.

Each ecosystem has one explicit package manifest target per Code installation.
Nested paths are supported; there is no implicit workspace discovery or recursive
editing. An optional `lock` path identifies the Cargo workspace lockfile when it
lives above a member manifest. Configure the matching lock location explicitly.
The normal npm target uses `package.json` with `package-lock.json` beside it.
This initial implementation does not implement pnpm/Yarn/Bun, target-specific
Cargo tables, workspace-wide dependency tables or other native ecosystems.

`offline: true` requests offline native installation for that target. Global
`--offline` also selects the granted offline command variant. `program` can select
an explicit executable path instead of PATH's `cargo` or `npm`; it is part of the
reviewed process grant. Native toolchains are needed to execute native installation,
not to fetch extensions, validate catalogs or preview a plan.

## Declarations

Declarations remain in the generator and activate only for selected modules:

```toml
[[generator.dependencies]]
ecosystem = "cargo"
package = "some-driver"
version = "1"
module = "storage"
options = { features = ["sqlite"], default-features = false }

[[generator.dependencies]]
ecosystem = "npm"
package = "some-client"
version = "^2"
module = "web"
options = { section = "devDependencies" }
```

Omit `module` for a composition-wide requirement. Instance owners are retained even
when two instances select the same module. `options` is opaque to the generic
codegen interface and interpreted by the Code extension.

Cargo accepts `section` (`dependencies`, `dev-dependencies`, `build-dependencies`),
`features`, `default-features`, `optional`, `path`, `package` and `registry`.
Npm accepts `section` (`dependencies`, `devDependencies`, `peerDependencies`,
`optionalDependencies`). Unknown ecosystem/options and malformed entry types fail
before planning. Cargo uses version strings or native dependency tables. Npm uses
native version/spec strings, including local `file:` packages.

Repeated Cargo requests union features when their other fields match. Compatible
identical requests share one entry. Different version/source/options requirements
for the same ecosystem/section/package are rejected with an identity diagnostic;
this layer does not solve arbitrary native version intersections. Requirements in
different sections remain independent. Native package managers remain authoritative
for version syntax, transitive resolution and package compatibility.

## Manifest changes and conflicts

The extension edits only requested dependency entries. Cargo editing preserves
unrelated TOML content and comments. Npm editing preserves unrelated JSON values
and emits stable pretty JSON; formatting and key order may change. Duplicate JSON
keys are rejected before rewriting, rather than silently discarding a declaration.

Existing user declarations are not silently adopted as disposable generated data:

- An identical pre-existing entry is shared and retained when Lenso's last owner
  disappears.
- A different pre-existing or locally modified entry yields a `native:<digest>`
  select question showing current and proposed values. `keep` preserves it;
  `replace` applies the proposal. `--yes` cannot select either answer.
- Updating an unchanged Lenso-managed entry applies the new request. A kept entry
  does not repeatedly prompt for an unchanged request. A later request change can
  produce a new conflict.
- Removing the last owner restores the original declaration if one existed, or
  removes the entry introduced by Lenso. A locally changed obsolete entry requires
  `keep` or `replace`; keeping releases ownership and retains the user's entry.

Empty dependency tables are retained. Unrelated dependencies and other manifest
fields remain present. Native dependency declarations themselves are not source
file ownership records and do not authorize arbitrary file deletion.

## Shared project record

`lenso/native.lock` is a project-local TOML ownership and preparation record.
Commit it alongside root `lenso.toml`, `lenso.lock` and generated composition state.
It is not a package download cache and does not replace Cargo/npm lockfiles.

The shared record identifies manifests, requested entries, composition groups,
instance owners, original values, last applied/requested values and successful
installation fingerprints. Composition identity binds resource source, catalog
path and artifact destination. Mutable revisions are excluded so upgrades retain
ownership. Different Code profiles producing different composition artifacts can
share a native entry. Removing one profile's owner leaves other owners intact.

The record is written before native manifest replacements. It retains original
baselines and pending work even if installation fails. An empty request group can
act as a retirement retry marker until the removal installation succeeds. Native
records are local editable state, not signed ownership claims or additional grants.
Host reads and Project preimages guard against stale concurrent edits.

Retire requirements through the original profile before removing its native target
configuration or relocating its composition artifact. Removing the Code extension
itself retains generated code and native state, consistent with existing extension
removal behavior; it does not guess that application dependencies are unused.

## Installation, completion and retries

Code proposes exact shell-free commands:

- `cargo fetch --manifest-path <manifest>` prepares registry/path dependencies and
  synchronizes the native Cargo lock. See [Cargo fetch](https://doc.rust-lang.org/cargo/commands/cargo-fetch.html).
- `npm install --prefix <manifest-directory> --ignore-scripts --no-audit --no-fund
  --package-lock=true` installs the resulting native manifest and synchronizes its
  lock. See [npm install](https://docs.npmjs.com/cli/v12/commands/npm-install/).
- Offline preparation appends `--offline`.

These are full native manifest operations. The package manager owns transitive
resolution, locks, its normal cache and installed-package layout. Npm lifecycle
scripts are disabled. Code never directly deletes vendor files, native caches,
Cargo target directories or arbitrary `node_modules` paths. Installer side effects
are covered by command authorization, not a precomputed diff of native lock bytes.
The existing process environment/cancellation/logging policy still applies.

The reviewed plan includes prepared file contents, commands and exact successful
completion contents for `lenso/native.lock`. Project gathers all confirmations,
writes preparation state and manifests, runs commands, checks prepared outputs,
then finalizes the reviewed completion record. Completion changes are included in
the initial plan digest and preview; they do not cause a second approval round.
The generic Project mechanism contains no Cargo/npm parsing.

A failed installer leaves a pending receipt with original baselines intact. Running
the same composition again proposes installation again, including after the last
owner was removed. Successful unchanged manifests with existing native lockfiles
produce no repeat commands. `code compose --install` explicitly reinstalls after
cache/installed-directory loss or retries preparation; `compose-preview --install`
shows the same forced plan without executing it.

Fingerprints bind manifest semantics and installer identity; they are not an
integrity inventory of `node_modules` or native package caches. Native lockfiles
remain authoritative for resolved versions. Code records their observed preimages
for review but does not certify future manual edits to locks/caches. Use `--install`
to resynchronize after external changes. Global `--locked` retains its existing
Lenso distribution-lock meaning; native lock updates are reviewed installer effects.

File application and completion are individually guarded, not a globally reversible
transaction. Disk errors or commands can leave completed mutations; failures report
completed paths/commands. A native lock/cache write is not automatically rolled back.
If a command changes a prepared manifest unexpectedly, completion fails and pending
state is retained. Do not run competing native installers as if this were a global
package-manager execution lock.

## JSON/Agent and verification

Existing `--json`, `--answers`, questions and exit statuses are reused. Composition
output includes native counts under `native`, exact `files`, `commands`,
`finalizations` and dependency proposals. Conflict IDs bind current/requested values
and ownership state; plan and command approvals bind the complete plan digest.
Preview does not execute installers or write application/native manifest files.
Author validation checks selected native request shapes/conflicts without installing.

`python3 scripts/test-native.py` exercises actual CLI/Wasm execution and actual
offline Cargo/npm operations against self-contained local packages. It covers
separate consent, denied grants, user baselines, updates, module/profile sharing,
installer failure and retirement retry, safe removal, native lock creation,
no-op replay and disabled npm lifecycle scripts. No official Registry or SDK is
needed by this acceptance fixture.
