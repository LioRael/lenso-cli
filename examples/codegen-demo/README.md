# Standalone code generation example

A user-owned Rust business contract and entry point, two standard-library storage
adapters, and extension-generated factories and assembly. No Lenso SDK or official
Registry package is used. Run with native CLI 1.0.0 and a local build of the
unpublished Code 0.2.0 extension.

From the CLI repository, build and configure a copy of this directory:

```sh
cp -R examples/codegen-demo /tmp/lenso-codegen-demo
python3 scripts/configure-code.py /tmp/lenso-codegen-demo --author-root author
cd /tmp/lenso-codegen-demo
lenso code validate
lenso code compose-preview --sample memory
lenso code compose --sample memory
cargo run
lenso code compose --sample disk
cargo run
lenso code compose --sample shared
cargo run
```

The published CLI binary can run this example. After the new
Code package is published, normal onboarding can use `lenso ext add code --input
'{"author_root":"author","target":"src"}'` instead of the development helper.
`--json`, `--answers FILE`, `--yes`, and nonterminal execution use the normal Agent
protocol. `--yes` approves file mutations but does not choose a conflict resolution.

`src/business.rs` and `src/main.rs` stay user-owned. Generated source lives only
under `src/generated`; the existing composition artifact is `src/composition.json`.
Switching to disk removes unused memory implementation/factory files after review;
customized obsolete files require a conflict decision. The shared sample creates
one storage instance and clones its `Arc` into two independently configured greeters.

Author validation loads all resources, compiles all templates, resolves every
sample and checks repeated output equality. It does not execute or type-check code.
`cargo run` compiles and exercises the generated Rust; the disk adapter creates its
`state` directory only when the generated application runs. Native compiler/package
manager operations remain explicit developer actions.
