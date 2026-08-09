# Lenso public skills

These skills are the public agent workflow layer for Lenso. Install them from
this repository:

```sh
npx skills add LioRael/lenso
```

List the available skills without installing them:

```sh
npx skills add LioRael/lenso --list
```

Install only the workflows relevant to the current project with `--skill`.
`lenso-start` is the user-invoked router; the other skills have narrow,
model-facing descriptions so an agent can discover them from a concrete task.

## Catalog

| Skill | Use it for |
| --- | --- |
| `lenso-start` | Choose the smallest applicable Lenso workflow. |
| `lenso-business-planning` | Turn a product idea into owned capabilities and a first useful slice. |
| `lenso-app-composition` | Create or evolve a Launchpad app, blueprint, addon, or capability pack. |
| `lenso-starter-host` | Create or repair a thin runnable Lenso host. |
| `lenso-module-authoring` | Build a linked Rust Module after its boundary is known. |
| `lenso-service-authoring` | Build an out-of-process service that provides Modules. |
| `lenso-console-surface-authoring` | Create or improve a declared Console Surface and its UI artifact. |
| `lenso-api-client` | Consume a committed Lenso OpenAPI or Service Contract. |
| `lenso-autonomous-service-authoring` | Build an independently authoritative Service within the GA support surface. |
| `lenso-contract-evolution` | Add, change, deprecate, migrate, or retire a Contract. |
| `lenso-durable-workflow` | Design or evolve a versioned Durable Workflow. |
| `lenso-module-extraction` | Move a linked Module toward an Autonomous Service. |
| `lenso-incident-recovery` | Diagnose an incident and prepare the smallest safe recovery action. |
| `lenso-reviewed-release` | Prepare, validate, or recover a repository-owned release. |

## Authoring rule

Each `SKILL.md` owns the ordered workflow and completion criteria. Branch-only
material lives in that skill's `references/` directory. Current commands,
versions, package exports, and repository scripts remain environment-owned:
inspect them at execution time instead of copying them into a skill.
