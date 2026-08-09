# Ownership boundaries

Assign one authority for every mutable business fact. A capability owns a fact
when it defines its lifecycle, validates changes, persists the authoritative
state, and emits the evidence other capabilities consume.

Keep objects together when they share the same owner, lifecycle, permission
model, store, and failure boundary. Split them when they can be installed or
disabled independently, have different business owners, cross a trust boundary,
or require independent deployment and recovery.

Model collaboration through declared dependencies, public APIs, Events,
host-owned delivery rails, or generated Contract clients. A direct import of
another Module's internals or direct access to its tables is unresolved
ownership, not collaboration.

For imported or synchronized data, name both the external authority and the
local projection. State which side resolves conflicts and what happens when
the authority is unavailable.
