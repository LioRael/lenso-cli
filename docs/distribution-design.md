# GitHub-based distribution design

Status: design record with an initial implementation. GitHub Release extension
packages, author packaging/configuration and GitHub publishing, repository resource snapshots, source
pins, and the optional code extension are implemented. The operational contract is
[GitHub distribution](github-distribution.md). Repository-hosted executable packages
and installed-file baseline receipts below
remain future goals. Existing static HTTP sources remain supported.

## Decision

Use GitHub Releases for executable extension packages and GitHub repository
snapshots for source-code resources. Keep the existing static HTTP provider as
an alternative for mirrors, private infrastructure, and existing registries.
A website, npm, Node.js, Git, and the GitHub CLI are not installation prerequisites.
An extension may independently require a runtime or a target project's package
manager; its manifest and workflow must make that requirement explicit.

| Content | Default source | Published unit |
| --- | --- | --- |
| Lenso CLI | GitHub Release | OS/architecture-specific native binary archive |
| Wasm extension | GitHub Release | Prebuilt package, small metadata, checksums |
| Components/templates/rules/configuration | GitHub repository | Catalog plus ordinary source files at one commit |
| Dynamic or existing resource catalog | HTTPS endpoint | Extension-owned response format |

npm is an optional future provider, not an execution requirement. A Rust client
can implement the registry/tarball protocol without invoking npm; package transport
and JavaScript execution are separate decisions. The initial release should focus
on GitHub and HTTP rather than reproducing npm's publishing ecosystem.

## Responsibility split

One shared source module resolves an address to a pinned snapshot and reads files
from it. GitHub Release, GitHub repository, HTTPS, and local sources are adapters.
The module owns transfer, credentials, limits, content verification, and cache.
It does not know what a component, template, npm dependency, or code catalog means.

The extension loader interprets extension manifests. A code-distribution extension
interprets its own resource catalogs, dependencies, destination mappings, transforms,
search, diffs, and updates. Both reuse source transport; neither needs the other's
manifest schema. A catalog containing source files cannot become an executable
extension merely by declaring commands or permissions.

Conceptual interface:

```text
resolve(source, selector) -> snapshot handle + immutable provenance
read(snapshot handle, relative path) -> verified bytes
```

For extensions, resolution happens before loading executable code. For resource
consumers, equivalent Host operations require the owning extension's source/read
authorization. Handles are host-owned and scoped to the invoking extension;
guests cannot forge provenance or expand a repository grant to another repository.
Downloaded documents remain data. No repository build/install hooks execute during
resolution, file lookup, search, or cache preparation.

## Executable extension publishing

A publisher builds Wasm in their own environment or CI and publishes ready-to-run
artifacts. Users do not clone a source tree and compile it on first execution.

Proposed single-package convention:

```text
repository source                 release v1.2.3
  extension.toml                    extension.toml
  Cargo.toml                        lenso-release.json
  src/                              lenso-code-1.2.3.tar.gz
```

The archive contains `extension.toml`, the pinned module files, and relevant license
files. The small release metadata identifies the package ID/version, manifest hash,
archive asset/name/size/hash, and package schema version. The archive's embedded
manifest must match the separately published manifest. Help can fetch small metadata
without downloading or compiling Wasm. Archives are extracted under existing limits;
absolute/traversal paths, symlinks, hard links, duplicate entries, and decompression
expansion beyond the declared limits are rejected.

A root `lenso.registry.toml` customizes publishing/discovery for nonstandard layouts
and monorepos. The name is proposed and distinct from a consumer's `lenso.toml`:

```toml
schema_version = 1

[[extensions]]
id = "code"
path = "extensions/code"
tag = "code-v{version}"
asset = "lenso-code-{version}.tar.gz"
include = ["extension.toml", "code.wasm", "LICENSE"]
```

