# Plugin identity v1 migration

New Plugin projects and catalog entries require a namespaced Plugin ID such as
`company.uppercase`. Each dot-separated label starts with a lowercase ASCII
letter, ends with a lowercase letter or digit, and may contain lowercase
letters, digits, or hyphens. Labels contain at most 63 bytes and the complete
Plugin ID at most 253 bytes. Release versions are exact Semantic Versions.

Existing source projects with a legacy unnamespaced ID remain readable by
`plugin check`, `plugin dev`, and `plugin pack`. The CLI prints a migration
warning instead of silently rejecting those files. Existing App roots remain
readable and retained legacy Releases remain rollback-capable; adding or
replacing a Bundle is a new identity boundary and therefore requires v1.
Rename the Plugin ID only as a coordinated Contract migration: update the
source manifest, package name when appropriate, Host Catalog reference, Plugin
Root directory, Instance references, and any catalog Release as one reviewed
change.

The machine-consumable contract and cross-language acceptance cases live at:

- `contracts/plugin-identity-v1.schema.json`
- `contracts/plugin-identity-v1.conformance.json`

Catalog implementations should vendor the files exactly and run the
conformance vectors in their own validator tests.
