# Packaging and verification

Use the package's own check and pack commands, then inspect the actual archive
contents. Prove:

- the packed service starts outside the framework workspace;
- the Provider descriptor, health, invocation recovery, and acknowledgement
  endpoints return valid current contracts;
- every live descriptor digest equals the installed release and Host lock;
- one declared route, action, runtime function, or Event path crosses the real
  host proxy or delivery boundary;
- install, status, doctor, upgrade, and rollback evidence uses the current CLI
  where relevant; and
- the Console Service shows the observed exact Module release, its authorized
  Surface, and host-owned call or Story evidence.

Report package validation separately from registry publication and deployment.
