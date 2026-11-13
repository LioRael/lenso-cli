# Code module directories

The Host Registry resolves extensions and presets. Code owns functional module
identity, versions, target implementations and composition catalogs. Auth is a
Code module, not an extension bundle.

```sh
lenso init --preset code
lenso code search auth
lenso code preview auth
lenso code add auth
lenso code preview auth --refresh
lenso code update auth
```

Without explicit repository settings, setup grants access to the official module
repository and native Cargo/npm plans. It does not select a language or modify a
native manifest. Adding a module asks for language/runtime and capability providers;
Agents can provide `--language`/`--runtime` and instance-scoped `bindings` through
`--input`. Confirmation does not substitute for these domain choices.

A directory uses `format="lenso-modules-v1"`, normally `lenso.modules.toml`:

```toml
schema_version = 1
namespace = "lenso"
[modules.auth]
description = "Password authentication and sessions"
root = "auth"
[[modules.auth.releases]]
version = "0.2.0"
reference = "v0.2.0"
[[modules.auth.releases.targets]]
language = "typescript"
runtime = "node26"
catalog = "auth.typescript.toml"
```

Catalogs and templates remain ordinary composition authoring files. Versioned
implementations live in the directory's own repository at their declared refs;
transport, source grants, global caching and repository locks stay Host-owned.
Custom GitHub repositories or project-local directories use the existing explicit
Code setup inputs (`repo`/`author_root`, `catalog`, `format`, `target`, `native`).
Directory metadata cannot grant new repositories or native commands.

Code stores the target context and selected versions in `<target>/code.modules.json`.
Each family has a composition receipt below `<target>/code/modules/<namespace>`.
Repeated adds retain choices; updates refresh source pins and review owned changes.
Independent module families cannot claim the same generated output. Native
manifest ownership remains shared through the existing native receipt. Changing
language/runtime requires another target.

Existing resource directories keep their search/add/preview/update workflow.
Existing composition catalogs also accept add/preview/update using their configured
root modules and existing receipt. `compose`, `compose-preview`, author validation,
Auth presets and old generated ownership records remain supported. Matching legacy
receipts are reused rather than claiming the same generated files twice.

Preview returns the complete file/native plan, including module metadata. Only the
existing Project apply workflow writes it after consent. Module directories do not
execute generators or native database connections outside those existing flows.

## Default and convention-specific targets

Code 0.3.6 allows a target without `when` to coexist with specialized targets for
the same language/runtime. Only fully matching targets are eligible. The match
with the most convention predicates wins; equally specific matches remain an
error. A default target avoids prompting for an optional specialization.

For example, an ordinary Auth consumer selects `auth.rust.toml`, while an App
project records `conventions.app_host = "lenso-http-v1"` and selects
`auth.rust.app.toml`. Languages remain separate implementations. This selection
only reads fixed project conventions; providers and options remain flexible
composition choices. It grants no additional file, network or process access.

`paths.modules` controls the physical destination independently of a generator's
validated logical directory. The authored isolation rules still apply, and all
final paths remain under the reviewed Code target. This supports the App default
`src/modules/auth` without weakening the generator contract.
