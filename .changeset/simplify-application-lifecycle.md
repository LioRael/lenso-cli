---
"@lenso/cli": minor
---

Simplify the public application lifecycle to Compose, Run locally, Connect, and
Status. Remove the retired App Plan, Apply, Verify, Diff, Repair, Next, Upgrade,
and Explain commands; the App Compose `--write-plan`, `--explain`, and `--addon`
options; and the retired System Init, AddService, AddModule, Plan, Diff, Apply,
Doctor, Release, Runbook, and Graph commands. Keep `app compose --apply` only as
one atomic materialization flag, not a lifecycle stage.
