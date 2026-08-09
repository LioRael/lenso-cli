# Release ownership and authority

Each Lenso repository publishes its own crates, npm packages, images, tags, and
GitHub Releases through its accepted workflow. Cross-repository compatibility
uses SemVer, Contracts, consumer updates, and integration evidence; it is not a
synchronized global release.

Group artifacts only when they form one genuine release unit. Repository write
access, local credentials, or a compatible multi-repository test do not grant
production publication authority.

Use Trusted Publishing or the repository's approved OIDC path. Do not introduce
a shadow registry, shared mutable release plan, or long-lived token as a
shortcut around the owning workflow.
