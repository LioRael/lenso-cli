# Module shapes

Read only the branch that matches the selected Execution Adapter and product
responsibility.

## Native Rust

Compile the Module as an ordinary Cargo dependency and supply its factory to
the native Execution Adapter. App Composition remains the authority for which
linked factories become Instances. Prefer typed local dispatch; use generated
portable contracts when another supported Adapter must interoperate.

## Bun process

Treat the Bun implementation as an ordinary Module executed by the Bun
Adapter. Keep process startup, handshake, framing, bounds, cancellation, and
failure translation in the Adapter. Keep domain behavior and Capability
provider logic in the Module package. Do not recreate Service or Provider as a
peer product type.

## Web and UI

Model target Web behavior with ordinary Modules and Capabilities. UI
Contributions provide `lenso.ui.contribution@1`; a Web Shell consumes many
contributions; selected portable business requirements receive generated
browser clients through the Browser Adapter. Web Ingress owns protocol-facing
behavior, while socket and host endpoint mechanics remain outside Kernel.

A cross-App operator Console is its own Lenso App only when it has an
independent trust domain, target set, durable state, or release lifecycle. It
does not create a Console Module kind.

## Stateful

The Module owns data meaning, schema, migrations, transaction and recovery
semantics. It either contains a private persistence Adapter or requires a deep
semantic persistence Capability. Shared infrastructure never grants another
Module access to its tables, and storage failure never falls back silently to
ephemeral memory.

## Cross-cutting

Auth, Secrets, Story, Audit, OpenTelemetry, Workflow, Outbox, health, and
similar concerns are optional Modules when selected product behavior needs
them. They expose deep Capabilities or stay owner-local. Removing their package
and Composition entry removes their tasks, state, policy, and operational
surface while Kernel mechanics remain unchanged.
