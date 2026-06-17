---
name: lenso-remote-module-authoring
description: Use when building or editing an out-of-process Lenso module in JavaScript or TypeScript with `@lenso/remote-module-kit`, including manifests, remote routes, runtime functions, event handlers, and custom admin surfaces.
---

# Lenso Remote Module Authoring

## Overview

Use the public remote-module kit for modules that run outside the host.
Keep the module package self-contained and publishable.

## Start Here

```sh
pnpm add @lenso/remote-module-kit@0.1.1
```

Start with the manifest:

- `defineRemoteModule(...)`
- `getRoute(...)`
- `runtimeFunction(...)`
- `adminAction(...)`
- `declarativeCustom(...)`
- `serveRemoteModule(...)`

## Host Boundaries

- Keep auth, retries, queues, and visibility on the host.
- Keep remote manifests declarative.
- Do not depend on sibling workspace paths for examples.
- Use the host proxy and console surfaces already provided by Lenso.

## Checks

```sh
pnpm package-readiness
```

Use `npm pack --dry-run` before publishing a package change.
