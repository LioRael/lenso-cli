# Official functional modules on the published SDKs

Status: implementation proposal. This plan creates no repositories or packages and
changes no SDK code. It supersedes the assumption that the foundational SDK must
be designed before functional modules can start.

## Verified baseline

Checked on 2026-11-13 against the public package registries and SDK repository
`LioRael/lenso` at `d0bd9d2e1e4a6383fc214b0b6232cc19da23286e`:

| SDK | Published baseline |
| --- | --- |
| TypeScript application core | `@lenso/app` 0.1.1 |
| Rust application core | `lenso-app` 0.1.1 |
| TypeScript SQLite tasks/events | `@lenso/sqlite` 0.1.1 |
| Rust SQLite tasks/events | `lenso-sqlite` 0.1.1 |
| Headless Console client/page registration | `@lenso/console` 3.0.0 |

The SDK supplies typed native Actions, Access, Resources, Modules, Tasks, Events
and Console descriptions. Its Module/Catalog describe actual native operations;
they do not implement the CLI's adapter resolver. Access supplies authorization
primitives, not a password authentication product. The SQLite packages supply
durable task/event facilities, not an AuthStore. The Console SDK is a client and
page registry, not the full Console application.

Use the published packages as dependencies. Initial reference environments follow
the demonstrated SDK consumers: Node.js 26 and Rust 1.99. Preserve independent
TypeScript/Rust builds and their native interfaces. No RPC, FFI or generated
cross-language runtime is required.

