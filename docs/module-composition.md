# Composable modules across CLI, SDK and Registry

Status: CLI foundation implemented in this repository. SDK and official Registry
changes below are proposed contracts, not shipped integrations. The example Auth
catalog contains acceptance fixtures, not a production authentication service.

## Decision

Maintain behavior contracts and independent adapters, rather than an Auth variant
for every database, cache, crypto library, language and runtime combination.
An Auth module consumes an `AuthStore`, a password hasher and, if enabled, a
session cache. PostgreSQL and SQLite each implement the AuthStore behavior. Redis
and an in-process cache each implement the session cache behavior. A recipe can
recommend a starting selection; users can replace one adapter without replacing
an entire recipe.

A generic SQL client is too weak a contract for this seam. Transaction semantics,
uniqueness, expiry, account lookup and credential updates must be defined by the
AuthStore contract and implemented by each storage adapter. A password hasher
must expose password-specific behavior and policy; arbitrary encryption or a
fast digest is not an interchangeable substitute. Metadata only selects eligible
implementations; SDK type checking and behavioral tests establish correctness.

Three graphs remain distinct:

1. The executable extension dependency graph prepares CLI commands and their Host
   grants. The optional Project extension is needed only by file-changing workflows.
2. The composition graph connects application instances through capability slots.
3. The language package graph installs the selected implementations' native
   dependencies through an explicitly approved project toolchain operation.

A composition graph must not become a second language package manager. Native
package resolution and build verification remain language-specific extension work.

## Responsibilities and identities

| Concept | Identity/example | Owner |
| --- | --- | --- |
| Executable extension | `@lenso/code` package and version | CLI distribution; Release archive, digest, Host grants |
| Preset/recipe | An onboarding selection of extensions and module intent | Registry author; does not duplicate implementation sources |
| Contract | `example.auth-store`, contract version and future compiler fingerprint | SDK contract author |
| Module definition | `auth` or `pg`, module version in a pinned catalog | Registry module author |
| Adapter | A module providing one or more contracts | SDK implementation plus Registry descriptor |
| Application instance | `auth`, `staff`, `auth.store` | Project composition intent |
| Binding | `auth.store -> auth.store` or `auth.hasher -> shared-hash` | Project composition intent |

Module IDs in schema 1 are local catalog IDs. Contract IDs can be qualified strings.
They are not automatically resolved as CLI extension namespaces. The Code extension
reads a complete catalog from one pinned repository snapshot. Cross-catalog
imports and independently versioned module catalogs require a future import-lock
contract; they are not silently fetched by the resolver.

The runtime retains generic repository reads, immutable caching, provenance,
permissions and extension execution. It knows nothing about Auth, SQL, Redis,
component formats or application bootstrapping. The pure `lenso-composition`
library is linked into the Code Wasm extension; runtime and framework do not
acquire a dependency on its business resource format.

## CLI foundation implemented now

The pure interface is `Catalog::evaluate(&Request) -> Result<Evaluation, Error>`.
It returns either the next typed choice or a deterministic resolution. No network,
filesystem, environment lookup, native command execution or code rendering occurs
inside this library.

A module describes:

- A fixed module version and provided capability contract versions/attributes.
- Named consumer slots with contract version constraints and required attributes.
- Typed text, boolean, integer, enum and environment-reference options. Defaults
  are explicit author decisions. Missing values produce questions.
- Optional slots and simple equality conditions on its own configuration fields.
- Supported project context values, such as language and runtime.

Bindings create independent child instances at slot addresses. Reusing the same
module twice does not imply shared state. Sharing requires explicitly binding to
a named root instance. Dependencies are ordered before their consumers. Cycles,
incompatible bindings, unknown active configuration, unsupported context and graph
limits are errors with instance addresses.

