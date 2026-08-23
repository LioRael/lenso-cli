# Runtime seams

Use the **host-facility test**: if the concern exists because a host must make
Kernel or a Module executable, place it in the narrow Driver, Execution
Adapter, or Runner seam. If selecting the product feature creates the concern
and deleting that selection should remove it, use a Module.

| Seam | Owns | Does not own |
| --- | --- | --- |
| Runtime Driver | local task lane, scheduling, monotonic time, timers, cooperative cancellation, progress | Module factories, endpoints, product policy |
| Execution Adapter | Module generation, endpoint mechanics, execution class, isolation, process or wire translation, host-specific failure semantics | graph resolution, Capability selection, business behavior |
| Runner | Driver and Adapter catalog, root Kernel future, host shutdown translation, terminal outcome | package acquisition, App mutation, product services |
| Authoring tooling | project files, package-manager inspection, validation, code generation, Plan materialization | running graph mutation, Kernel installation state |
| Kernel | portable graph, lifecycle, invocation, admission, readiness, supervision, diagnostics | OS facilities, networks, databases, Auth, UI, transport, product policy |

## Boundary cases

- An HTTP or game Ingress Module owns protocol-facing product behavior and
  Capability projection. Socket accept loops, browser host APIs, or process
  bridges belong to the supporting Adapter or Runner.
- A database client or pool is normally a private Module persistence Adapter,
  not a global Module and not Kernel state.
- Bun child-process framing belongs to the Bun Adapter; the TypeScript business
  implementation remains a Module.
- Web Shell and UI Contributions are Modules; browser scheduling and generated
  client projection are Driver or Adapter mechanics.

Current ownership is physically split: portable Plan, Kernel, and conformance
remain in `lenso`; Rust Drivers, native Adapter, and Runner live in
`lenso-runtime-rust`; Bun integration lives in `lenso-bun-adapter`; protocol
source and code generation live in `lenso-protocols`; authoring lives in
`lenso-cli`; optional Modules live with their product owners. Verify these
locations before editing because repository ownership may evolve.
