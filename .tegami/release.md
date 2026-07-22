---
packages:
  lenso-cli:
    type: patch
  "@lenso/cli":
    type: patch
---

### Fixes

Publish the M6 CLI distribution after scoping the shadow npm registry to the
sealed publish command instead of the publishing toolchain bootstrap.
