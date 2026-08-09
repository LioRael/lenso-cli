# GA support boundary

The released GA Support Manifest is the authority for compatible component,
Contract, adapter, and state-format combinations. Read it from the installed
distribution or current repository artifact, then use the current CLI support
check. SemVer proximity is not compatibility evidence.

A host-managed Provider remains a different runtime category. It relies on the
host for installation, delivery, and visibility. An Autonomous Service owns
those operational meanings and must not be created by renaming Provider
metadata.

When a combination is unsupported, return the exact issue and supported next
action. Do not deep-import workspace internals to manufacture support.