The resolver filters candidates using contract versions, advertised attributes and
known project context. A single compatible required module is selected directly
when no matching named root instance exists. Multiple candidates and optional
absence require an explicit choice. This is bounded interactive resolution, not a
SAT solver: a chosen subtree can fail and require a different choice. Schema 1
supports local equality conditions, not arbitrary expressions or executable
catalog hooks. Catalog module versions are fixed; the existing extension package
version solver is separate.

Booleans are select questions, so `--yes` cannot decide whether caching is enabled.
`--yes` continues to answer authorization confirmation questions only. Existing
terminal controls, JSON ABI 1, answers files and Agent exit statuses are reused.
Question IDs bind the pinned commit, current intent and pending choice.

Environment options accept only `{"env":"DATABASE_URL"}`. The running
application, not the generator, will resolve the value. The CLI never reads the
named application variable. Do not enter secret values into ordinary text fields
or answers files; those values are intentionally persisted as project intent.

## Code extension flow

The new Code extension source package is version 0.2.0. It is not published or
added to the official Registry by this change. Existing published Code versions
retain their existing commands. Authors build and publish the Wasm package with
the established Release pipeline; native consumers still require no Node.js or Git.

With the new package available, onboarding uses the existing extension setup:

```sh
lenso ext add code --input '{"repo":"owner/modules","ref":"v1","catalog":"catalog.toml","target":"src/generated","format":"lenso-compose-v1"}'
lenso code compose-preview auth
lenso code compose auth

# Reuse saved roots, bindings and configuration; verify pinned cached bytes.
lenso code compose-preview --locked --offline
```

For local author development, with a Rust Wasm toolchain available:

```sh
python3 scripts/configure-code.py /path/to/project \
  --repo owner/modules --ref v1 --catalog catalog.toml \
  --format lenso-compose-v1 --target src/generated
```

No manual `lenso.toml` editing is required by either entry point. The author helper
is a source-development tool; it is not an extra consumer installation requirement.
A future official composition preset can carry this profile and recommended intent.

The Code extension resolves the repository through the existing Host, reads the
TOML catalog, asks for missing composition choices, reads only selected module
resources, then returns one combined Project plan. `compose-preview` stops at the
plan; `compose` asks for approval, applies the plan and verifies exact file bytes.
Repository and read/write grants still apply even if all bytes are cached. There
are no new Host imports or automatic grants.

Adapter source files are copied once per selected module definition to
`<target>/modules/<module-id>/<resource-target>` for legacy catalogs. A generator
declaration places all copied/rendered output below its isolated generated
directory, with module, instance and composition ownership scopes. Instance configuration and
bindings are stored separately in generated `<target>/composition.json`. The
artifact contains original intent, effective instances, dependency order, catalog
identity, pinned commit and a deterministic resolution digest. The existing root
`lenso.lock` pins repository resources and content hashes. The resolution digest
covers the graph and canonical catalog, not the source file bytes; source byte
integrity is supplied by Host provenance and the reviewed Project plan.

The generated artifact is machine-maintained output and a reusable source of
intent. The catalog is TOML; project configuration remains root `lenso.toml`;
`lenso.lock`, the generated artifact and Agent envelopes remain machine JSON.
No additional editable JSON configuration is required.

Explicit `--input` supports roots, configuration, bindings and context:

```json
{
  "instances": {"auth":"auth", "staff":"auth", "hash":"argon2"},
  "context": {"language":"typescript"},
  "configuration": {
    "auth":{"cache-enabled":true},
    "auth.store":{"connection":{"env":"DATABASE_URL"}},
    "auth.cache":{"connection":{"env":"REDIS_URL"}}
  },
  "bindings": {
    "auth.store":{"type":"module","module":"pg"},
    "staff.store":{"type":"module","module":"sqlite"},
    "auth.cache":{"type":"module","module":"redis"},
    "auth.hasher":{"type":"instance","instance":"hash"},
    "staff.hasher":{"type":"instance","instance":"hash"}
  }
}
```

