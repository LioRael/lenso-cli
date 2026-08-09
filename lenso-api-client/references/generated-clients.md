# Generated clients

Select a generator that supports the Contract and consumer language. Pin the
generator through the consumer repository's normal dependency mechanism, then
record the Contract input and generation command there.

Generated files are outputs. Put authentication injection, context assembly,
retry policy, tracing, and product-friendly adapters in stable handwritten
seams around them. Regenerate after source Contract changes and review the
semantic diff before accepting it.
