# Module-first Lenso skills

These public skills follow the vNext product model on `main`: selected product
behavior is built from ordinary Modules, while Capabilities, App Composition,
and the runtime seams each keep one distinct responsibility.

```text
product outcome
      |
      v
Module map -> Capability contracts -> Module implementations
                                      |
                                      v
                         App Composition -> Resolved App Plan
                                      |
                                      v
                         Driver + Execution Adapters
```

“Everything is a Module” is the product rule, not a reason to turn every
technical object into one. A Module owns removable product behavior. A
Capability is its collaboration Interface. App Composition selects Instances
and bindings. Runtime Drivers, Execution Adapters, Runners, and the portable
Kernel make those Modules run.

| Skill | Use it for |
| --- | --- |
| `lenso-start` | Explicitly route a request to one primary vNext workflow. |
| `lenso-business-planning` | Turn a product outcome into a vertical Module map. |
| `lenso-capability-authoring` | Design or evolve a versioned Capability contract and bindings. |
| `lenso-module-authoring` | Implement any Module shape, including Rust, Bun, Web, stateful, and cross-cutting Modules. |
| `lenso-app-composition` | Select packages and keyed Module Instances, bind Capabilities, and resolve the Plan. |
| `lenso-runtime-extension` | Extend the Driver, Execution Adapter, Runner, or host mechanism that executes Modules. |

The old Service, Provider, Host, Console Surface, and API-client workflows are
not peer vNext authoring models. Out-of-process behavior, UI Contributions,
Auth, State, Story, Audit, OpenTelemetry, Web ingress, and similar product
concerns route through Module authoring. Generated consumers and providers
route through Capability authoring. Process, transport, and endpoint mechanics
route through runtime extension.

Install this catalog from its owning repository with:

```sh
npx skills add LioRael/lenso-cli --list
```