Use `lenso code compose --input '<JSON>'`. A positional module creates a fresh root
named `app`; explicit `instances` creates fresh named roots. Supplying neither
reuses the artifact's saved request. Each supplied `configuration`, `bindings` or
`context` map replaces that saved map, rather than recursively merging it. Omit a
map to retain it. To change a provider with different option names, replace the
relevant configuration map as well; stale active options are rejected.

Disabled conditional branches retain prior intent but do not appear in effective
instances or selected resources. Re-enabling revalidates their configuration and
bindings. Generation and update behavior is described below. The Code extension
can generate extension-authored factories and assembly through strict templates; see [authoring code generators](code-generation.md). It does not generate
main-SDK factories, install language dependencies, edit user-owned imports or
compile a production Auth application. Those operations need the SDK contract
below and a language-aware code extension. The artifact is the explicit handoff.

### Ownership, incremental updates and cleanup

`composition.json` now includes a versioned `generation` receipt. It identifies the
Code composition generator and implementation version; each path records its
module, all effective instance owners, exact upstream base content and SHA-256.
The containing artifact supplies catalog identity, revision and module versions.
These are project-local baselines, not shared cache entries. Commit the artifact
with generated code; it contains no application environment values read by Lenso.

Each run compares the last generated base, current project content and newly
selected upstream source. Only changed files are proposed. Local-only changes
survive; disjoint line edits merge conservatively. Overlapping edits and conflicting
unowned destinations produce a `generation:<digest>` select question with a diff.
`keep` preserves current content; `overwrite` selects generated content. The digest
binds the base, current and proposed content, prior receipt and input. Reusing an
answer after the current file changes cannot authorize a different choice.
`--yes` never answers these select questions.

Keeping an active file advances its upstream baseline so a repeated identical run
does not repeatedly ask about the same upstream change. Subsequent upstream changes
are compared against that baseline. A local deletion survives when upstream is
unchanged; recreating a deleted file for a new upstream version requires a choice.
This merge is line-based and conservative, not an AST or semantic merge.

When the last instance using a module disappears, an unchanged owned file is
proposed for deletion. Modified obsolete files require `keep` or `overwrite`:
`keep` retains the file and releases ownership; `overwrite` deletes it. Released
and unrelated files are never collected by later runs. Files still used by another
instance remain owned. Empty directories are left in place. Receipts cannot propose
cleanup outside the previous composition's module output paths, and invalid owner
sets or base hashes are rejected. A receipt is editable local state, not a grant
or a cryptographically authenticated declaration of ownership.

Legacy artifacts without receipts can adopt identical selected files or ask about
a conflicting destination. They never infer ownership of obsolete files. Deleted
or missing receipt state therefore disables automatic cleanup of those files.
A different catalog identity requires a separate target or deliberate migration.

`compose-preview` returns proposed replacements, deletions, receipt, expected file
postconditions and resource counts (`written`, `deleted`, `kept`, `unchanged`,
`merged`). `compose` follows the same decisions, then requests one digest-bound
Project approval for the exact mutations, including its receipt update. A repeated
identical run produces no file mutations. Source refresh is explicit through
`--refresh`; pins and offline behavior remain unchanged.

Project provider 0.1.1 introduced file deletions; current Code 0.2.0 requires
provider 0.1.2 for reviewed completion writes used by native installation. Older native binaries fail dependency resolution instead
of silently ignoring cleanup. CLI 1.0.0 includes Project 0.1.2; Code 0.2.0 still
requires a local extension build. Project stages all replacement bytes and rechecks every preimage before
applying deletions, then replacements, with the composition receipt last. Per-file
operations are guarded, but a later I/O failure can leave completed changes. Errors
list completed paths; no whole-plan filesystem transaction or rollback is claimed.
Re-running reconciles completed operations against the surviving old receipt.

