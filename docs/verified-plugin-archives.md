# Verified Plugin archive ingestion

`lenso_app_authoring::bundle_archive::VerifiedPluginArchive` is the shared
authoring seam for a marketplace ingestion service or target installer that
has independently admitted an exact remote archive identity.

```rust,no_run
use lenso_app_authoring::bundle_archive::{PluginArchiveIdentity, PluginReleaseIdentity, VerifiedPluginArchive};
let expected = PluginArchiveIdentity {
    size: 1024,
    sha256: format!("sha256:{}", "a".repeat(64)),
};
let release = PluginReleaseIdentity {
    plugin_id: "example.echo".into(),
    release_version: "1.0.0".into(),
    manifest_digest: format!("sha256:{}", "b".repeat(64)),
};
let archive = VerifiedPluginArchive::read_release(
    std::fs::File::open("download.lenso-plugin")?, &expected, &release,
)?;
// Use an explicitly selected, privately staged target root.
let candidate = archive.prepare_mutation(
    std::path::Path::new("candidate-app"), lenso_app_authoring::BundleMutation::Add,
)?;
// Inspect candidate.verified() and candidate.resolved(). Dropping it cancels.
// candidate.commit()? publishes bytes to this authoring root; it is not Ready.
# Ok::<(), anyhow::Error>(())
```

The API bounds reads to the expected size plus one, hashes the exact private
copy, rejects mismatches before extraction, reuses the CLI's bounded ZIP
extractor, and invokes the framework Bundle verifier. It retains the archive
and extraction together until drop. It never trusts a second caller-provided
directory or reopens a mutable source path after checking it.

The caller owns network origin/redirect/DNS/timeout policy, catalog trust and
freshness, authorization, and target revision/Ready admission. `read_release`
binds the expected Plugin ID, version and manifest digest supplied by that caller. The private directory must not be modified before use.
`read` and `read_release` do not mutate an App. `prepare_mutation` reuses the
existing Host policy and complete-App resolver, then compares the staged identity
to the originally verified Bundle before returning a commit handle. Both add and
replace use the same path. A mismatch or discarded proposal leaves installed
Bundle bytes unchanged. The method does not switch a live Generation.

The caller must not hold this short-lived authoring lock while waiting for a user
or network request. A live target prepares its private candidate root, serializes
its own expiring review proposal and receipt, and revalidates authorization and
base revision at apply time. Do not call this primitive directly on a running
App to bypass its control plane.

`archive_bundle` and `with_bundle_directory` remain local authoring helpers.
The latter accepts directories and does not establish remote archive identity.

For real-artifact acceptance, set `LENSO_TEST_PLUGIN_ARCHIVE` to an absolute
CLI-built `lenso.marketplace.echo` archive path and run the ignored
`verified_archive_retains_validated_bytes_after_source_changes` library test.
It proves source replacement cannot replace retained bytes and dropping the
handle removes its private extraction.

Run `verified_release_prepares_add_and_rejects_substitution_without_changing_root`
with the same environment for add, canceled preview, duplicate-add rejection,
wrong release identity, temporary-directory substitution, preserved old bytes and
an independently admitted replacement. The test's second release is explicitly
retagged fixture metadata using the same executable, not a new runtime behavior.
Build the archive with this CLI checkout so its target vocabulary matches the
Host; archives from older packers may be valid but incompatible.

This source API is pending CLI publication. Marketplace's
`verify-archive-handoff.sh` compiles an explicit source integration proof without
adding a permanent sibling-checkout dependency. Network acquisition policy,
Agent's remote proposal API, durable operations and online Ready proof remain
separate target-owned prerequisites.

## Target-admitted HTTPS download

`PluginArchiveDownloadPolicy::new(&origins)` accepts 1–16 credential-free HTTPS
origins, provided by the target operator independently of catalog metadata.
`download_release(url, transport_identity, release_identity)` streams a response
into `VerifiedPluginArchive::read_release`. It does not buffer the full archive
in memory, change a Plugin Root, or grant permission to execute it.

The transport denies redirects, ambient proxies, non-200 responses, encoded
responses and mismatching Content-Length. The bounded archive reader enforces
the signed byte limit and digest even if the server omits Content-Length.
Both literal addresses and the complete DNS result set must satisfy the public
address policy. The connector uses those exact checked addresses rather than
performing a second DNS lookup. An empty, excessive or mixed public/private DNS
answer is rejected. TLS uses the normal library trust store; allowlisting an
origin does not bypass certificate verification.

Network operations have a 60-second HTTP deadline, a 10-second connect/write
limit and a 30-second read limit. System DNS resolution is OS-managed and is not
a cancelable part of that deadline. Target code must bound concurrent acquisition
and recheck authorization, catalog freshness and its base revision after download
and before applying a proposal. Private artifact servers and redirecting CDN URLs
are deliberately unsupported by this initial policy; publish a direct allowed
HTTPS URL instead.

The conservative address policy was checked against the
[IANA IPv4 registry](https://www.iana.org/assignments/iana-ipv4-special-registry/)
and [IANA IPv6 registry](https://www.iana.org/assignments/iana-ipv6-special-registry/).
Some globally reachable protocol exceptions are intentionally denied.

Tests use a real local HTTPS server, an ephemeral certificate and a test-only
resolver/CA. Production has no option to disable the public-address check or
certificate verification. The suite covers exact-byte success, redirect refusal,
private/mixed DNS answers, bad certificate, wrong length/encoding/digest, oversized
body and response timeout. Set `LENSO_TEST_PLUGIN_ARCHIVE` to a current CLI-packed
Echo archive and include ignored tests to run the valid archive download case.

This remains a source-level installation primitive pending publication. Agent's
remote proposal endpoint, target-bound approval and durable runtime receipts are
not implemented by this transport addition.

Validation: 62 library tests pass with the real archive tests enabled, including
the local HTTPS transfer. Library/test Clippy passes with warnings denied.
No Agent production configuration, public registry or running App was changed.

## Runtime dependency

Registry builds require `lenso-plugin-bundle >=0.4.2`, which preserves canonical
manifest identity when other dependencies enable `serde_json/preserve_order`.
The CLI no longer requires a Git source override or a sibling checkout.
