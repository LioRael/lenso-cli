---
name: lenso-reviewed-release
description: Use when preparing or validating a Lenso repository-local release. Follow the repository's Release-plz or Changesets workflow and keep registry evidence explicit; do not reintroduce a central coordinator.
---

# Lenso Repository Release Runbook

Prepare and verify a release in the repository that owns the package. This
repository-local runbook replaces the retired central release path. Read
`docs/release-process.md`, the accepted release ADR, and the repository's
workflow before changing release configuration.

## Safe workflow

- Run the repository quality gate and the ecosystem package dry-run.
- Review the exact release pull request, source commit, package versions, and
  dependency closure.
- Use crates.io or npm Trusted Publishing; do not add a long-lived registry
  token to the default path.
- Verify the registry version, archive checksum, tag, provenance, receipt
  evidence, and a fresh install after publication.
- Treat the registry as immutable source of truth. Repair a failed workflow
  from the current registry state without republishing an existing version.
- Do not introduce shadow publication or a second mutable release state.

## Approval boundaries

Do not perform production publication, configure a Trusted Publisher, alter
repository permissions, or change a registry package outside the user's
requested migration. A cross-repository compatibility check is not a
synchronized release and does not authorize another repository's publication.
