# Host verification

Use the generated repository's current scripts and package ownership. At
minimum prove:

- the API, Worker, and Migration entrypoints compile;
- migrations can reach the configured local database;
- the host loads the expected Module manifests and Service sources;
- one request or smoke path fails if the target capability is not wired; and
- the Console Service connection is explicit when Console evidence is part of
  the requested outcome.

Report local validation separately from commit, push, pull request, package,
and deployment state.
