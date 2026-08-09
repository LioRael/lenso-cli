# Registry evidence

After the workflow reports success, verify the public system that owns each
fact:

- registry version and archive contents for Cargo or npm;
- immutable digest and manifest for OCI;
- repository tag and GitHub Release at the exact source commit;
- provenance or attestation for the published artifact;
- dependency versions and feature or export surface in the archive; and
- a fresh install, import, invocation, or pull outside the release workspace.

Changeset or Release-plz status alone does not prove publication. Report each
artifact independently because one registry can succeed while another fails.
