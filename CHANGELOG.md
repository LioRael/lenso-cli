## lenso-cli@0.2.16

## 0.6.2

### Patch Changes

- 19822d5: Give generated TypeScript Services enough time to pass dependency safety checks during a cold local startup.

## 0.6.1

### Patch Changes

- 6349d38: Point generated Host and Service guidance at the workspace-driven `lenso dev up` golden path.

## 0.6.0

### Minor Changes

- e6095ea: Install exact workspace Provider releases during `lenso dev up` and keep generated Service manifests package-compatible.

## 0.5.0

### Minor Changes

- 2ff0152: Add a public `lenso dev up --console-root` golden path that prepares and owns the local Host, Services, Console, Operator, enrollment, UI artifacts, and exact System connection. Generated Hosts and Services now resolve current compatible published releases by default, local framework dependencies are explicit, Provider metadata is route-complete, npm binary permissions self-repair, and Console Operator authority persists in the Console Access store.

## 0.4.0

### Minor Changes

- 40c27c2: Generate Provider V1 TypeScript services, install exact Provider Module releases into Host runtime inputs, preserve qualified Service identities in local plans, configure complete Operator scopes, and apply signed Console connection bundles through one public command.

## 0.3.0

### Minor Changes

- 5ca5c67: Simplify the public application lifecycle to Compose, Run locally, Connect, and
  Status. Remove the retired App Plan, Apply, Verify, Diff, Repair, Next, Upgrade,
  and Explain commands; the App Compose `--write-plan`, `--explain`, and `--addon`
  options; and the retired System Init, AddService, AddModule, Plan, Diff, Apply,
  Doctor, Release, Runbook, and Graph commands. Keep `app compose --apply` only as
  one atomic materialization flag, not a lifecycle stage.
- dfb1168: Add a typed local Workload Control adapter for observing, suspending, and resuming managed workloads with fail-closed process ownership and local credential handling.

## 0.2.17

### Patch Changes

- 8425caa: Publish the TypeScript-compiled npm CLI shim and its current Console product naming.

### Fixes

Use the current Console product name in the CLI launchpad instead of the
retired Runtime Console name.

## lenso-cli@0.2.15

### Fixes

Preserve executable modes on the bundled Unix CLI binaries when an npm release
is rebuilt through the reviewed partial-recovery path.

## @lenso/cli@0.2.14

### Features

Publish independent Lenso Console installation, upgrade, recovery, backup, and
Operator bootstrap operations through the universal CLI distribution.

## lenso-cli@0.2.13

### Fixes

Forward termination signals from the npm shim to the bundled CLI so long-running
commands stop their Workloads and clean owned state.

## lenso-cli@0.2.12

### Fixes

Publish the universal CLI fixed group after removing the unrelated hosted
Console step that blocked the Windows binary build before registry writes.

## lenso-cli@0.2.11

### Fixes

Publish universal platform binaries through the reviewed coordinator flow so
the exact staged CLI package is consumable by the M6 fresh-starter proof.

## lenso-cli@0.2.10

### Fixes

Publish the completed M6 candidate tracer against the exact reviewed support
manifest and accepted release evidence.

## @lenso/cli@0.2.9

### Fixes

Publish the M6 General Availability CLI with canonical Environment
Verification evidence.

## lenso-cli@0.2.8

### Fixes

Publish the M6 CLI distribution with Cargo packaging staged from the reviewed
publisher workspace.

## lenso-cli@0.2.7

### Fixes

Publish the M6 CLI distribution with deterministic publisher dependencies
excluded from Cargo workspace dirtiness checks.

## lenso-cli@0.2.6

### Fixes

Publish the M6 CLI distribution with publisher authentication scoped to both
the component repository and release coordinator.

## @lenso/cli@0.2.5

### Fixes

Publish the M6 CLI distribution with diagnosable fail-closed coordinator
preflight rejections.

## lenso-cli@0.2.4

### Fixes

Publish the M6 CLI distribution with its current `lenso-service` dependency
recorded in the reviewed release graph.

## lenso-cli@0.2.3

### Fixes

Publish the M6 CLI distribution with full Git history available to the
fail-closed source-ancestry verification.

## lenso-cli@0.2.2

### Fixes

Publish the M6 CLI distribution after scoping the shadow npm registry to the
sealed publish command instead of the publishing toolchain bootstrap.

## lenso-cli@0.2.1

### Fixes

Publish the M6 CLI distribution through the coordinator-scoped release workflow
after replacing the stale publisher contract.

## lenso-cli@0.2.0

### Features

Add the M6 GA support manifest, environment verification, migration, rollback,
retirement, recovery, and coordination-outage command surfaces.
