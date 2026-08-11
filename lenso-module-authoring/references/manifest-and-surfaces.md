# Manifest and surfaces

Inspect the current public `lenso` crate and existing Module fixtures before
authoring declarations. Treat the types and manifest lint as authoritative.

For every declaration, prove its implementation partner:

- an HTTP declaration maps to a registered handler;
- a runtime function maps to a callable runtime seam;
- an Event handler maps to durable consumption behavior;
- lifecycle work maps to a real job or hook; and
- a Console Surface maps to a release-bound `console_ui_esm` artifact whose
  generated client calls the Module's Business API through Surface Gateway.

Omit empty optional surfaces. Keep stable identifiers stable, normalize and
sort declared dependencies where the current contract requires it, and use the
current lint output rather than remembered field names.
