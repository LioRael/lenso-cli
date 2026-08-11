# App verification

Verify the application through the same public lifecycle its users follow:

- validate the exact `lenso.app.json` revision, release digests, dependency
  selections, and implementation bindings;
- run the App through `lenso system dev` and its typed Local Control Adapter;
- connect the exact topology and Management Binding to the separate Console
  Service;
- complete one real Business API workflow through a receipt-bound
  `console_ui_esm` Surface and its generated client;
- confirm direct System, Service, Module, Surface, and Workload states with
  reasons; and
- complete one supported local Workload control round trip, then confirm an
  unavailable Adapter reports unknown state and rejects mutation.

Use the installed CLI's current validation commands and the owning example's
integrated acceptance. Verification is stale when `lenso.app.json`, a bound
release, or the relevant implementation changes after the run.