Current limits include 128 catalog modules, 32 named roots, 128 effective instances,
32 levels, bounded options/slots and 1 MiB metadata/request limits. A generated plan
has at most 127 adapter files plus its artifact and 1 MiB total output. These bounds
are enforced in addition to the existing Wasm execution and Host resource limits.

See [the executable catalog fixture](../fixtures/composition/catalog.toml) for the
complete schema and the selected adapter resource mappings.

## Main Lenso SDK design — proposed, no implementation here

Build on Lenso's existing `.lenso` contracts and compiler-owned composition model.
The main repository already describes TypeScript, Rust and Go implementations,
capability attachment and application-owned lifecycle. Its Notes/Audit example has
multiple slots implementing the same NotesStore contract, which is the correct
model for independently configured instances. Do not introduce a second contract
language or a parallel compiler in the CLI.

The SDK/compiler should export a versioned, machine-readable contract descriptor:
contract identity, semantic version, canonical schema fingerprint, operations and
errors, supported binding kinds, lifecycle requirements and configuration schema.
Fingerprinting must include relevant action semantics/schema, rather than hashing
a display name. Schema 1 CLI metadata is only a selection prefilter; the SDK
compiler remains authoritative for whether selected implementations satisfy the
actual application contract. Behavioral compatibility still needs contract tests.

Each native adapter should expose a typed factory whose inputs are validated
configuration, explicit resource handles and explicit capability dependencies. A
factory creates an instance; a module package is not a singleton. The application
owns startup, shutdown, migrations and connection pools. A shared root binding
should create one resource owner and dispose it once, after consumers stop.
Partial startup failures must unwind already-started resources deterministically.
An in-process cache is not a distributed cache, and a provider's contract metadata
must state the semantics consumers depend on.

Database adapters should own SQL dialect, migrations, transaction semantics and
native driver dependencies. Business Auth logic consumes an AuthStore contract.
A separate composition can share one pool through an explicit pool contract when
appropriate. Schema changes and data migrations are reviewed operations; selecting
a new store must not silently migrate user data. Password policy upgrades similarly
need explicit compatibility and rehash rules, not just a library-name switch.

The language-specific code extension should consume the CLI resolution and ask the
SDK compiler to validate it, then emit native imports, typed factories, configuration
bindings and application assembly. Generate `.lenso` source or use a documented
compiler-owned IR; never handcraft an undocumented compiler structure. Reuse the
same descriptor schema across TS/Rust/Go, with language-specific implementations
and explicit runtime support. A TS-only adapter cannot be selected for Rust merely
because it advertises the same behavior contract.

Before enabling this integration, agree on:

| SDK output/input | Required contract |
| --- | --- |
| Descriptor export | Schema version, contract ID/version/fingerprint, options and supported targets |
| Composition validation | Instance/slot addresses and actionable incompatibility diagnostics |
| Generation | Deterministic native code plus declared dependencies and evidence; no writes during planning |
| Native lifecycle | Ownership, explicit sharing, startup order, cleanup and migration policy |
| Verification | Type/build checks and per-provider behavioral conformance results |

The main repository currently also has application tooling named `lenso`.
Coordinate its command namespace and packaging through an explicit app-tooling
extension before promising a unified command. This CLI does not replace that tool
or import its changing internal libraries as part of this change.

## Official Registry design — proposed, no implementation here

Keep the current executable package Registry for name-to-Release resolution. Add
module catalogs as a separate resource kind, delivered through pinned GitHub
repository snapshots. A website can mirror discovery metadata later; serving an
HTTP JSON API should not be a requirement for a module author.

A versioned module catalog should reference:

- Contract descriptors exported by the SDK, including schema fingerprints.
- Module definitions, supported language/runtime profiles, typed configuration,
  required/provided contracts, adapter source mappings and native dependencies.
- Recipes containing recommended roots, defaults and slot bindings. A recipe is
  editable intent; it must not contain another copy of all variant source files.
- Generator extension identity/version requirements, ownership/license information,
  immutable source references and conformance evidence.

