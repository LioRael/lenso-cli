---
name: lenso-api-client
description: Use whenever consuming, testing, or generating clients for Lenso Host or Autonomous Service APIs from committed OpenAPI or Protobuf Contracts, especially when preserving deadline, idempotency, Story, identity, tenant, and Call Policy behavior.
---

# Lenso API Client

## Overview

Use the committed Contract artifact as the client source of truth.
Do not infer request, response, context, or retry behavior from old examples.

## Start Here

- OpenAPI: `contracts/openapi/app-api.v1.yaml`
- Autonomous Service HTTP bindings: `contracts/services/*-http.v1.bindings.json`
- Autonomous Service Protobuf: `contracts/services/*.proto`
- Common context: `contracts/context/lenso-context.v1.schema.json`
- Errors: `contracts/errors/error-response.v1.schema.json`

## Typical Uses

- Generate a typed client
- Generate a Service client from an exact Contract Version
- Verify endpoint paths and payloads
- Check the standard error envelope
- Confirm admin/runtime and schema-admin endpoints
- Preserve one absolute Deadline, Idempotency Key, Story Context, Service
  Principal, delegated actor, tenant, causation, and region
- Apply the declared protocol-neutral Call Policy without hiding native
  transport failures

## Guardrails

- Treat the committed contract as authoritative.
- Check the exact component and Contract combination against the GA Support Manifest.
- Keep generated client code out of hand edits.
- Update backend sources first, then regenerate contracts.
- Prefer verifying the exact path and envelope before writing wrapper code.

## Agent Output

When consuming an API, leave:

- the contract path used
- the endpoint and method
- the request and response shapes
- one focused command or assertion that verifies the integration
- the preserved context and effective Call Policy

## Checks

```sh
just generate
just generated-check
```

## Keep Out

- Do not infer endpoints from README examples.
- Do not hand-edit generated clients.
- Do not retry past an absolute Deadline or retry an unsafe call without a
  declared Idempotency Key.
- Do not rename a Host-managed Provider client into an Autonomous Service
  client.
