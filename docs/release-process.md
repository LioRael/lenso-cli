# Release Process

This repository releases the Cargo CLI and its npm distribution independently.
There is no repository-wide release plan, shadow registry, central publisher,
nonce, or receipt channel.

## Cargo crate

Release-plz runs on pushes to `main`:

1. `release-pr` opens or updates a release pull request for `lenso-cli`.
2. `release` publishes the version from a merged release pull request through
   crates.io Trusted Publishing and creates the `lenso-cli@<version>` tag.

Merge the generated PR with its `release` label intact, keep its
`release-plz-` source-branch prefix, and do not customize the final squash
subject. Release-plz verifies that the `main` commit is associated with a PR
whose source branch has that prefix; a generic recovery PR can therefore
produce a successful workflow that correctly skips publication. For a
one-commit recovery PR, use the same branch prefix and name the commit
`chore: release` so GitHub preserves the generated release-PR identity.

The crates.io registry is the source of truth for existing versions. Public
versions, tags, and the historical `CHANGELOG.md` are not rewritten. Configure
a crates.io Trusted Publisher for `lenso-cli` before the first live publish
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
cargo test --locked
cargo package --locked -p lenso-authoring --allow-dirty
cargo package --locked -p lenso-cli --allow-dirty --no-verify
cargo publish --dry-run --locked -p lenso-authoring --allow-dirty
cargo publish --dry-run --locked -p lenso-cli --allow-dirty --no-verify
```

The CLI package uses `--no-verify` before release because its workspace
`lenso-authoring` changes are not available from crates.io yet. The workspace
test verifies both packages together, and the preceding command verifies the
library package independently. Release-plz publishes changed workspace crates
in dependency order.

To inspect an npm archive locally, build the current platform payload first:

```sh
npm run package:npm
npm run check:npm-publish
npm pack --dry-run --ignore-scripts
```

Cross-repository compatibility is proven by SemVer requirements, contracts,
and focused integration checks. Do not restore the retired `lenso-release`
runtime or a shared release channel to coordinate the two package streams.

## Event recovery

GitHub suppresses new workflow runs when a repository `GITHUB_TOKEN` creates or
updates a release branch. If merging one of those generated branches does not
produce the expected `main` push run, dispatch the same reviewed Trusted
Publisher workflow against `main` instead of creating an empty commit or using
a local registry token:

```sh
gh workflow run release-plz.yml --ref main
gh workflow run release-changesets.yml --ref main
```

Inspect the exact `main` commit and public registry state before dispatching.
The manual entry points run the same jobs, permissions, package checks, and OIDC
publish steps as the normal push path.
