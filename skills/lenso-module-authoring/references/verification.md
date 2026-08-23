# Module verification

Use the owning repository's narrowest meaningful check, then its required
workspace gate.

## Contract proof

- every declared endpoint exists before preparation completes;
- every required handle comes from an explicit binding;
- success, domain errors, and runtime failures preserve their contract; and
- undeclared or duplicate Operations fail closed.

## Lifecycle proof

- preparation validates configuration and required resources;
- activation opens work only after dependencies are ready;
- deactivation cancels and joins managed work;
- a restart creates a fresh generation and does not leak old resources; and
- unavailable durable state remains an honest runtime failure.

## Product proof

Exercise the smallest useful behavior through a real Capability consumer or
owned fixture. For Web behavior, cross the Browser Adapter boundary. For Bun,
cross the real process Adapter when the change affects wire or lifecycle
semantics. For a stateful Module, prove recovery or failure at its persistence
boundary.

Finally remove the Module from the test Composition. The remaining App must
still resolve when no declared requirement needs it, and the removed concern
must leave no feature hook, policy branch, background task, or mandatory
storage in Kernel.
