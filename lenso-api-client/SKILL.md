---
name: lenso-api-client
description: Consume, test, or generate a client for a Lenso Host, Module, Provider, or Autonomous Service API from committed OpenAPI, Protobuf, binding, context, and error Contracts. Use when endpoint shape, generated code, deadline, idempotency, Story, identity, tenant, causation, or Call Policy must remain contract-correct.
---

# Lenso API Client

Build integrations from the committed Contract artifact and preserve the
caller context and transport semantics declared by that Contract.

## Workflow

1. **Resolve the authority.** Follow
   [contract discovery](references/contract-discovery.md). Identify the owning
   Service or host, exact Contract artifact and version, binding, generated
   client owner, and supported component combination. Finish when examples and
   README prose are no longer being used as the schema source.
2. **Inspect the operation.** Read the exact path or RPC, input, output, error,
   authentication, context, deadline, idempotency, and transport declarations.
   Finish when required versus optional fields and native failure behavior are
   explicit.
3. **Preserve context.** Follow
   [context and call policy](references/context-and-call-policy.md). Carry one
   absolute deadline and the declared identities across hops. Finish when no
   wrapper silently resets context or widens authority.
4. **Generate rather than hand-copy.** Follow
   [generated clients](references/generated-clients.md). Use a generator
   compatible with the consumer language and keep custom policy outside
   generated files.
5. **Integrate at one boundary.** Add the smallest adapter or call site that
   exposes native success and failure semantics. Do not create a generic client
   layer before two real consumers require it.
6. **Verify the Contract.** Follow [verification](references/verification.md).
   Finish when generation is reproducible, one real or fixture-backed call
   checks the envelope, and deadline, retry, error, and context behavior are
   asserted for the changed operation.

## Report

Return the Contract and binding paths, exact operation and version, generated
files, preserved context, effective Call Policy, focused verification, and any
unsupported component combination.
