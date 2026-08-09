# Extraction stages

The evidence chain is ordered:

1. readiness report;
2. Extraction Plan;
3. scaffold plan and applied scaffold receipt;
4. destination expansion;
5. checkpointed backfill;
6. business reconciliation;
7. linked-versus-autonomous verification;
8. quiescence and provisional cutover; and
9. rollback or authority commit.

Each stage consumes exact prior artifacts and produces a new immutable result.
Do not skip a stage because later code or infrastructure already exists.
