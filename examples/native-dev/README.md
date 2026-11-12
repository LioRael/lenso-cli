# A process extension without project changes

Run `lenso --project examples/native-dev dev` from the repository root, then open
the localhost URL printed by Python's standard-library HTTP server. The operating
system chooses a free port. Ctrl-C cancels the child process group on Unix.

This installation grants two exact process invocations and no file destinations.
It can also be loaded by the Acme CLI without the optional project extension:

```sh
cargo run -p acme-example -- --project examples/native-dev toolchain version --json
```

The authored declaration is `lenso.toml`, with a visible TOML source manifest under
`lenso/extensions/`. The generated `lenso.lock` contains pinned command metadata.
Commands create the lock automatically and reuse the per-user global cache.
`.lenso/` is not used for runtime cache.
