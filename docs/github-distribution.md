# GitHub distribution

GitHub Releases distribute prebuilt extensions. GitHub repository snapshots supply
code resources to an optional Wasm extension. Runtime owns transport, provenance,
permissions, the root lock and the user cache; it does not interpret code catalogs.
No Node.js, Git or GitHub CLI process is used by either consumer path.

## Publish ready-to-run extension assets

A single-package author's directory contains `extension.toml` and its prebuilt Wasm
modules. Build the modules explicitly in your development environment or CI, then run:

```sh
lenso --project PATH_TO_AUTHOR_PROJECT package --out dist/lenso
```

This local command creates three assets: `extension.toml`, `lenso-release.json`,
and `lenso-<id>-<version>.tar.gz`. It computes exact module pins from local `auto`
digests, rejects mismatched explicit digests, validates manifest ownership and workflow
graphs, and builds a deterministic archive.
The archive contains the same manifest plus its modules and optional included files.
Host-native workflow dependencies are checked against the consuming application.
Packaging does not execute Wasm or prove its behavior.
It executes no repository build/install scripts and performs no upload.

By default the tag is `v<version>`. For custom directories, names, or a monorepo,
commit an author-owned
`lenso.registry.toml` at the repository root:

```toml
schema_version = 1

[[extensions]]
id = "code"
path = "extensions/code"
tag = "code-v{version}"
asset = "custom-code-{version}.tgz"
include = ["LICENSE"]
```

```sh
lenso --project PATH_TO_AUTHOR_PROJECT package --id code --out dist/code
```

Paths in `include` are explicit and relative to the package directory. Required Wasm
files are always included. `path` and `include` are packaging settings; consumers
use the ID and tag/asset patterns. Only `{id}` and `{version}` substitutions are
supported. One extension package is published per Release; monorepos use distinct
package tag patterns. A selected package must be present in the author descriptor.

The generated release metadata has this shape:

```json
{
  "schema_version": 1,
  "id": "code",
  "version": "0.1.0",
  "manifest": { "name": "extension.toml", "sha256": "<sha256>", "size": 1234 },
  "archive": { "name": "custom-code-0.1.0.tgz", "sha256": "<sha256>", "size": 5678 }
}
```

Consumers declare the Release source:

```toml
[cli]
schema_version = 1

[[cli.extensions]]
id = "code"
source = "github-release:acme/lenso-tools/code"
version = "^0.1"
```

Use `github-release:owner/repo` for a conventional single-package repository or
`github-release:owner/repo/package` for an explicit descriptor entry. Lenso resolves
the author's discovery descriptor at a commit and paginates Releases to choose the
highest satisfying semantic version. Drafts are excluded; prereleases require an
explicit matching version requirement. It does not treat GitHub's `latest` label as
semantic version ordering.

Cold help fetches small metadata and the manifest, without downloading the executable
archive or writing a lock. First execution saves exact Release/asset IDs, descriptor
commit, version, manifest/archive/module hashes and API authority in `lenso.lock`.
Archives are fetched lazily when a module is needed. Manifest identity, version,
archive bytes, embedded manifest and required modules are verified before execution.
Local paths, direct HTTPS manifests and existing static `registry:` references remain
supported.

`lenso update` refreshes extension selection. Warm execution and `--locked` restore
exact pinned assets without querying current Releases or the author's default branch.
A removed asset fails restoration; it never silently selects a replacement asset.

### Publish from a terminal or Agent

```sh
# Prepare inspectable assets without contacting GitHub or needing a token.
lenso --project PATH_TO_AUTHOR_PROJECT publish --github --repo acme/lenso-tools \
  --id code --dry-run --offline --out dist/code

# Build Wasm explicitly first. Provide the token through the named environment variable.
export GH_TOKEN=...
lenso --project PATH_TO_AUTHOR_PROJECT publish --github --repo acme/lenso-tools \
  --id code --target FULL_COMMIT_SHA --out dist/code
```

