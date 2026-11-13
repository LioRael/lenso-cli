# Rust crates and the 1.0 migration

The crates.io package names `lenso-cli` and `lenso` move from the previous
`LioRael/lenso` implementation to `LioRael/lenso-cli` starting at 1.0.0.
Existing 0.x releases remain immutable and belong to their original implementation.
This is an intentional breaking product/API migration. The main SDK repository and
its other packages are not modified by this release.

| Crate | Version | Role |
| --- | --- | --- |
| `lenso-cli` | 1.0.0 | Embeddable command/workflow engine |
| `lenso-runtime` | 0.2.4 | Generic Host and extension distribution |
| `lenso-composition` | 0.2.4 | Pure capability/instance resolution |
| `lenso-codegen` | 0.2.4 | Pure generator contracts and templates |
| `lenso-project` | 0.1.2 | Optional reviewed file/command application |
| `lenso` | 1.0.0 | Standalone native executable, binary name `lenso` |

Applications using the old main-framework SDK should keep their existing 0.x
constraints. Do not replace a main-framework `[dependencies] lenso = "0.5"` with
`lenso = "1"`: the 1.0 package is the CLI executable, not the previous SDK library.
Existing old CLI APIs similarly require an explicit migration to the command,
handler, workflow and Session interfaces in this repository.

```sh
cargo install lenso --version 1.0.0 --locked
```

CLI 1.0 retains JSON ABI 1 and root project format. Host requirements in extension
manifests are checked against the independently versioned `lenso-runtime` Host
(0.2.4), not the executable's Cargo version. Existing shadcn 0.1.x releases remain
compatible with that Host line. Code 0.2.0 requires Host >=0.2.4,<0.3.0 and optional
Project >=0.1.2. This preserves existing extension/preset compatibility while the
Rust engine and executable names undergo their 1.0 product migration. The three
new independent support libraries start at 0.2.4; Project remains at 0.1.2.
Main SDK/application contract integration remains separate work.

Public Cargo path dependencies have explicit published version requirements.
`python3 scripts/release/crates.py` prints the dependency-ordered release set;
`--publish` publishes missing immutable versions with normal Cargo verification.
Repository integration fixtures/tests remain in Git; crate archives contain source
and the project README, without repository-relative test fixture dependencies.

The GitHub Actions workflow is `publish-crates.yml`, bound to environment
`crates-io`. Trusted Publishers must name owner `LioRael`, repository `lenso-cli`,
that exact workflow filename and that exact environment. Publication jobs follow
the dependency graph and request a short-lived OIDC token after distributable
verification. No long-lived Cargo token is stored in GitHub secrets.

Crates.io currently requires a normal API token for a brand-new crate's first
publication. After that bootstrap, configure its Trusted Publisher and verify the
OIDC workflow. Existing crate publishers can be configured before publishing 1.0.
Existing publisher bindings for older release lines are not automatically removed.
