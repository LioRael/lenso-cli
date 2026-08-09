# Artifact and release

Bind one immutable Console UI artifact to the same Module Release that declares
the Surface. The release evidence accounts for the ESM entry, declared style
assets, generated Console Module manifest, UI digest, release digest,
compatibility ranges, provenance, and requested permissions.

The Console Shell loads reviewed same-origin artifacts from receipts. It
rejects unsafe paths, undeclared assets, identity mismatches, incompatible API
ranges, manifest mismatches, and digest mismatches. Those failures enter
Artifact Quarantine while the backend Module remains installed.

Published immutable artifacts are superseded by a new release, not rewritten.
Framework contract, Console SDK and host support, and Module artifact releases
must respect their dependency order.
