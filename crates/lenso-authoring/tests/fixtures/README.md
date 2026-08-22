# Authoring test fixtures

These fixtures are owned by the authoring test suite. They deliberately avoid
reaching into sibling repositories after the vNext repository split.

- `contracts/` contains immutable capability contract snapshots from Lenso
  commit `67d21499548d07e92c2f6529d7c8345e58c067d9`.
- `bun/` contains the Bun clean-project provider and its generated/runtime
  TypeScript dependencies from the same split point.
- `native-greeter/` is a minimal package-source fixture. Runtime behavior is
  supplied by the pinned `lenso-native-greeter` dev dependency.

Refresh a snapshot intentionally and review its contract diff when the test
needs to cover a newer external interface.
