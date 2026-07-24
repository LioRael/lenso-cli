---
name: lenso-reviewed-release
description: Use whenever preparing, validating, shadowing, diagnosing, or recovering a Lenso component release. Read the authoritative release runbook and exact reviewed plan; never manually dispatch protected publishers or cross production publication boundaries without explicit approval.
---

# Lenso Reviewed Release

Prepare and verify the reviewed component release while keeping release
authority explicit.

## Authoritative Inputs

Read, in order:

1. the current Lenso release runbook;
2. the exact GA Support Manifest and component catalog;
3. repository release configuration and secret names, never secret values;
4. the reviewed release plan, generated lock, source/release commits, package
   set, dependency order, digests, changelogs, Policy Evidence, and publisher
   revision.

## Safe Workflow

- Validate plan integrity and current source state.
- Run the normal coordinator in shadow mode.
- Verify exact npm, Cargo, GitHub, provenance, SBOM, receipt, and attestation
  bytes.
- Treat changed plan, digest, ref, nonce, package set, or publisher revision as
  invalid approval.
- Recover a missing receipt idempotently without republishing an immutable
  version.
- Treat mismatched remote bytes as a supply-chain incident.
- Report `ready`, `blocked`, `partial`, `published`, `receipt-pending`, or
  `complete` with exact next actions.

## Approval Boundaries

Stop before production mode, public registry publication, channel promotion,
break-glass action, or temporary workflow changes. Ticket approval and
repository write access are not release authority. Never manually dispatch or
parameterize the protected publisher and never weaken preflight, digest, ref,
nonce, receipt, or attestation checks.
