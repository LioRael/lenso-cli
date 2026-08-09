# Behavior and collaboration

Keep validation, authorization decisions owned by the capability, explicit
storage, business transitions, and emitted evidence close together. Platform
crates may expose reusable seams but must not absorb one Module's product rules.

Choose collaboration by meaning:

- declared dependency for required capability presence;
- direct public API for synchronous authority checks;
- Event for durable notification or projection;
- host-owned queue, Outbox, or proxy for delivery; and
- generated Contract client across a Service boundary.

State failure behavior for missing dependencies, duplicate Events, retries,
and unavailable collaborators. Local transactions stop at the owning data
boundary.