Initially, one release publishes one extension package. Monorepos use per-package
tag patterns such as `code-v{version}` to keep metadata assets unambiguous.
A single-package repository can use the defaults without this descriptor. A custom
layout needs the descriptor; Lenso must not search arbitrary directories or guess
between similarly named artifacts. `path` and `include` configure packaging;
consumers use only package selectors and tag/asset conventions. The descriptor is
data, not a place to embed build commands. CI owns the explicit build step.

For discovery, resolve the descriptor's repository ref to a commit before reading
it, then list releases with pagination and interpret tags using its bounded pattern.
Choose the highest compatible non-draft/non-prerelease version unless the requirement
explicitly includes prereleases. Do not equate GitHub's `latest` label with the
highest satisfying semantic version. Fetch the selected release metadata/manifest
and verify ID, version, archive/module hashes, and compatibility before registration.
Package selectors within one repository identify packages, not arbitrary file paths.

The implemented commands are `lenso package` for local validation/artifacts and
`lenso publish --github` for uploading a prepared draft release. Uploading is an
explicit author action. A GitHub Actions workflow can invoke the same commands.
Publishing validates all assets before making the release visible, resumes matching
draft assets without overwriting, and accepts matching published/immutable releases
without mutation. The included Actions workflow calls the same native publisher.
No publishing command runs during consumer use.

## Code resource publishing

A code author should normally commit a catalog and the original source files:

```text
acme/ui-kits/
  catalog.toml
  components/button.tsx
  components/dialog.tsx
  templates/login/page.tsx
  rules/team.md
  LICENSE
```

A generic code extension could own a small author-facing TOML catalog:

```toml
schema_version = 1
name = "acme-ui"

[[items]]
name = "button"
files = ["components/button.tsx"]
```

The code extension defines this schema and any dependency/target semantics. Runtime
only sees repository objects and verified bytes. This is an illustrative minimal
schema, not a new mandatory Lenso-wide resource standard.

