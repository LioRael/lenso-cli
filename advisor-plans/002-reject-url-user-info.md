# Reject catalog URL user-info

Status: IMPLEMENTED AND VALIDATED

Finding: HTTPS catalog and bundle URLs currently accept user-info, allowing credentials to enter command output and network error diagnostics.

Scope:
- reject username/password components before requests;
- disable automatic redirects and validate each resolved relative or absolute
  target before the next request, with a maximum of three redirects;
- keep diagnostics free of the rejected URL value;
- add HTTPS and loopback credential regressions.

Implementation: catalog fetches now follow redirects manually, validate scheme,
loopback policy, and user-info before every request, and map transport failures
to non-echoing diagnostics.

Validation: `redirect_user_info_is_rejected_before_the_target_is_contacted`
proved a rejected redirect target received no request and leaked no credential;
URL parsing regressions and the full CLI workspace/release validation passed.
