# Code project configuration

`lenso.code.toml` belongs to the Code extension. It records project conventions and
preferences; Host configuration remains in `lenso.toml`. SDK code is unchanged.

Use commands rather than editing internal installation receipts:

```sh
lenso code config --language typescript --runtime node26
lenso code config --modules-path src/modules --import-alias '@/modules'
lenso code config --convention styles=tailwind rsc=true
lenso code config --provider lenso.auth-store=postgres
lenso code config
lenso code preview auth
lenso code add auth
```

Writes use the existing Project plan/approval flow. `code config` without settings
is read-only. JSON/Agent clients can replace settings using
`--input '{"settings":{"project":{"language":"typescript","runtime":"node26"}}}'`.
Individual flags update existing settings; mixing flags and replacement JSON is an
error. Older installations first upgrade Code with `lenso ext add code --constraint '^0.3.3'`.
Review `lenso ext configure code` if access is still missing, to grant
read/write access to the new root config file. The extension never bypasses grants.

```toml
schema_version = 1
[project]
language = "typescript"
runtime = "node26"
[paths]
modules = "src/modules"
import_alias = "@/modules"
[conventions]
styles = "tailwind"
rsc = true
[defaults.providers]
"lenso.auth-store" = "postgres"
[modules.auth.options]
ttl_ms = 60000
```

Project facts/conventions are fixed context. Conventions accept strings, booleans
and integers; booleans/integers become their string representation in composition
context. Templates read `resolution.context.styles`, `import_alias` and
`modules_path`. Module output roots must stay inside the configured Code target;
changing target grants requires explicit Code setup. File imports are adapted only
by author templates, not by arbitrary rewriting of handwritten application code.

For fixed implementations, directory targets can declare scalar conditions:

```toml
[[modules.widget.releases.targets]]
language = "typescript"
runtime = "node26"
catalog = "widget.tailwind.toml"
when = { styles = "tailwind", rsc = true }
```

Targets must match project conventions. Catalog `supports` constraints validate
context, and templates can use it for file layout/content. Flexible adapters still
use capability slots. Explicit instance settings and saved choices take precedence
over project defaults; project defaults fill unresolved choices before author
option defaults. An ineligible provider preference produces a diagnostic when a
choice is needed. It cannot silently switch an installed provider.

Convention changes report affected module IDs in config plans. Existing modules
reject contradictory saved context until reviewed explicitly:

```sh
lenso code preview widget --reconfigure
lenso code add widget --reconfigure
```

Preview does not write files. Apply uses existing ownership/conflict handling and
native dependency reconciliation; changing fixed catalogs preserves the original
native ownership identity. Language/runtime changes require another project target.
This is not data migration, a framework detector or a platform installer.
