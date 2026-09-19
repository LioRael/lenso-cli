# Plugin and Capability authoring DX

The App workflow removes manual Host assembly. The next authoring layer keeps one
Capability authority and existing generated Provider/Client APIs while removing
manual generation steps. No filesystem convention grants permissions or chooses
between ambiguous providers.

## Delivery order

1. Synchronize local contract snapshots/projections before `app build`/`app dev`
   compiles selected Plugins. Reuse package contract metadata and released codegen.
   Rust source extraction remains separate from compiling a stale consumer.
   Descriptor-first Bun projects do not acquire a Cargo requirement.
2. Maintain a typed mixed-language example: a domain Capability, native consumer,
   Bun provider, and generated clients. Avoid dynamic ToolProvider JSON for stable
   business-to-business calls; expose tools separately when required.
3. Provide creation defaults for common authoring layouts. Web-only filesystem
   routing is a later extension; it must stay inside its owning Plugin. General
   Plugin lifecycle, identity, dependency roles, and contract versions remain
   explicit.

## Acceptance

- Existing package declarations and custom Host paths continue working.
- Source changes update projections before consumers compile; unchanged output
  is not rewritten, preventing watcher feedback loops.
- Invalid contracts, conflicting output ownership and incompatible changes fail
  before any generated output is installed or running App replaced.
- Shared contracts are generated only from explicitly selected local inputs.
- Contract identity/version stays authored; generation never bumps it implicitly.
- Source-first Rust and Descriptor-first TypeScript both have real consumer proof.
- Distribution startup never performs generation or requires authoring tools.

The integration will document any extraction convention required by existing Rust
packages rather than infer or run arbitrary regeneration scripts. Generated files
are reviewable; accepted local development snapshots are diagnostic baselines, not
a replacement for released compatibility checks.
