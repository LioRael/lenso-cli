# Functional module variants and selection experience

Status: proposed module-authoring and Code-extension behavior. The published SDKs
and existing CLI/Code capabilities remain the baseline. This document is not a
claim that the variant-family discovery and target-selection flow already ships.

## Module identity and execution target

A user installs a module family such as Auth, then selects an implementation for
an application target. Language is a first-class choice at that level, not a late
template flag. Keep one discoverable Auth family with language implementations;
avoid exposing every combination as a separate top-level module name.

A target is one native build/application context with an explicit manifest root,
language/runtime, generated output ownership and resource configuration. These
properties apply to all module instances in that target. They are not a global
language setting for the entire repository.

For example, one workspace may contain:

- `apps/api`: TypeScript, Node.js, a package manifest and Auth instances.
- `services/worker`: Rust, its Cargo manifest and notification modules.

Multiple Auth instances in the same target can select different stores or explicitly
share one store instance. TypeScript and Rust native instances cannot share an
in-process object. Cross-target communication needs an explicit transport/interface;
selecting a shared contract ID does not generate an RPC bridge.

Initially each application can have its own `lenso.toml` and `lenso.lock`, selected
through the existing `--project`. Code 0.2 supports one native manifest per ecosystem
per Code installation. Centralized workspace management of several npm or Cargo
manifest targets requires an explicit extension-side design and acceptance; it
must not be implied by introducing the logical target model.

## Selection layers

| Layer | Example | Consequences |
| --- | --- | --- |
| Application target | `apps/api` | Manifest root, ownership scope and project policy |
| Language | TypeScript or Rust | Native SDK, implementation source, types and dependency ecosystem |
| Runtime/environment | Node.js host; supported Rust host; server versus browser | Available SDK/providers, transport and lifecycle requirements |
| Module and instance | Auth / `customer-auth` | Business operations and required capability slots |
| Capabilities | Password sessions; optional cache | Active graph and questions |
| Adapters | SQLite/PostgreSQL; supported password hasher | Concrete implementation, native dependencies and configuration |
| Parameters | Database path, environment-variable binding, session TTL | Effective validated configuration |

Language is the first implementation choice for a new target. For an existing
project, identify the target before asking about language. A package.json and a
Cargo.toml in the same workspace are evidence of multiple possible targets, not
permission to choose whichever manifest is found first.

Language alone is insufficient: a TypeScript server module using Node's SQLite
cannot be offered as a browser implementation. Only declare language/runtime
combinations with actual source, supported SDK dependencies and consumer evidence.
The current initial baselines are TypeScript on Node.js 26 and Rust 1.99. Do not
advertise Bun, Deno, Go or another host merely because the descriptor can name it.

New languages require native implementations, interface tests, dependency/install
support and generated-consumer compilation. A metadata entry alone cannot provide
language support.

## Author declarations and reusable implementations

A family descriptor associates stable variant IDs with their language/runtime
support, SDK requirements and complete Code catalogs. This descriptor is proposed
module-authoring metadata, interpreted by the Code extension. Runtime does not
learn Auth, languages, provider names or business configuration.

Registry presets supply the pinned family source and compatible defaults. The
family's source snapshot and selected catalog must resolve to immutable hashes in
the normal source-lock flow; language selection cannot fetch unpinned templates.
Complete catalog closures remain required until cross-catalog imports are designed.

Keep business behavior and shared fixtures together by module. TypeScript/Rust
implementations can live underneath that module and use separate root catalog
entrypoints. A high-level language choice does not require separate repositories
or separate public names for otherwise equivalent features.

Authors declare dimensions and constraints independently:

- Language support selects the native implementation and package ecosystem.
- Required capabilities select compatible providers within that language/runtime.
- Optional capabilities activate a subtree, such as session caching.
- Provider configuration declares typed parameters and environment references.
- Interface versions state behavioral compatibility and update expectations.

Do not maintain a template for every language/database/cache/hasher tuple. Copy
ordinary native implementations and render assembly from the selected graph.
An author supplies named recipes/samples for useful entry points; recipes are
compatible defaults over the same definitions, not forks of the business source.

Compatibility metadata filters choices. Native compilation and shared behavioral
fixtures prove the implementation. The current resolver is bounded interactive
resolution, not a global SAT solver; an impossible selected subtree must identify
its constraint path and allow the affected choice to be revised before application.

## Interactive experience

Proposed flow for an existing application:

```text
Install Auth
  Target       apps/api
  Language     TypeScript       detected from this target; shown in summary
  Runtime      Node.js 26       detected/declared; checked against support
  Store        SQLite           recommended default; PostgreSQL also available
  Sessions     opaque tokens
  Cache        disabled         optional feature is visible without extra questions
  Output       src/generated/auth

Review
  selected implementation and adapters
  SDK/native dependency changes
  generated file replacements/deletions and retained customizations
  required configuration bindings and requested permissions
```

