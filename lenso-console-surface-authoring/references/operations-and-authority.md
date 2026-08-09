# Operations and authority

Surface entry capability permits navigation; it does not grant every operation
inside the page. Reauthorize reads and mutations at their execution boundary.

Every Managed Service operation carries one explicit context. Include the
selected Service identity and the context fields required by the current
public API. A context switch invalidates records, capabilities, actions,
configuration, inventory, and pending work from the previous Service.

Model at least these states where applicable:

- entry denied;
- read allowed but mutation denied;
- Managed Service unavailable;
- stale or revoked access;
- action pending, succeeded, failed, or needs restart; and
- contribution removed while the owning Surface remains usable.

Secret values remain target-owned. Configuration operations stay inside the
declared Module namespace and descriptor contract.
