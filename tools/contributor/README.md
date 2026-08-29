# Local framework checkout tools

These scripts manage a contributor's local multi-repository Lenso checkout.
They are deliberately outside the Cargo workspace and the published crate/npm
payload, and they do not register public `lenso` subcommands. Product authors
do not need `gh`, Worktrunk, a sibling-repository layout, or these tools.

Install or refresh the thin local entrypoints explicitly:

```sh
tools/contributor/install.sh --framework-root /absolute/path/to/framework
```

The installer copies `lenso-workspace` and `lenso-pr` into
`<framework-root>/.lenso-tools/bin`. It never changes Git repositories.

`lenso-workspace snapshot` is a fast local snapshot. `doctor` fails closed with
`attention` for dirty or diverged worktrees and with `failed` for unsafe state
or missing dependencies. `release-status` fetches remotes by default, reports
open PR/check state and Git dependency pins, and fails when remote evidence is
unavailable; use `--no-fetch` only when explicitly inspecting cached refs.

`lenso-pr finish` requires an exact repository, PR number, and clean worktree.
Pass each first-party gate by its exact GitHub check name with
`--required-check`; `--merge` refuses to run without at least one. The helper
waits a bounded minute for those checks to become observable, verifies all
visible checks are terminal-successful, merges only with explicit `--merge`,
compares the fetched local tree with GitHub's merge tree, and then asks
Worktrunk to remove the integrated branch without any force flag. Transient
GitHub read and merge failures are retried without leaking partial output into
JSON mode.

If this directory grows beyond this bounded pair, move it to a dedicated
`lenso-workspace-tools` repository instead of turning the product CLI into a
cross-repository control plane.