This illustrates interaction, not current terminal output or an implemented Auth
preset. New targets ask for language explicitly. Prepared targets with one
unambiguous, supported language/runtime can skip redundant questions and expose
those choices in the final summary.

For fresh intent, apply explicit input first, then unambiguous target evidence,
then compatible preset defaults and author recommendations. Existing saved target
intent remains authoritative; conflicting project evidence requires a diagnostic
and an explicit target/configuration decision. No default may override a hard
compatibility constraint.

Show only relevant questions. Enabling Redis asks for its environment-variable
binding and applicable options; disabling cache does not ask for a Redis address.
Preserve unaffected answers when changing a store. Recompute affected dependent
choices and report their changes instead of silently substituting a provider.

Show why a choice is unavailable when it helps recovery, for example a provider
requiring a different runtime. Diagnostics name the target, requested variant,
constraint and feasible next action. Never choose an unrelated variant because
an update removed the selected one. Recommendations must say where they came
from; avoid unexplained preferred defaults.

Ordinary onboarding remains interactive. Agents provide the same typed intent
through the existing input/answers protocol and receive structured questions for
missing choices. `--yes` remains approval of confirmations, not a choice of
language, database, caching or conflict strategy. Changing a parent choice must
invalidate stale dependent answers/approvals while retaining unaffected intent.
The exact target/family input shape belongs to the upcoming Code-extension design;
this proposal does not add a native global `--language` flag.

## Persist requested intent separately from the effective plan

Target/profile declarations belong to project TOML under extension-owned options.
Module-specific requested roots, bindings and configuration use the existing
composition intent/receipt flow. Keep each piece of intent authoritative in one
place; do not introduce another manually maintained variant configuration file.

The effective selection records the resolved language/runtime implementation,
provider graph, dependency requirements, source/catalog identity and observed
hashes. Lock/source pins, generation receipts and native ownership retain their
existing responsibilities. Do not use CLI global cache as project selection state.

The review summary must explain the effective selection even when defaults or
detection avoided prompts. A user should be able to tell what was selected, why,
which SDK/native versions will be used, and what will change before approving.
Environment choices persist variable names, never secret values. Planning and
validation do not connect to application databases.

## Updates and switching variants

| Change | Intended handling |
| --- | --- |
| Parameter within one implementation | Incremental generation, normal diff/approval and receipt updates |
| Adapter within a compatible native interface | Rebind graph; review generated updates, obsolete files and native dependency ownership |
| Optional feature enabled/disabled | Add/retire only its active subtree; preserve unaffected instances and user edits |
| Selected implementation removed or interface incompatible | Explain the conflict; require an explicit replacement/migration or retain the previous pinned source |
| Language/runtime or catalog identity crossing the generation boundary | Initially create a new target; support in-place migration only through an explicitly implemented migration path |

A language switch is not a three-way source merge from TypeScript into Rust.
Initial delivery supports parallel targets rather than automatic in-place language
conversion. Keep old target files and user code intact. Any future migration must
identify generated ownership, manual entry/integration work, dependencies and data
separately.

Store changes can retire source and dependencies, but do not transfer database data
or authorize deleting databases. Password-provider changes must account for existing
stored credential formats. Disabling caching does not authorize flushing an external
cache. Those operations need their own explicit, supported application semantics.

A compatible update keeps stable choice IDs and saved intent. New recommendations
must not rewrite explicit choices. Report introduced dependencies, changed effective
defaults and retained project grants needing review. Approval binds the resolved
plan; changing the selected variant invalidates that approval.

## Initial acceptance requirements

Add these gates to the first module implementation:

1. Both TS and Rust entry choices reach actual native SDK consumers, not a TS-first
   implementation hidden behind a language label.
2. A workspace with both manifests asks for a target; separate target selections
   retain separate state and cannot accidentally overwrite one another.
3. A prepared target receives compatible defaults, while explicit overrides,
   incompatible runtime evidence and unavailable providers have clear outcomes.
4. Changing a provider or optional feature preserves unrelated answers, instance
   bindings, user code and dependency ownership; no application file changes occur
   while questions remain unresolved.
5. Terminal and Agent flows resolve the same effective selection. Missing language
   or provider input remains a structured question; `--yes` cannot choose it.
6. Every advertised implementation compiles against its declared published SDK and
   passes shared behavior fixtures. Recipes get complete application tests; adapters
   get interface tests. Unsupported Cartesian combinations are neither maintained
   as forks nor implicitly claimed as supported.
7. An update preserves selected variants, rejects removed/incompatible selections
   with actionable diagnostics, and exercises local customization and decline paths.
8. A requested language change leaves the existing target intact and explains the
   new-target/manual integration path. Adapter cleanup makes no data-migration claim.

Implement target/context discovery, preset-provided defaults and family routing in
M0 of the official module plan. M1 must demonstrate both language branches before
any language selector or Auth preset is presented as delivered.
