# Manifest and provided Modules

Use the current Service Kit exports and generated scaffold as the API source of
truth. The live `lenso.provider.v1` descriptor identifies one exact Service
release and its exported exact Module releases. A legacy Service manifest may
describe local process startup, but it is not a runtime Module discovery or
digest authority.

For every declaration, verify the implementation is served at the advertised
path. Keep stable provider, service, Module, route, action, runtime, and Event
identities stable across restarts and releases. Omit empty optional sections.

Bind every export to the same Module Manifest, Module Release digest, Manifest
digest, operation Contract digests, and Service Release digest used by the App
and Host runtime. Provider invocation, recovery, and acknowledgement must fail
closed when any locked identity differs.

When the Service has a local process command, declare it through the current
workspace contract so local running does not rely on an undocumented shell
step. Business reads and mutations stay in the Module Business API; retired
generic Admin Data, query, command, and `admin_action` shapes are not valid new
authoring targets.
