---
name: lenso-remote-module-authoring
description: Use when building or editing an out-of-process Lenso service in JavaScript or TypeScript with `@lenso/service-kit`, including service manifests, provided modules, remote routes, runtime functions, event handlers, and custom admin surfaces.
---

# Lenso Service Module Authoring

## Overview

Use the public service kit for services that run outside the host while
providing one or more modules. The host keeps auth, queues, retries, and
visibility. Keep the service package self-contained and publishable.

## Start Here

```sh
pnpm add @lenso/service-kit@0.1.0
```

Start with the manifest:

- `defineService(...)`
- `defineModule(...)`
- `getRoute(...)`
- `runtimeFunction(...)`
- `adminAction(...)`
- `declarativeCustom(...)`
- `queryValue(...)`
- `serveService(...)`

When the service is part of a reusable app slice, create and check a capability
pack around it before composing the host:

```sh
lenso capability init support-sla --dir ./capabilities/support-sla --lang ts --for-blueprint support-desk
lenso capability library add ./capabilities/support-sla
lenso capability fit support-sla --repo-root .
lenso app compose ./acme-support --blueprint support-desk --pack support-sla --apply
```

## Host Boundaries

- Keep auth, retries, queues, and visibility on the host.
- Keep service and module manifests declarative.
- Keep capability packs as local authoring and composition metadata; service install still owns the out-of-process provider lifecycle.
- Do not depend on sibling workspace paths for examples.
- Use the host proxy and console surfaces already provided by Lenso.

## Agent Output

For a service, leave:

- a service manifest URL such as `/lenso/service/v1/manifest`
- one or more provided modules below `/lenso/service/v1/modules/{moduleName}`
- declared HTTP routes, runtime functions, event handlers, actions, query values, or custom surfaces that are actually served
- install instructions using `lenso service install <manifest-url>`
- lifecycle instructions using `lenso service list`, `lenso service status <provider> <service>`, and `lenso service doctor <module> --json`
- composed-app instructions using `lenso app next`, `lenso app explain`, and `lenso agent task --from-app-plan "add the requested business behavior"` when the service is part of an App Composer flow
- capability-pack instructions using `lenso capability library add <pack-dir>`, `lenso capability fit <pack> --repo-root .`, and `lenso agent task --for-capability <pack> "add the requested business behavior"` when the service ships as part of a pack
- a manifest `install.services` declaration when the service has a local process command
- one package or focused check that proves the service can run outside the host
- Console expectations: Modules should show the provided module installed / configured / ready, with Remote Calls and Runtime Story staying host-owned

## Checks

```sh
pnpm package-readiness
```

Use `npm pack --dry-run` before publishing a package change.

## Keep Out

- Do not make the service responsible for host auth, retries, queues, or observability.
- Do not require a sibling Lenso checkout for a publishable example.
