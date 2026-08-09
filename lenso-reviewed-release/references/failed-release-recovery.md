# Failed release recovery

Begin from current public registry state, not the workflow's intended plan.
Classify every artifact as already published, missing, or inconsistent. Public
versions, tags, archives, and changelogs are immutable history.

Repair prerequisites, permissions, ordering, or workflow association, then
resume only unpublished artifacts through the owning repository workflow. Do
not bump or republish an artifact merely to make a failed workflow look green.

Any one-use recovery machinery must be removed after the normal path is proven.
Reverify registry bytes, tag, provenance, and fresh installation before calling
the release recovered.
