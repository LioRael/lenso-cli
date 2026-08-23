# Planning output

Return one compact implementation handoff.

## Outcome

- actor and useful result
- first success and honest failure
- authoritative facts and trust boundary

## Module map

For each Module, record:

- responsibility and deletion boundary
- owned facts and lifecycle
- provided and required Capabilities
- execution needs without choosing an unnecessary process boundary

## Capability map

For each cross-Module edge, record:

- Capability role and owning contract package
- consumer, eligible providers, and cardinality
- request, stream, or event Operations needed by the first slice

## First executable slice

- keyed Module Instances
- explicit bindings and required configuration or secret references
- success, failure, and observable evidence
- primary implementation skill for each remaining owner

The handoff is incomplete if a fact has multiple owners, a dependency is only
a code import, or the proof requires an undeclared global registry.
