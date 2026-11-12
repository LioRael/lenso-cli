# Extension distribution Registry

Lenso's distribution Registry resolves an extension package name and version into
an immutable manifest address and SHA-256. It has no knowledge of components,
code generators, npm, workspace formats, or an extension's own resource catalog.
Extension IDs still own commands, workflows, and handlers; package references locate
those extensions. A package name does not grant execution or file/network access.

```toml
[cli]
schema_version = 1
registries = { "@acme" = "https://packages.example.com/index/" }

[[cli.extensions]]
id = "code"
source = "registry:@acme/code"
version = "^1.2"
reads = ["input.txt"]
files = ["output.txt"]
```

Only declared extensions resolve. Unknown commands do not search registries or
install packages. Namespace routes are explicit: no public official Registry is
assumed or deployed. Local manifests and explicit HTTPS manifest URLs remain valid.
The package reference and the manifest's extension ID may differ; both are checked
against their respective declaration/index identities.

## Static HTTP protocol

The example fetches `https://packages.example.com/index/code.json`:

```json
{
  "schema_version": 1,
  "name": "@acme/code",
  "releases": [
    {
      "version": "1.2.3",
      "manifest": "../packages/code/1.2.3/extension.toml",
      "sha256": "<64 lowercase hexadecimal characters>",
      "yanked": false
    }
  ]
}
```

The index is ordinary static JSON. Host it and the existing TOML/JSON manifests
and Wasm files on an HTTPS static file server or object store. Numeric loopback
HTTP is allowed for development. Registry base URLs end in `/` without queries;
manifest references resolve relative to the index and must remain on its origin.
URL credentials, fragments, and redirects are rejected. Namespace/package segments
accept ASCII letters, digits, hyphens, and underscores.

Lenso chooses the highest non-yanked semantic version satisfying the requirement.
With no requirement, `*` selects stable releases; prereleases require a matching
explicit requirement. Duplicate versions, mismatched index names, unsupported
schemas, and more than 4096 releases fail resolution. The index is bounded to 1 MiB.
Before accepting a manifest, Lenso verifies its exact digest, extension ID, release
version, command ownership, and module digests. A remote manifest cannot use `auto`
module digests. Publish each release under a stable versioned path.

This protocol provides name/version resolution and content integrity, not publisher
signatures or namespace ownership verification. The configured Registry is a trust
root. Static HTTP index distribution currently uses public HTTPS endpoints; private HTTP-index auth,
a hosted directory/search UI, publishing automation, and dependency graphs between
extension packages are not implemented. Authenticated GitHub Release packages and
repository sources are described in [GitHub distribution](github-distribution.md). An extension's authenticated business HTTP
requests use the separate Host credential bindings described in the protocol docs.

## Locking and cache

First execution resolves index and manifest metadata and saves `lenso.lock`. The
lock preserves the declared Registry reference, configured route, resolved manifest
URL, selected definition/version, manifest digest, and exact module sources/digests.
Changing a used namespace route invalidates its declaration match; policy-only grant
changes do not upgrade dependencies. `--locked` rejects missing or changed pins.

Warm execution uses the pinned definition without consulting the index. Wasm files
are downloaded only when invoked and reuse the per-user content-addressed cache.
`lenso update` refreshes index/manifest metadata and selects again. A yanked release
continues to work from an existing lock until an explicit update changes selection.
`--offline` can resolve a new project's declaration from cached metadata and reuse
cached modules; it fails when the required data is unavailable. Removing cache
restores exact locked module bytes and never silently upgrades a package.

Help may fetch index/manifest metadata on a cold project but downloads no remote
Wasm and writes no project lock. Resolution never executes an extension. Invalid
metadata, a digest/version mismatch, or failed definition registration leaves the
existing project lock intact. Registry responses cannot grant capabilities; root
project policy and digest-bound interactive approvals still govern actual work.

## Verification

```sh
cargo test --workspace
cargo build --workspace
python3 scripts/build-demo.py
python3 scripts/test-dependencies.py --registry
```

The command-level acceptance serves the real greeting Wasm over HTTP, resolves it
by name, checks JSON questions and read-only previews, shares cached bytes between
projects, exercises locked/offline execution, and restores concurrent cache misses.
Native tests cover release selection, yanking, prereleases, explicit upgrades,
route changes, malformed identities, hash/version mismatches, and failed updates.
