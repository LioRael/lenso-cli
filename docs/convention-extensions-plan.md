# Extensible App conventions

Implementation: local selection, compiler protocol, bundled CLI support, and
multi-language scaffolds are implemented. See [convention authoring](convention-authoring.md)
for supported commands, authoring examples, and current boundaries. Central
tracking: LioRael/lenso#733. Registry publication remains a separate authorized step.

## Completed implementation

1. Deterministic read-only selection uses adopted support declarations, optional
   and required surfaces, path validation, and actionable inspect diagnostics.
2. Selected independent packages retain normal contract generation, Bundle
   validation, Host admission, and offline distribution. Ownership/support
   provenance is recorded; ambiguous multiple-owner composition is rejected.
3. Versioned out-of-process compiler extensions lower standalone files to ordinary
   source packages or verified Bundles. Compiler execution and output validation
   are bounded; discovery never executes build tools. Freshness is checked before
   distribution publication.
4. Bundled CLI support recognizes Rust and TypeScript entries, including bare
   App-owned directories. Local adoption and per-language scaffolding are built
   in. Existing terminal catalog/Stream contracts, generated codecs, and typed
   Plan-bound ingress are reused; no competing command wire protocol is introduced.
5. Integration tests exercise creation, inactive malformed TUI package isolation,
   missing Rust tools, mixed native/TS command invocation, help/error shutdown,
   and source-free offline distributions. Workspace tests verify generated
   terminal projection freshness and selection behavior.

## Retained boundaries

One simple Plugin uses one language package manifest. A logical Plugin can own
several independent packages to isolate optional surfaces. The core package owns
identity/version; contributions have distinct runtime identities. Alternative
implementations of one runtime Contract remain alternatives, while additive
surfaces may run together. Neither selection nor source proximity grants runtime
permissions or merges descriptors.

No npm import, Cargo feature, or discoverable source implicitly adopts shared
support. App-owned defaults and explicit Plugin Root instances determine adoption.
Shared workspace dependencies remain the author's responsibility. Runtime-only
Root changes cannot recompute build-time selections. Advanced Host integrations
and codecs use the existing extension interfaces; arbitrary newly discovered
files do not expand the prebuilt Host's contract repertoire.
