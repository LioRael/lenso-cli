# Contract discovery

Search committed contract directories, package exports, generated artifacts,
and the exact producing handler or Service Contract. Prefer the most specific
artifact that owns the operation:

- OpenAPI for host or HTTP surfaces;
- a Service Contract plus its HTTP binding for Autonomous Service HTTP;
- Protobuf for a declared RPC transport;
- shared context and error schemas for cross-cutting envelopes; and
- the GA Support Manifest for supported component combinations.

If generated output and the producing source disagree, stop and regenerate
from the source before changing the consumer.
