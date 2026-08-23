# Evolution and verification

## Evolve deliberately

- Patch releases preserve the contract shape.
- Additive compatible changes advance the Descriptor minor version only when
  existing consumers and providers remain valid.
- Breaking role or shape changes create a new major identity.
- Module package releases do not silently change the Capability identity.

Run the owning code generator's compatibility lint against the previous
accepted Descriptor. Record intentional version decisions beside the contract
change rather than inferring them from generated diffs.

## Prove the contract

Require evidence for every changed Operation:

- Descriptor and Schema validation;
- deterministic generation and a freshness check for committed artifacts;
- typed consumer and provider compilation;
- success, domain-error, and runtime-failure preservation;
- cross-language wire vectors for portable contracts; and
- stream or event terminal, cancellation, backpressure, and partial-admission
  behavior when those interaction kinds are present.

The proof is complete when changing the Descriptor without regenerating makes
the freshness gate fail and at least one consumer-provider path exercises the
new contract.
