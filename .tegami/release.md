---
packages:
  lenso-cli: patch
  "@lenso/cli": patch
---

### Fixes

Preserve executable modes on the bundled Unix CLI binaries when an npm release
is rebuilt through the reviewed partial-recovery path.
