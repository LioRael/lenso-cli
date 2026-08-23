# Contract shape

## Deep role Interface

Describe the role a consumer needs, not the provider's storage model, package
version, transport, process, or concrete type. A cohesive Module may provide
several Capabilities. Capability Descriptor versions evolve independently from
Module package versions.

## Interaction kinds

- **Request** has one terminal success, domain error, or runtime failure.
- **Stream** is bidirectional, bounded, cancellable, independently
  half-closable, and ends in one explicit terminal outcome.
- **Event** attempts independent bounded admission for every bound subscriber
  and reports partial outcomes. It does not imply persistence, replay,
  redelivery, ordering across subscribers, or exactly-once delivery.

Use a semantic State, Secrets, Auth, Story, Audit, or similar Capability when
another Module truly needs that role. Keep private helpers, tables, database
pools, HTTP routes, and process protocols out of the public contract.

## Portable source

A portable Capability uses a runtime-neutral Descriptor plus package-local
JSON Schema 2020-12 files. Preserve the portable value profile:

- wide integers, bytes, timestamps, and durations use their declared string
  encodings;
- missing and explicit null remain distinct;
- unknown domain-error codes and payloads remain representable; and
- shapes that discard wire data fail generation.

Generated Rust, TypeScript, or browser artifacts are projections of that one
source, never parallel handwritten contracts.
