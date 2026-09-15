# Release Process

This repository releases the Cargo CLI and its npm distribution independently.
There is no repository-wide release plan, shadow registry, central publisher,
nonce, or receipt channel.

## Cargo crate

Release-plz runs on pushes to `main`:

1. `release-pr` opens or updates a release pull request for changed workspace crates.
2. `release` publishes the version from a merged release pull request through
   crates.io Trusted Publishing and creates a `<package>@<version>` tag.

Merge the generated PR with its `release` label intact, keep its
`release-plz-` source-branch prefix, and do not customize the final squash
subject. Release-plz verifies that the `main` commit is associated with a PR
whose source branch has that prefix; a generic recovery PR can therefore
produce a successful workflow that correctly skips publication. For a
one-commit recovery PR, use the same branch prefix and name the commit
`chore: release` so GitHub preserves the generated release-PR identity.

The crates.io registry is the source of truth for existing versions. Public
versions, tags, and the historical `CHANGELOG.md` are not rewritten. Configure
a crates.io Trusted Publisher for each package (`lenso-cli` and
`lenso-plugin-catalog`) before its first live publish
after this migration; no long-lived `CARGO_REGISTRY_TOKEN` is used.

## npm distribution

Create a changeset for every user-facing npm distribution change:

```sh
pnpm changeset
```

The Changesets workflow creates a version pull request. After it is merged,
the workflow builds `darwin-arm64`, `darwin-x64`, `linux-x64`, and `win32-x64`
artifacts, verifies the npm payload, and publishes `@lenso/cli` through npm
Trusted Publishing. The Cargo and npm versions are separate streams; the npm
wrapper may publish a packaging-only change without forcing a Cargo release.

Configure an npm Trusted Publisher for `@lenso/cli` before the first live
publish after this migration. The workflow uses the checked-in binary payload,
not a long-lived `NPM_TOKEN`.

## Local checks

```sh
pnpm install --frozen-lockfile
pnpm changeset status --output /tmp/lenso-cli-changesets.json
npm run check:npm-shim
cargo fmt --all -- --check
cargo test --locked --workspace
cargo check --locked -p lenso-plugin-catalog --target wasm32-unknown-unknown
cargo metadata --locked --format-version 1
cargo package --locked -p lenso-plugin-catalog --allow-dirty
cargo package --locked --workspace --allow-dirty --no-verify
cargo publish --dry-run --locked -p lenso-plugin-catalog --allow-dirty
cargo publish --dry-run --locked --workspace --allow-dirty --no-verify
```

The portable catalog package is verified independently. Workspace packaging stages
local packages in dependency order, so the CLI payload can be inspected before a
new catalog version exists on crates.io. The workspace test verifies the CLI's
native Bundle integration; the separate Wasm check verifies the catalog's default
feature set without that integration. Release-plz publishes changed workspace
crates in dependency order. The catalog package is new and remains unpublished
until explicit publication approval and its Trusted Publisher are in place.

To inspect an npm archive locally, build the current platform payload first:

```sh
npm run package:npm
npm run check:npm-publish
npm pack --dry-run --ignore-scripts
```

Cross-repository compatibility is proven by SemVer requirements, contracts,
and focused integration checks. Do not restore the retired `lenso-release`
runtime or a shared release channel to coordinate the two package streams.

## Generated release PR checks

Release PRs use the repository `GITHUB_TOKEN`; no dedicated Release App or
central coordinator is required. Keep Actions pull-request creation enabled and
retain the workflow's scoped `contents: write` and `pull-requests: write` permissions.
Registry publication continues to use the repository's own OIDC workflow.

GitHub places workflows for `github-actions[bot]` pull requests behind an
[approval gate](https://github.blog/changelog/2026-06-11-bot-created-pull-requests-can-run-workflows-if-approved/).
During delivery:

1. Review the generated version/lockfile changes and record the PR's current head SHA.
2. Open the pending CI run for that same head and use **Approve and run**.
3. Wait for all required checks on the reviewed head before merging. If the bot
   updates the PR, review the new head and approve its pending runs again.

`action_required` and an expired approval are delivery blockers, not executed
test failures. Approving CI does not approve a merge or publication. Do not bypass
required checks or dispatch a publisher to compensate for a pending PR approval.

## Publication event recovery

If an authorized release merge does not produce the expected `main` publisher
run, dispatch the same reviewed Trusted Publisher workflow against `main`
instead of creating an empty commit or using a local registry token:

```sh
gh workflow run release-plz.yml --ref main
gh workflow run release-changesets.yml --ref main
```

Inspect the exact `main` commit and public registry state before dispatching.
The manual entry points run the same jobs, permissions, package checks, and OIDC
publish steps as the normal push path.
