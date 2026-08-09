# Packaging and verification

Use the package's own check and pack commands, then inspect the actual archive
contents. Prove:

- the packed service starts outside the framework workspace;
- manifest and provided Module endpoints return valid current contracts;
- one declared route, action, runtime function, or Event path crosses the real
  host proxy or delivery boundary;
- install, status, doctor, upgrade, and rollback evidence uses the current CLI
  where relevant; and
- the Console Service shows the provided Module and host-owned call evidence.

Report package validation separately from registry publication and deployment.
