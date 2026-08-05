# Changesets

Run `pnpm changeset` for every user-facing npm CLI package change. The
Changesets workflow creates the version pull request; merging it builds the
four platform binaries and publishes `@lenso/cli` through npm Trusted
Publishing.

Configure the npm Trusted Publisher for `@lenso/cli` before the first live
publish after this migration. Cargo publication is handled independently by
Release-plz.