Separate catalog schema version, module release version, contract version and
executable generator version. Resolve mutable discovery once into immutable
revisions and hashes. Future multi-catalog imports need a deterministic import lock
with unique qualified IDs and conflict diagnostics before they are supported.
User-level caches store immutable source bytes; instance options and secrets do
not belong in a shared cache. Authenticated sources retain explicit Host grants.

Prefer one business module and a small set of adapters. Add a genuinely distinct
business module only when the behavior contract differs. Runtime/language support
is eligibility metadata, not a promise that all providers are interchangeable.
Some incompatibilities are real: required transaction behavior, distributed
invalidation, crypto policy or deployment restrictions should reject a choice,
not grow a hidden exception matrix or silently pick another backend.

Registry CI should verify descriptor/schema integrity, deterministic rendering,
path/resource safety and source/license provenance. SDK CI should run a shared
behavioral suite against each provider, including failure, concurrency, lifecycle
and migration semantics. Run selected representative compositions and explicit
known interaction cases, with pairwise coverage where useful. Do not claim that
pairwise tests prove every possible cross-feature behavior. Avoid generating and
maintaining the full Cartesian product of templates or integration jobs.

## Rich source changes and updates

The extension seam must eventually support more than copying a source tree, but
that does not require teaching the generic runtime TypeScript or SQL.

Language-aware generator extensions should turn a validated graph into a semantic
edit plan: add an import, register a factory, bind a dependency, amend package
metadata or propose a migration. Use the language's parser/compiler and stable
symbols or explicit generated regions. Do not use catalog-authored regex rewrites
for arbitrary application files. Return the resulting exact file/command plan to
the existing Project review and authorization flow.

Project-local file ownership receipts and conservative line-based updates now exist
in the Code extension. Future language-aware generators should extend this contract
with owned symbols or regions and declared native dependency owners. They must not
overwrite user edits or delete shared output because one instance disappeared.
Dependency removal must account for other owners; obsolete data migrations must
never be treated as disposable generated files.

Receipts, semantic operations and compiler validation need a separately versioned
extension contract. Keep Host primitives generic and permission-limited; verify
all reads and outputs against the reviewed plan. Script execution, dependency
installation, migrations and builds require the existing explicit command and
permission flow, even when a recipe recommends them.

## Coordinated delivery sequence

1. **CLI, this change:** pure typed resolver, scoped instances, explicit sharing,
   conditional slots, pinned Code catalogs, deterministic artifact and existing
   preview/apply/Agent workflows. Real Wasm acceptance covers two Auth instances,
   PG/SQLite, optional Redis, shared hashing, denial and locked/offline replay.
   The author generator interface, isolated templates and read-only validation are
   also implemented, with compiled/running memory and disk Rust adapter samples.
2. **SDK:** export one contract descriptor; implement and verify one AuthStore
   contract with SQLite and PG adapters, native factory lifecycle and explicit
   sharing. Publish a deterministic assembly interface before code generation
   depends on it. Start in one language; declare support honestly.
3. **Registry:** publish descriptors, adapter sources and one editable recipe in
   versioned catalogs. Validate each adapter against the SDK contract suite and a
   small set of real application compositions. Publish the new Code extension
   version and a preset only after this interface is exercised.
4. **CLI code extension + SDK:** validate the graph through the SDK, generate real
   assembly and native dependencies, then extend the existing file receipts to semantic
   updates and native dependency ownership. Existing file previews, scoped grants and Agent envelopes remain the
   execution path.

Completion for the next cross-repository slice means one application can change
only its store binding from SQLite to PG, optionally enable Redis, and create a
second independently configured Auth instance without maintaining a new template.
It must type-check, exercise contract behavior, start/stop resources correctly,
keep credentials in the application environment and expose all project mutations
for review. The current fixture proves the CLI composition/distribution path;
it does not claim this application-level acceptance has already shipped.
