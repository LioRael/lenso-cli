# Host boundary

The host owns composition: process entrypoints, shared infrastructure wiring,
configuration anchors, Module and Service registration, and deployment-facing
integration.

Modules own business declarations and behavior. Services own their
out-of-process implementation. The Console Service remains independently
installed; a managed host exposes the contracts and evidence it consumes but
does not absorb the Console web application.

A host change is justified when every installed capability needs the seam or
when composition cannot be expressed by an existing public facade. A helper
needed by one business capability belongs with that capability.
