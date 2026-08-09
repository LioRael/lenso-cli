# Failure, compensation, and evolution

Distinguish an attempted call, an accepted command, and a completed durable
effect. Compensation can be selected only from recorded completed effects and
must have its own declared completion evidence.

Retries address transient execution failure; compensation addresses completed
business effects that must be reversed. Neither creates a distributed
transaction.

Definition evolution preserves the meaning of existing versions. In-flight
migration requires a deterministic mapping of current state, history,
identities, pending timers, children, effects, and compensation. Unknown or
ambiguous mappings block migration.
