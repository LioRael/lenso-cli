# Identity and execution

The definition version gives durable meaning to an instance. Step and
transition identities remain stable within that version. Attempt identities
distinguish retries; effect and compensation identities make external outcomes
reconcilable.

Persist the selected transition and its context before scheduling work. Claim
work through the owning Service Store, make retries safe through stable
idempotency, and record completion before advancing dependent steps.

A child Workflow is a separately version-pinned instance. The parent waits for
one stable completion identity and resumes exactly once.
