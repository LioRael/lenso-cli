# Module boundary

A linked Module is an installable business capability that runs in the host
deployable. It owns its business data and behavior while exposing serializable
declarations through `ModuleManifest` and behavior through narrow host seams.

Keep a capability linked when it is first-party, benefits from a local
transaction, and shares the host's release and recovery boundary. Use
`lenso-service-authoring` when a host-managed out-of-process provider is the
real boundary. Use `lenso-autonomous-service-authoring` only when the capability
owns independent authority, Workloads, Stores, Contracts, and reliability.

Direct cross-Module imports and cross-Module table access are boundary failures.
Collaborate through declared dependencies, Events, public APIs, or host-owned
delivery rails.
