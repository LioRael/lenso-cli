# Module versus Service

Choose a linked Rust Module for a first-party capability that ships with the
host, benefits from local transactions, and does not yet need an independent
operational boundary.

Choose an out-of-process service that provides Modules when it has an
independent team or release lifecycle, uses a different language, represents a
trust boundary, already exists outside the host, or needs independent scaling
and failure isolation.

Choose an Autonomous Service only when it owns its authority, Workloads,
Service Store, Contracts, reliability profile, and recovery boundary. A
host-managed service provider is not an Autonomous Service.

Use a capability pack when several Modules, Services, seed inputs, and agent
handoffs must be reused as one app-level composition. A pack describes
composition; Module and Service installation still own runtime state.
