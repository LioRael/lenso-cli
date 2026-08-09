---
name: lenso-reviewed-release
description: Prepare, validate, publish, or recover a release of a Lenso-owned Cargo crate, npm package, CLI distribution, Console Service image, GitHub Release, or repository tag. Use when release PRs, Release-plz, Changesets, Trusted Publishing, registry drift, provenance, failed publication, or fresh-install proof are involved.
---

# Lenso Reviewed Release

Release each artifact through the repository that owns it and distinguish
source readiness, workflow authority, publication, and registry verification.

## Workflow

1. **Resolve the release owner.** Follow
   [ownership and authority](references/ownership-and-authority.md). Identify
   the repository, changed public artifacts, release unit, current branch,
   release PR, workflow, tag convention, registry, and Trusted Publisher.
   Finish when no artifact depends on a retired central coordinator.
2. **Read the repository process.** Before changing or executing a release,
   read its release process, accepted release ADR, workflow files, and package
   metadata. Environment and registry state override remembered versions.
3. **Inventory current truth.** Compare source manifests and lockfiles, release
   PR versions, public registry versions, tags, GitHub Releases, image digests,
   provenance, and consumer constraints. Finish when every public surface is
   classified as unpublished, published, drifted, or not part of the release.
4. **Prepare the reviewed change.** Use the repository's Release-plz,
   Changesets, or approved OCI path. Run the owning quality gate and ecosystem
   package or image dry-run for the changed artifact set.
5. **Cross the publication boundary deliberately.** Confirm that merging or
   dispatching the exact reviewed workflow is within the user's request and
   that required Trusted Publishers and permissions exist. A green local build
   is not publication authority.
6. **Verify publication.** Follow
   [registry evidence](references/registry-evidence.md). Verify registry bytes,
   version, checksum or digest, tag, GitHub Release, provenance or attestation,
   dependency closure, and a fresh install or pull.
7. **Recover from registry truth.** If publication is partial, preserve
   immutable versions and follow
   [failed release recovery](references/failed-release-recovery.md). Resume only
   missing artifacts through the owning workflow; never republish an existing
   version or restore the retired central runtime.

## Report

Report per artifact: owner, source commit, planned version, dry-run, workflow,
publication authority, registry result, tag or digest, provenance, fresh
install, recovery action, and remaining blocker. State commit, push, release
PR, merge, publication, and verification separately.
