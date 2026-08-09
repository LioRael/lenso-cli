---
name: lenso-start
description: Choose a Lenso development workflow.
disable-model-invocation: true
---

# Lenso Start

Route the user's goal to the smallest public Lenso workflow. This skill is a
human-invoked index; the selected authoring skill owns the implementation.

## Route

1. State the requested outcome in one sentence.
2. Choose exactly one primary route:
   - unclear product idea or capability boundary -> `lenso-business-planning`
   - blueprint, addon, capability pack, or generated app -> `lenso-app-composition`
   - blank or broken host shell -> `lenso-starter-host`
   - linked Rust Module -> `lenso-module-authoring`
   - out-of-process service that provides Modules -> `lenso-service-authoring`
   - declared Console Surface or `console_ui_esm` -> `lenso-console-surface-authoring`
   - OpenAPI, Protobuf, or generated client -> `lenso-api-client`
   - independently authoritative Service -> `lenso-autonomous-service-authoring`
   - Contract compatibility or retirement -> `lenso-contract-evolution`
   - Durable Workflow definition or evolution -> `lenso-durable-workflow`
   - linked Module extraction -> `lenso-module-extraction`
   - live failure diagnosis or recovery plan -> `lenso-incident-recovery`
   - crate, npm, image, tag, or repository release -> `lenso-reviewed-release`
3. Name a secondary skill only when the request genuinely spans its boundary.
4. Tell the user the next observable result, not a copied version or command.

The route is complete when one primary skill and its expected completion
evidence are unambiguous. If two routes still compete, ask only the boundary
question that separates them.
