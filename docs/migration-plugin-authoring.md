# Plugin authoring migration

Installable Harness extensions now use one Plugin namespace. Module authoring
remains available for behavior intentionally compiled into an App Host.

| Old | New |
| --- | --- |
| `lenso new` | `lenso module new` for built-ins, or `lenso plugin new` for installable Harness extensions |
| `lenso check` / `lenso dev` / `lenso verify` | matching explicit `lenso module` namespace for built-ins |
| `lenso plugin build --manifest ...` | `lenso plugin pack` |
| `lenso plugin verify --bundle ...` | no normal replacement; `pack` and Harness `plugins add` validate automatically |

The old top-level Module commands and template-based Plugin commands remain
hidden compatibility aliases for one release window and print an actionable
warning. New Plugin projects contain no Module declaration, Module Descriptor,
Manifest template, contribution array, or Plan.
