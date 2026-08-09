# First useful slice

A first slice is a tracer through the real system, not a list of CRUD screens.
It includes:

- one primary actor and one valuable outcome;
- the smallest authoritative data needed for that outcome;
- one meaningful lifecycle transition;
- authorization for both entry and mutation;
- one failure or denial path;
- one check that fails when the capability is not wired; and
- visible evidence in the configured Console Service when the capability is
  intended to be operated there.

Defer secondary actors, bulk operations, generalized policy engines,
marketplace behavior, and speculative service splits unless the outcome cannot
be proved without them.
