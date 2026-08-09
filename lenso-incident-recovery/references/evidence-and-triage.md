# Evidence and triage

Collect identifiers and revisions before interpreting symptoms: Service and
Workload ids, Store and migration state, release and deployment artifacts,
Contract versions, configuration digest, identity and trust epoch, Story and
causation ids, Inbox and Outbox positions, Workflow instances, region, and
observation timestamp.

Separate desired state, last applied state, and observed state. Logs are
observations; receipts, Store state, registry state, and protected plans carry
different authority. Prefer the source that owns the fact.

Classify scope and behavior as continue, degraded, paused, rejected,
fail-closed, partial, blocked, or unknown. Record what evidence would change
the classification.
