---
name: lenso-api-client
description: Use when consuming, testing, or generating clients for Lenso HTTP APIs from the committed OpenAPI contract, especially from a blank project or when verifying request and response shapes.
---

# Lenso API Client

## Overview

Use the committed OpenAPI document as the client source of truth.
Do not infer request or response shapes from old examples.

## Start Here

- OpenAPI: `contracts/openapi/app-api.v1.yaml`
- Errors: `contracts/errors/error-response.v1.schema.json`

## Typical Uses

- Generate a typed client
- Verify endpoint paths and payloads
- Check the standard error envelope
- Confirm admin/runtime and schema-admin endpoints

## Guardrails

- Treat the committed contract as authoritative.
- Keep generated client code out of hand edits.
- Update backend sources first, then regenerate contracts.
- Prefer verifying the exact path and envelope before writing wrapper code.

## Agent Output

When consuming an API, leave:

- the contract path used
- the endpoint and method
- the request and response shapes
- one focused command or assertion that verifies the integration

## Checks

```sh
just generate
just generated-check
```

## Keep Out

- Do not infer endpoints from README examples.
- Do not hand-edit generated clients.