`publish` packages the local prebuilt modules, verifies all three assets, and writes
them to `--out` (default `dist/lenso`). It resolves the target through GitHub's API;
the default is remote `HEAD`, not the local checkout. Pass `--target` to bind the
release to the commit used to build the artifacts. Existing lightweight/annotated
tags must reference that commit; Lenso never creates a replacement tag or moves one.
GitHub creates a missing tag when publishing the release. Local source/build provenance
is the author's responsibility; the CLI does not invoke Git to compare a worktree.

Custom tag/asset conventions must already be discoverable from `lenso.registry.toml`
on the repository's default branch. Packaging-only `path`/`include` settings can differ
in CI. The local version, selected ID and archive name must match discovery conventions.
The archive name cannot collide with either metadata asset name.

The confirmation shows the repository, tag, resolved commit, output directory, and
each asset's byte size and SHA-256. JSON/Agent mode returns the same digest-bound
confirmation through `needs_input` (exit 2). Supply its boolean answer through
`--answers`, or use `--yes` for an explicitly approved CI invocation. Changing the
payload, target or observed release state invalidates old answers. No token or asset
body is returned in the question/result. `--dry-run` performs no GitHub requests and
needs no confirmation. A real publish fails with `--offline`.

Uploads always start with a draft. Each uploaded asset is verified against GitHub's
SHA-256 digest, or downloaded and hashed when the server omits the digest. Only a
complete, verified set is made public. `--draft` keeps that set private for review.
Extension publication does not change GitHub's `latest` entry; native CLI Releases
use that entry for their installer channel. Extension consumers resolve by semantic
version and do not use `latest`.
On interruption, rerun with the same artifacts and `--target`: matching draft assets
are reused and missing ones are uploaded. Conflicting, duplicate or unexpected assets
fail without deletion/overwrite. A complete published release is an idempotent success,
including immutable releases; published assets are never edited or converted to draft.
Mutations are not automatically retried. Failures preserve the release and report its
ID and the number of verified uploads; inspect any asset whose outcome was uncertain
before retrying. One package owns the complete three-asset Release.

The result includes `state` (`prepared`, `draft`, `published`), repository, package ID,
version, tag, commit and asset checksums; successful uploads also return `release_id`,
`release_url` and the number uploaded in this attempt. Progress stays on stderr in
JSON mode. Publishing uses native HTTP and never runs `gh`, Git, Node.js or build hooks.

