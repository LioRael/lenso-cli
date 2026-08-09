# Host-managed responsibilities

The host authenticates callers at its public boundary, evaluates installation
and capability state, owns delivery policy and retries, and records Provider
Calls and Runtime Story evidence. The service implements the business behavior
and returns native transport failures without pretending host policy succeeded.

Preserve request identity, tenant, actor, deadline, idempotency, causation, and
Story context supplied by the host. Do not mint replacement context or retry
past the host's absolute deadline.

The service may expose operational health for its own process. It does not own
the host's Module registry, installation receipts, or Console navigation.
