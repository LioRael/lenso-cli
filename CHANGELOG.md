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