Publishing credentials, in priority order, are `--token-env NAME`, project
`cli.github.token_env`, `GH_TOKEN`, then `GITHUB_TOKEN`. `--api` overrides the project's
API base for publishing. Only publishing auto-detects these two conventional token
variables; consumer credentials remain explicit project policy. Use a token permitted
to write Releases in the target repository. Uploads use the returned upload URL with
repository/release and origin validation; authorization is never followed across a
mutation redirect. See the [GitHub Release API](https://docs.github.com/en/rest/releases/releases)
and [asset API](https://docs.github.com/en/rest/releases/assets).

### GitHub Actions

The repository's [publish workflow](../.github/workflows/publish-extension.yml) builds
the native CLI and portable `code` Wasm with the committed lockfiles, stages the package
declared in root `lenso.registry.toml`, and calls the same command. Push a `code-v<version>`
tag matching `presets/code/extension.toml`, or dispatch the workflow manually (draft by
default). Commit the discovery descriptor to the default branch before the first run.
The workflow grants `contents: write` and exposes its token to the native publish
and post-publication consumer verification steps. It does not persist checkout credentials.

Inside Actions, `--repo` defaults to `GITHUB_REPOSITORY` and `--target` defaults to
`GITHUB_SHA`. A tag-triggered invocation verifies `GITHUB_REF_NAME` against the packaged
version/tag convention before any network access. Explicit `--token-env GITHUB_TOKEN`
avoids a different `GH_TOKEN` taking precedence. Subsequent steps can use
`steps.publish.outputs.release_id`, `release_url`, `tag`, `package_id`, `version` and
`state`; these are appended to `GITHUB_OUTPUT` after success (not during dry runs).
Workflow builds may use development tools; consumers download the ready-to-run Wasm
through the native CLI and need neither Node.js nor Git.

Once a Release leaves draft state, the workflow runs
`scripts/verify-github-release.py`. This creates a fresh consumer and cache, empties
the CLI's PATH, and downloads the package and `fixtures/distribution` catalog/source
at the workflow commit. It checks search, preview without writes, declined and approved
file plans, exact file content, checksums, cache restoration under `--locked`, and
offline replay with unreachable proxies. Draft releases skip consumer verification.
The script retains `verification.json` and JSON command traces under its `--out`
directory; Actions also writes a summary with the release and checksums.

For a separate local consumer verification, export a named token and run:

```sh
python3 scripts/verify-github-release.py --binary target/debug/lenso \
  --repo OWNER/REPO --version VERSION --ref FULL_SOURCE_COMMIT \
  --token-env GH_TOKEN --out dist/live-consumer
```

`--out` must be new. The verification script requires Python as a test driver;
each consumer command runs only the native Lenso binary with an empty PATH.

## Optional code resource extension

For local development, build and configure the supplied extension:

```sh
cargo build --workspace
python3 scripts/configure-code.py PROJECT --repo acme/ui-kits \
  --registry acme --ref main --catalog catalog.toml --target src

lenso --project PROJECT code search --query button
lenso --project PROJECT code preview button
lenso --project PROJECT code add button
lenso --project PROJECT code diff button
lenso --project PROJECT code update button
```

The helper stores an immutable local source package and preserves other extension
declarations. It grants one explicit repository and the chosen destination/read root.
Authors can distribute the same Wasm extension through Releases; consumers then use:

```toml
[cli]
schema_version = 1

[[cli.extensions]]
id = "code"
source = "github-release:acme/lenso-code"
version = "^0.1"
sources = ["github:acme/ui-kits"]
read_roots = ["src"]
write_roots = ["src"]

[cli.extensions.options.registries.acme]
source = "github-repo:acme/ui-kits"
ref = "main"
path = "catalog.toml"
format = "lenso-code-v1"
target = "src"
```

Those repository names are illustrative; no official package repository is deployed
by configuring Lenso. `sources` is a capability declaration. `options` is opaque
business configuration delivered to the owning extension and grants no capabilities.
Multiple registries prompt for a selection or accept `--registry NAME`.

A resource author's repository contains ordinary UTF-8 source files and a catalog:

```toml
schema_version = 1
name = "acme-ui"

[[items]]
name = "button"
description = "A reusable button"
files = [{ path = "components/button.tsx", target = "components/ui/button.tsx" }]

[[items]]
name = "rules"
files = ["rules/team.md"]
```

Catalog paths are configurable. Source file paths are relative to the catalog's
repository directory. String entries preserve that relative path as their target;
object entries give an explicit target. The consumer's `target` prefix is prepended
to destinations, so the example writes `src/components/ui/button.tsx`. All destination
reads and writes still need their existing independent project grants.

An `inline` format can declare `items` directly in the extension's registry options
and read explicit files relative to the repository root. This is useful for existing
repositories without an author catalog:

```sh
python3 scripts/configure-code.py PROJECT --repo octocat/Hello-World \
  --file README --item readme --target docs
lenso --project PROJECT code preview readme
```

The code extension parses catalogs and maps files; Host only resolves snapshots and
reads verified files through scoped handles. Raw URLs, forged handles, cross-repository
access, traversal, symlinks and submodules cannot expand a repository capability.

## Source pins, review and cache

First resource resolution pins a branch/tag to one full commit. Catalog and source
files are all read at that commit, including tree-mode and Git blob identity checks. Their SHA-256 hashes
are stored in the root lock's owner-scoped `resources` entries. These are resolution
pins, not installed-file baselines or three-way merge receipts. There is no additional
component state file. Extending the set of observed files can add hashes to the lock.

Resource preparation, search and previews may update resolution metadata in
`lenso.lock` and fill the user cache, as extension preparation does. They do not write
application files or execute package commands. `code update` explicitly refreshes
the resource ref even if the user later declines application. `lenso update` refreshes
extension packages and preserves resource pins. Changed source pins invalidate stale
file plans; concurrent metadata writes preserve both extension and resource records.

`code add` preserves the current pin. `code diff` compares without overwrite questions
or application. Existing differing files in add/update require overwrite/keep choices,
then use the existing full-diff review and digest-bound approval. `--yes` answers
confirmations only. JSON/Agent mode preserves `complete`, `needs_input`, diagnostics
and exit codes; terminal prompts retain filtering, back/revision and cancellation.

`--locked` rejects new/changed repository pins and previously unobserved source files;
prepare the intended resource normally before using locked CI. `--offline` prevents
these source fetches and uses verified cached objects. Missing/corrupt bytes are restored
under pinned identities when online, or fail offline. Cache deletion never upgrades
an extension version or follows a moving repository ref.

## Authentication, transport and limits

GitHub credentials are explicit project policy:

```toml
[cli.github]
token_env = "GH_TOKEN"
# api = "https://api.github.com/"
```

Export that environment variable before invoking Lenso. An API token stays inside
trusted transport state, is sent only on the initial API-origin request, and is not
forwarded across asset download redirects. GitHub's API may return either a body or
a redirect for assets, so the provider handles both with bounded redirects and an
allowlist of HTTPS GitHub download hosts. Generic HTTP redirect rules remain unchanged.
Stable asset IDs are persisted; expiring signed transfer URLs are not.
[GitHub asset API](https://docs.github.com/en/rest/releases/assets).

`cli.github.api` can select a compatible custom HTTPS API base; numeric loopback HTTP
is accepted for tests. It must end in `/` without credentials, query or fragment.
Private metadata indexes are separated by API/credential context. Locked private
sources require a configured credential even when using cache. Offline use checks
local source authorization and hashes; it cannot detect server-side revocation.
No `gh` login, token discovery or Git credential helper runs inside Lenso.

The provider caches immutable metadata/files and refreshes mutable lookups only on
explicit resolution/update. Rate-limit responses fail with an actionable diagnostic
without polling. Conditional ETag refresh and automatic rate-limit scheduling are not
implemented. Anonymous GitHub quotas can limit large first-use catalogs; explicit
credentials provide the appropriate authenticated API access.

Bounds: 128 Release pages of 30 entries, 1 MiB JSON metadata/source files, 32 MiB
compressed packages, 64 MiB expanded packages, 128 package files (256 total archive entries), 16 path segments,
128 resource pins and 128 observed files per pin, with an 8 MiB root lock limit.
Archives reject duplicate paths, links, special files and traversal. Each consumer
transport instance has a 20-second transfer deadline and 256-request budget; Wasm's
existing memory/fuel limits still apply. Author publishing mutations have a
120-second per-request timeout and no automatic retry.

The first code catalog format supports explicit UTF-8 files and targets. It does not
perform dependency installation, interpolation, framework transforms, arbitrary
post-install commands, automatic deletion, glob expansion, or three-way merging.
Repository-hosted executable extension packages remain future work.

## Verification

```sh
cargo test --workspace
cargo build --workspace
python3 scripts/test-github.py
python3 scripts/test-publish.py
```

Consumer acceptance places locally built real Wasm into a local GitHub REST fixture,
resolves custom Release conventions, follows credential-free signed redirects,
reads authored catalogs/files at pinned commits, exercises approvals/keep/overwrite,
checks locked/offline replay and cache restoration, rejects malformed archives and
source escapes, and runs consumers with an empty PATH. This is not evidence of a
new publicly published GitHub Release; live public API verification is reported
separately when available.

Publisher acceptance exercises the native REST upload path, consent/revision, partial
resume, tag conflicts, immutable/idempotent releases, Actions variables/outputs and
credential isolation, then consumes the uploaded package with an empty PATH. A local
fixture does not establish a successful GitHub-hosted Actions run or public release.

Real private Release uploads, a successful GitHub-hosted Actions build/publish/consume
run, and independent macOS consumption are recorded in
[live GitHub Release verification](github-release-verification.md).
