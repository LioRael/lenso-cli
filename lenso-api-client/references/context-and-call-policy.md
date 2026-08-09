# Context and Call Policy

Preserve the context declared by the Contract: Story and causation identities,
Service or Workload principal, delegated actor, tenant, region, idempotency,
and one absolute deadline. A downstream timeout cannot extend that deadline.

Apply only the declared Call Policy. Retries require a retryable failure, time
remaining before the deadline, and safe semantics or a stable Idempotency Key.
Expose native transport failures instead of translating them into success or
an unrelated framework error.

Do not rename a host-managed Provider call into an Autonomous Service call;
their authority and reliability boundaries differ.
