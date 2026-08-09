# Generated state

Treat the blueprint, addons, capability packs, workspace manifest, generated
state records, and the current change plan as separate evidence.

Before applying a change:

- identify files owned by generation and files owned by the user;
- inspect drift rather than overwriting it;
- distinguish a safe repair from a product change;
- keep Module and Service installation actions explicit; and
- reject a stale plan after its input state changes.

After applying a change, rerun the current diff or explain surface. Completion
requires zero unexplained generated drift, not a successful command exit alone.
