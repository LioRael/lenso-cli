# Module verification

Use the owning repository's current focused commands. Prove:

- the Module manifest passes the current lint;
- changed handlers, data, actions, runtime functions, Events, and lifecycle
  declarations have focused tests or a runnable smoke path;
- host composition actually registers the Module;
- generated contracts are fresh when public surfaces changed;
- architecture checks preserve the Module/platform boundary; and
- the configured Console Service shows only the surfaces and evidence the
  Module actually provides.

A compile-only check is insufficient when the requested outcome is an
installed capability or visible Console behavior.
