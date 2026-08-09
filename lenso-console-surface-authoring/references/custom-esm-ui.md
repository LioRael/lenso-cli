# Custom ESM UI

Use `@lenso/console-module-api` for the framework-neutral manifest and typed
host operations. Use `@lenso/console-ui` for the React adapter, Surface root,
shared primitives, and typed StyleX slots. Inspect the packages' current
exports before importing symbols.

The default ESM export must be the validated Console UI Module. Its manifest
must match the release receipt, and its component map must contain every
declared Surface id exactly once.

Root the page in the public Surface boundary so height, theme, managed context,
and Shell behavior propagate correctly. Use semantic Console tokens and public
StyleX slots. Keep global CSS, deep imports, Shell internals, and workspace-only
aliases outside a publishable Module UI.

The ESM artifact is trusted same-realm execution, not a browser sandbox. Keep
capability review, artifact review, and runtime authorization explicit.
