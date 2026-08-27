# One Plugin authoring migration

Application behavior now uses one Plugin namespace. Installability no longer
creates a second identity or authoring model.

| Old | Current |
| --- | --- |
| `lenso new` or `lenso module new` | `lenso plugin new` |
| `lenso check` or `lenso module check` | `lenso plugin check` |
| `lenso dev` or `lenso module dev` | `lenso plugin dev` |
| `lenso plugin build --manifest ...` | `lenso plugin pack` |
| `lenso plugin verify --bundle ...` | no replacement; `pack` and the receiving Host validate automatically |

The first supported target is a Rust/Wasm Tool Plugin for the Agent Harness.
Legacy Module commands remain hidden only for a bounded compatibility period.
They are not the replacement for embedded behavior.

Host-linked behavior will join the same Plugin workflow through an embedded
distribution target before compatibility commands are deleted. Until that
target ships, existing built-in projects remain readable and buildable through
the hidden commands, but new application behavior should not introduce another
public Module identity.

New Plugin projects contain no Module declaration, Module Descriptor, Manifest
template, contribution array, or execution Plan.
