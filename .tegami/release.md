---
packages:
  lenso-cli:
    type: patch
  "@lenso/cli":
    type: patch
---

### Fixes

Publish the universal CLI fixed group after removing the unrelated hosted
Console step that blocked the Windows binary build before registry writes.
