# API client verification

Prove:

- generation from the committed Contract is reproducible;
- the client uses the exact method or RPC and serialized field names;
- success and standard error envelopes decode correctly;
- authentication and required context reach the producer;
- absolute deadline and idempotency behavior survive retries and hops;
- unsafe calls are not retried; and
- a fixture, contract test, or live local call fails when the operation drifts.

When the producer changed, run its generator and freshness checks before the
consumer check. Report producer and consumer validation separately.
