# Service boundary

A host-managed service is an out-of-process provider of one or more Modules.
It may use another language and release independently, but the host still owns
installation state, authentication at the host boundary, delivery rails,
retries, and Console visibility.

An Autonomous Service owns a stable Service identity, independent Workloads,
Service Stores, Contracts, reliability, deployment evidence, and recovery
boundary. Route that work to `lenso-autonomous-service-authoring`.

Do not create an out-of-process boundary merely to organize code. Start with a
linked Module when independent operation and ownership are not yet real.
