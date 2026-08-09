# Composition choices

Use a blueprint when creating the initial product shape. Use an addon when a
built-in optional capability extends an existing blueprint. Use a capability
pack when Modules, Services, seed inputs, documentation, and an agent handoff
must travel together across applications.

For an existing app, prefer a planned change over recomposition. Inspect and
diff first so generated ownership is visible before applying an upgrade or
repair.

Composition chooses and wires capabilities. It does not authorize the agent to
invent missing business behavior inside generated files.
