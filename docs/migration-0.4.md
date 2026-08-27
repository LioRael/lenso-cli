# Migrating to the intent-level CLI

> Historical note: this page documents the 0.4 transition. The current public
> workflow is `lenso plugin new → dev → check → pack`; built-in Module authoring
> now lives explicitly under `lenso module`. See
> [Plugin authoring migration](migration-plugin-authoring.md).

Cargo `lenso-cli` 0.4 and npm `@lenso/cli` 0.12 intentionally reduce the
public command surface. Ordinary Module authors now work with four top-level
commands; App Definition editing remains under `lenso app`.

## Command replacements

| Before | Now |
| --- | --- |
| `lenso module create NAME` | `lenso new NAME` |
| `lenso module dev` | `lenso dev` |
| `lenso module check` | `lenso check` |
| `lenso module verify` | `lenso verify` |
| `lenso add ...` | `lenso app add ...` |
| `lenso resolve ...` | `lenso app resolve ...` |

Top-level `run` and the `compose` command group have no direct replacement.
They exposed internal recipe, Plan, and Runner stages as an ordinary authoring
workflow. Product-owned Hosts now run reviewed immutable Plans; App owners use
the source-derived App Definition commands only when they need to inspect or
materialize those Plan bytes.

## Module workflow

```sh
lenso new greeting
cd greeting
lenso check
lenso dev
lenso verify
```

`check` is the fast diagnostic command. `verify` adds behavior, lifecycle, and
removal evidence. Existing project files remain valid inputs to these commands;
the migration changes the command interface rather than the Plan format.

## App Definition workflow

```sh
lenso app add example-greeting-module --definition lenso.app.json
lenso app check --definition lenso.app.json
lenso app resolve --definition lenso.app.json \
  --output .lenso/resolved-plan.json
lenso app remove greeting --definition lenso.app.json
```

Do not copy generated Module Descriptors, endpoint tables, or unambiguous
bindings into the App Definition. Keep only package selections, stable Instance
keys, configuration or secret references, optional lanes, and real ambiguity
decisions.

Large static Module settings may move from inline `configuration` to a reviewed
`config/modules/<instance>.toml` file referenced by `configuration_file`. Do
not set both fields. Resolution loads the TOML as the same Module configuration
overlay, applies package defaults, validates the package Schema, and emits only
the completed configuration into the immutable Plan.
