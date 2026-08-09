# Service model

One logical Service has a stable identity across replicas and deployments.
Workloads declare execution roles; they do not become independent business
authorities. API, Worker, and Migration Workloads share the Service boundary
while retaining separate health and rollout evidence.

Keep business tables, Inbox, Outbox, Workflow, timer, Story Segment, health,
and migration state in Service-owned Stores. Cross-Service work travels through
versioned Contracts and durable delivery. Local transactions end at the Store
boundary.

Declare how the Service behaves when each dependency is slow, unavailable,
stale, or unauthorized. A degraded mode states which work continues, pauses,
rejects, or fails closed and which evidence proves the decision.