SDK evidence:
[packages and releases](https://github.com/LioRael/lenso/blob/d0bd9d2e1e4a6383fc214b0b6232cc19da23286e/docs/publishing.md),
[native semantics](https://github.com/LioRael/lenso/blob/d0bd9d2e1e4a6383fc214b0b6232cc19da23286e/docs/semantics.md),
[Private Files](https://github.com/LioRael/lenso/tree/d0bd9d2e1e4a6383fc214b0b6232cc19da23286e/typescript/examples/private-files),
[Team Desk](https://github.com/LioRael/lenso/blob/d0bd9d2e1e4a6383fc214b0b6232cc19da23286e/examples/team-desk/README.md).

## Repository ownership

| Repository | Owns |
| --- | --- |
| `LioRael/lenso` | Published SDK primitives and their compatibility contracts |
| Proposed `LioRael/lenso-modules` | Functional module source, business interfaces, adapters, migrations, templates, behavioral fixtures and reference applications |
| `LioRael/lenso-registry` | Curated module onboarding presets and source/version selection metadata |
| `LioRael/lenso-cli` | Generic Host and tooling; Code extension consumes module catalogs and generates reviewed project changes |

Create the module repository at `/Users/leosouthey/Projects/lenso-modules` when
implementation starts. Keep related official modules together initially, so a
business interface, two adapters and their shared tests can evolve in one change.
The Registry stays a small distribution catalog. Functional code remains outside
the CLI runtime and outside the SDK primitives.

Proposed initial source organization:

```text
lenso-modules/
  auth.typescript.toml          # Complete, directly consumable Code catalog
  auth.rust.toml
  modules/
    auth/
      contract.md
      fixtures/
      typescript/
      rust/
  adapters/
    auth-store/
      sqlite/{typescript,rust,migrations}/
      postgres/{typescript,rust,migrations}/
    password-hasher/
      scrypt/{typescript,rust}/
  templates/
    typescript/
    rust/
  examples/
    auth/{typescript,rust}/
    private-files/{typescript,rust}/
  tests/
    consumers/
    upgrades/
```

These names and paths are proposed author conventions, not a new runtime schema.
Root catalogs allow ordinary repository-relative source paths without parent
traversal. First delivery uses complete catalogs per installable feature and
language. When multiple features form a recipe, build one complete catalog from
the selected definitions at publication time. Commit consumable catalog outputs
before tagging the snapshot.

Code 0.2.0 does not resolve independent cross-catalog or cross-repository module
imports. Do not introduce a global module resolver as a prerequisite. Keep initial
catalogs within the existing source/output limits; a repository-wide mega-catalog
would make validation and source budgets grow with unrelated features.

## Authoring and distribution model

Functional modules initially ship as editable application source. SDKs and
third-party libraries remain native npm/crate dependencies. A functional module is
not a new executable CLI extension. Code supplies generation and update behavior.

Maintain business implementations as ordinary compilable TypeScript/Rust files.
Templates generate the assembly, imports, resource wiring, Action registration and
small project-specific configuration. Keep business algorithms out of large
conditional templates. Each module exposes a small native interface and accepts
explicit resources; it does not construct a global application container.

A source release pins one immutable Git tag/commit. The Code profile selects its
repository, root catalog, format and target directory. `lenso.lock` records the
actual repository commit and observed source hashes; generation receipts retain
file baselines and owners. Native manifests and their own lockfiles handle SDK and
provider dependencies.

Connection settings use environment-variable references, not persisted secret
values. SDK Resource scopes open and close native connections when the application
runs. Catalog validation and generation do not connect to the application's database.

Begin with a single source release train, such as `lenso-modules` v0.1.0, while
retaining module/interface versions in the catalogs. Independently versioned module
imports are future work, not a capability inferred from per-module version fields.

Functional interface versions specify behavior: atomicity, uniqueness, expiry,
revocation and errors. TypeScript interfaces and Rust traits establish native type
compatibility; the same behavioral fixtures establish semantic compatibility.
CLI contract IDs/semver select eligible adapters. Matching metadata alone proves
neither native type compatibility nor correct behavior.

## First module: password authentication and opaque sessions

Extract a reusable module from the SDK's Private Files reference implementation,
reviewing application assumptions rather than treating an example as production
identity infrastructure.

The first vertical slice includes registration, password login, current-session
resolution and logout/revocation. Session verification produces the SDK's trusted
principal through an application/transport adapter. Incoming client fields cannot
choose their own trusted subject or scopes.

Define the AuthStore and PasswordHasher native interfaces in the module repository.
The same business module consumes each adapter; SQLite/PostgreSQL variants do not
fork the business implementation. Store operations expose the business guarantees
needed by authentication, including unique account creation and session lifecycle
operations. Retain native transaction ownership inside the appropriate adapter or
explicit application operation; a generic CRUD interface cannot establish these
invariants.

Start with SQLite and the scrypt behavior already exercised by the SDK examples.
Confirm hash encoding, password verification and session expiry/revocation fixtures
in both languages. Additional password providers must pass the interface tests and
support stored credential verification/migration before becoming catalog choices.
Cryptography is not an arbitrary algorithm-name selector.

Add PostgreSQL as the second real durable store. This is the proof that the AuthStore
interface supports actual variation. Use real databases for uniqueness races,
transaction rollback, expiry, revocation, restart and error mapping tests.

Keep session caching disabled in the first release. Define revocation/freshness,
TTL and outage behavior before adding Redis or memory caching. Email recovery,
OAuth/OIDC, MFA and organization roles are separate increments with their own
behavioral interfaces. Document the initial authentication scope accurately.

## Coordinate Code and Registry without rebuilding the SDK

There is a small integration task before a functional preset can be turnkey.
Code's current setup workflow primarily obtains its source from `--input` or a
repository question. It does not yet adopt a preset-populated catalog profile as
its normal setup default.

1. Let Code use preset-provided `options.registries` profiles as reviewed defaults.
   Explicit user input overrides defaults, and missing values still ask questions.
2. Keep source/read/write/process grants in the existing review flow. Preset
   metadata cannot grant privileges by itself.
3. Let an `auth` preset choose pinned module source catalogs and offer the supported
   language/profile choices. Keep Auth-specific configuration in the module catalog
   and Code extension, not the runtime.
4. Publish the compatible Code extension change, then register the functional
   preset with its minimum Code version. Prefer the existing opaque options and
   preset format; require a newer CLI only if a demonstrated Host gap demands it.

The initial user experience targets prepared TypeScript/Rust projects. The
following commands are proposed, not currently available Auth commands:

```sh
lenso init --preset auth
lenso code compose-preview auth
lenso code compose auth
```

Onboarding selects the language/catalog. Composition asks for the store and other
missing choices, then previews generated files and native dependencies. Existing
project authorization and JSON/Agent behavior continue to apply.

The current Code preset does not scaffold a full native application. A later
`lenso new ... --preset ...` application recipe must explicitly create the native
manifest/entry and provide a working host. Do not claim that recipe until its
real install/build/run acceptance passes.

## Generated code, customizations and database state

Keep a user-owned application entry and business extension points. Put generated
module source and assembly in an explicitly owned directory. Record copied files
and templates through the existing receipts, so local edits merge or ask for a
conflict choice. Never rewrite unrelated modules as a side effect of installation.

Version database schema/migration source with the storage adapter. Applying a
migration is an explicit application operation with its own tooling and tests.
Code's current approved file/process mechanism does not supply a database migration
product or a transactional data rollback guarantee.

Switching SQLite to PostgreSQL can update assembly and retire obsolete generated
files/dependencies. It does not transfer user data or authorize deleting databases.
Support data conversion only through a separately designed, reviewed and tested
migration path. Do not present code cleanup as data migration.

## Delivery sequence and acceptance gates

| Stage | Deliverable | Acceptance |
| --- | --- | --- |
| M0 | Module repository, SDK-based native interfaces and Code preset-default integration | Both languages consume published SDKs; preset defaults/input overrides and grant denial work through published CLI; no SDK redesign |
| M1 | Auth + SQLite in TS/Rust, explicit HTTP adapter and small runnable consumer | Register/login/session/logout persist across restart; validation/authorization/expiry/revocation and uniqueness failures pass; generated consumers compile and run |
| M2 | PostgreSQL AuthStore | Same business module and behavioral fixtures pass on real SQLite and PostgreSQL; user customizations survive generated updates; database migration scope is explicit |
| M3 | Published source snapshot and official Auth preset | Fresh-project official onboarding uses immutable source pins, previews and approvals; native dependencies install; JSON, cache restore and locked/offline replay pass |
| M4 | Files module and private-files application composition | Identity, ownership and file operations compose through native SDK interfaces; optional Console bindings use the same Actions; adding/removing a module preserves the other |
| M5 | Notifications/mail, then membership/roles and audit according to consumer demand | Reuse SDK Task/Event mechanisms; external effects prove idempotency and failure handling; each added adapter passes its interface tests |

M0 and M1 form the first implementation batch. Do not implement every planned
module or every adapter before this batch has a working consumer.

Verification uses published SDK packages, not workspace path dependencies. Check
TypeScript and Rust independently. Run shared semantic fixtures against each
adapter, plus a small number of complete applications for assembly, authorization,
transactions and lifecycle behavior. This avoids maintaining a Cartesian matrix
without assuming that isolated adapter tests prove every composition correct.

For generation/update acceptance, install through the real Code/Registry path with
a new cache, compile the result, make a user edit, update templates/source, change
an adapter, decline and approve mutations, and verify stale-file/dependency cleanup.
Treat Console/browser acceptance as a separate gate when views are added; the
headless Console SDK does not imply a shipped management application.

## Immediate next step

Create the proposed module repository, establish its TS/Rust published-SDK consumer
checks and behavioral fixtures, and implement Auth + SQLite. Make the small Code
setup-default change alongside that consumer. Add PostgreSQL only after the first
vertical slice demonstrates the native interface. The core SDK remains unchanged
unless real consumer code reveals a reusable primitive with a concrete owner.
