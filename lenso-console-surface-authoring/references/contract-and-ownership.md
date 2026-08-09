# Contract and ownership

Resolve four owners before editing:

- the framework or Module repository owns typed Module and Surface declarations;
- the Module UI package owns component implementation;
- the Managed Service owns business reads and mutations; and
- the Console repository owns the Shell, host API, loader, quarantine, shared
  UI primitives, and Console Service operations.

Do not move code because its filename contains `console`. Move or edit it only
when its authoritative state and responsibility belong to the target owner.

Keep Module id, Surface id, path, area, navigation, required capabilities, and
presentation stable across declaration, generated manifest, UI export, Module
Release, receipt, and loaded component mapping.
