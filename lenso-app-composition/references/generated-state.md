# Generated state

Treat blueprints, addons, and capability packs as authoring inputs.
`lenso.app.json` is the one exact, revisioned App Composition and lock. Local
Adapter state and process credentials remain outside it.

Before applying a change:

- identify files owned by generation and files owned by the user;
- inspect drift rather than overwriting it;
- distinguish a safe repair from a product change;
- keep Module and Service installation actions explicit; and
- reject a stale preview after its input revision changes.

After applying a change, rerun the current inspect or validation surface.
Completion requires exact digests, dependency selections, and implementation
bindings with zero unexplained generated drift, not a successful command exit
alone.
