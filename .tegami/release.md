---
packages:
  lenso-cli:
    type: patch
  "@lenso/cli":
    type: patch
---

### Fixes

Forward termination signals from the npm shim to the bundled CLI so long-running
commands stop their Workloads and clean owned state.
