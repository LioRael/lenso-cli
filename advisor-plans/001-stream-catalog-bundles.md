# Stream catalog bundle IO

Status: IMPLEMENTED AND VALIDATED

Finding: a catalog bundle is retained entirely in memory while downloading and hashing, and immutable retention compares two whole archives at once, allowing roughly twice the 256 MiB bundle limit in transient memory.

Scope:
- stream download bytes through the size bound and SHA-256 into the staged file;
- compare retained archives with fixed-size buffers;
- add bounded-copy, digest, oversize, and equality regressions.

Implementation:
- catalog downloads stream through one 64 KiB buffer into the staged file while
  enforcing 256 MiB and computing SHA-256;
- immutable archive comparison checks equal metadata length, compares exact
  fixed-size blocks with `read_exact`, and verifies EOF without retaining either
  file.

Validation: bounded-copy/hash/oversize tests and short-read archive regressions
passed; CLI workspace fmt/check/clippy (`-D warnings`)/test passed (28 library,
33 binary, 4 intentionally ignored clean-room tests); release all-targets check
passed.