For shadcn compatibility, that extension can instead read an existing `registry.json`
and its referenced source files. JSON need not contain duplicated inline source
text, and authors need not generate one JSON response per item or deploy a server.
The current official shadcn GitHub workflow already supports this model; it is a
useful interoperability adapter, not a schema to put back into runtime.
[Official GitHub Registry documentation](https://ui.shadcn.com/docs/registry/github).

Resolve the chosen repository branch/tag once to a full commit, then read catalog
and files from that same snapshot. Same-repository resource dependencies use that
snapshot; dependencies in other repositories resolve and lock their own snapshots.
Files cannot silently fall back to `main` if missing at the selected commit.
Source ref movement becomes visible only through an explicit resource update.

The first implementation should use explicit file lists. Globs can later use a
bounded Git tree listing; truncated listings must be handled explicitly. Do not
require a complete clone or local Git installation. Batch/archive downloads can
reduce many-file transfer costs while selected-file digests remain the content
identity. Reject symlinks/submodules by default; do not follow them into other sources.

## Consumer configuration and ownership

Implemented consumer configuration:

```toml
[cli]
schema_version = 1

[[cli.extensions]]
id = "code"
source = "github-release:acme/lenso-code"
version = "^1.2"
# Separate, explicit source authorization; options do not grant capabilities.
sources = ["github:acme/ui-kits"]

[cli.extensions.options.registries.acme]
source = "github-repo:acme/ui-kits"
ref = "main"
path = "catalog.toml"
format = "lenso-code-v1"
```

Runtime forwards opaque `options` only to their owning extension. It recognizes
transport locators and authorization declarations, not `registries`, `format`, or
item names inside those options. File writes and package-manager invocations keep
the existing independent grants and digest-bound review.

An explicit package selector supports a monorepo, for example
`github-release:acme/lenso-tools/code`. A `github-repo:` extension source is also
possible for development, but its pinned snapshot must contain ready-to-run Wasm;
it never implies a local build. HTTPS/local sources and existing `registry:` references
remain valid.

Aliases such as `@acme/code` require an explicit routing entry. They do not imply
GitHub namespace ownership. A future official directory can be a GitHub-maintained
list mapping names to source locators; it need not host package bytes or be required
for direct sources. A publisher cannot claim a globally trusted namespace by placing
an `id` in their repository.

## Lock, cache, and local changes

Keep one root `lenso.lock`. The initial implementation stores extension pins and
owner-scoped repository resolution pins. The following installed-resource receipt
model is a later goal: store extension package pins and separately owned
resource receipts. Core defines owner/source/revision/digest envelopes; the code
extension owns resource metadata such as item identity and installed file baselines.
Do not recreate a runtime-level component schema.

Extension pins include repository identity, discovery revision, chosen version/tag,
release/asset IDs, manifest/archive/module hashes, and platform variant if applicable.
Resource receipts include repository identity, commit, catalog digest, selected file
digests, and the owning extension's opaque receipt. This allows future update/diff
operations to identify sources rather than guessing from generated file names.
Receipts update with the approved file plan. Local modifications still require
review; recording a source does not authorize overwriting or deleting files.

Cache verified immutable blobs globally per user. Mutable ref lookup uses conditional
metadata caching; warm locked execution requires no ref/release lookup. Explicit
extension/resource update refreshes only the selected scope. Cache deletion restores
the locked content or fails; it must never select a replacement version/ref silently.
Missing/deleted upstream content is a real availability error. Registry signatures
and independent mirrors can improve recovery later; checksums alone do not establish
publisher identity.

## GitHub transport details

The current HTTP client rejects redirects, and current HTTP Registry manifests must
remain on one origin. GitHub assets require a dedicated provider: its API may return
200 or redirect to a download endpoint. Handle a bounded HTTPS redirect sequence,
validate destinations, and never forward an API credential to another origin.
Do not globally relax generic HTTP behavior to accommodate this provider.
[GitHub release asset API](https://docs.github.com/en/rest/releases/assets).

Repository content reads accept a branch/tag/commit ref. Resolve first, then use the
commit for every read. Persist stable repository/revision/asset identities, not
expiring download or signed CDN URLs. Fetch fresh transfer URLs when necessary.
[GitHub repository content API](https://docs.github.com/en/rest/repos/contents).

Private sources use explicit user/CI credentials, scoped to the GitHub API host and
repository. Do not require `gh` or `git` to be installed; optional integrations can
come later. Credentials and signed transfer URLs stay out of Wasm, lock files,
reports, and shared metadata indexes. Public content can be cached by digest;
private source metadata uses a credential-context namespace to avoid cross-account
lookup confusion and must still require a locally authorized source/credential
binding when accessed. Offline checks cannot detect server-side revocation or new
permission changes until online revalidation.

Rate limiting is part of the provider contract. Use ETags/conditional reads, respect
Retry-After/reset headers, and avoid per-command polling and unbounded fan-out.
Large catalogs can use source snapshots or publisher-provided bundles; dynamic search
and per-user entitlements remain reasons to choose an HTTP resource endpoint.
[GitHub API rate limits](https://docs.github.com/en/rest/using-the-rest-api/rate-limits-for-the-rest-api),
[API best practices](https://docs.github.com/en/rest/using-the-rest-api/best-practices-for-using-the-rest-api).

## Implementation order

1. Add repository snapshots, credential-scoped GitHub access, and provider-specific
   downloads. Keep existing direct HTTP contracts and generic Host grants intact.
2. Add extension package layout, package validation, Release resolution, and a local
   packaging command. Verify real no-Node/no-Git consumption before automating upload.
3. Expose authorized snapshot reads to resource extensions; implement a repository
   catalog adapter and source receipts in the code extension using existing preview,
   conflict, approval, and JSON/Agent flows.
4. Add optional publishing automation, namespace directories, and HTTP mirrors.
   npm and other storage providers can follow a demonstrated requirement.
