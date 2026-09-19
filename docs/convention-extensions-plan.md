# Extensible App conventions

Status: implementation in progress; proposed APIs are not release promises.

## Boundaries

A logical product Plugin may own several build packages. Alternative
implementations of one runtime Contract remain alternatives; optional surfaces
are separate contributions and may run together. Runtime descriptors keep exact
identities, capability requirements, and execution classes. A surface never
inherits permissions merely because its owner was selected.

Simple projects retain Cargo/package metadata. A composite `plugin.json` names
an existing core package and optional surface package paths; the core package
remains the sole authority for product identity and version. Surface packages
carry distinct runtime identities. This avoids merging unrelated descriptors
or duplicating the core identity into another manifest.

## Delivery sequence

1. Read-only, deterministic convention selection before any package manager:
   versioned support declarations, optional/required surfaces, conflict and path
   validation, selected-only package metadata, inspect diagnostics.
2. Feed selected contributions through existing contract generation, Bundle
   validation, Host admission, dev rebuild, and offline distribution. Retain
   parent/support provenance. Reject ambiguous instance composition.
3. Add a versioned out-of-process compiler capability for custom entry lowering,
   with bounded request/response, explicit inputs/outputs, deterministic ownership,
   and source freshness validation. Discovery never executes compiler code.
4. Ship CLI support with Rust and Bun entry lowering, scaffolds, and local adoption
   commands. TUI fixture proves unselected dependencies are never resolved.
5. Real mixed-language and no-Cargo development/build/offline smoke, followed by
   repository validation and review. Publication is a separate authorized step.

No npm package, Rust feature, or source import implicitly adopts a support
Plugin. App-owned defaults and explicit Plugin Root instances determine support.
An inactive optional surface is explained; a required surface without support
fails before compilation. Overlapping recognition fails deterministically.

Dependency isolation requires independent packages. A shared Cargo workspace or
npm workspace can still resolve unrelated dependencies; isolated surface packages
must not be mandatory members of the selected package's dependency closure.

## Initial metadata

A support package adds `conventions` to its existing Lenso metadata:

```json
{"conventions":[{"id":"example.cli","entries":["cli.rs","cli.ts"]}]}
```

A product adds `surfaces` in the same metadata, or uses a composite manifest:

```json
{
  "schema": "lenso.plugin-project.v1",
  "core": "core",
  "surfaces": [
    {"entry":"cli/cli.ts","project":"cli"},
    {"entry":"tui/tui.rs","project":"tui","required":false}
  ]
}
```

These declarations initially select existing ordinary Plugin packages. Filename
recognition alone does not yet synthesize a Plugin implementation. The compiler
capability in step 3 is required for the shorthand authoring syntax.

## Current implementation evidence

Steps 1 and 2 now have an initial package-selection implementation: composite
projects, adopted support declarations, optional/required independent surface
packages, source provenance, read-only `app inspect`, and pre-publication selection
freshness. Bun's optional `lenso.source` selects a package-local authoring entry.
Multiple active owner instances are rejected in this initial profile.

The real Bun smoke adopts a shared owner once, includes its selected contribution,
leaves an invalid inactive Cargo package unread, shadows cargo/rustc with failing
commands, removes all business/contract sources, and starts the offline output.
The ordinary workspace regression suite also passes.

Steps 3 through 5 are not complete. In particular, the existing terminal provider
Contract uses Stream, while the local generic Bun builder and portable codec only
support Request. Shorthand CLI entry lowering must address that existing Contract
rather than introduce a competing request-only command protocol. Registry support
installation and automatic per-language package scaffolding are also outstanding.
