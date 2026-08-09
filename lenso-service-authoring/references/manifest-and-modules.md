# Manifest and provided Modules

Use the current Service Kit exports and generated scaffold as the API source of
truth. A service manifest identifies the provider and its process-level
contract. Each provided Module has its own manifest surface below the service
boundary.

For every declaration, verify the implementation is served at the advertised
path. Keep stable provider, service, Module, route, action, runtime, and Event
identities stable across restarts and releases. Omit empty optional sections.

When the service has a local process command, declare it through the current
install contract so app composition and lifecycle commands do not rely on an
undocumented shell step.
