# Compatibility classification

A change is compatible only when every supported Consumer can process the new
producer output and every supported producer can satisfy the new Consumer
behavior where bidirectional compatibility is required.

An additive field can still be breaking when it becomes required, changes
authorization, alters default behavior, affects signatures or digests, or
reaches a strict Consumer. A renamed field, reused identity, changed Event
meaning, narrowed enum, different retry semantics, or reinterpreted Workflow
version requires a parallel major unless the Contract explicitly proves
otherwise.

Deprecation preserves the old meaning through a declared window. Retirement is
a protected mutation justified by current Consumer evidence, not the passage
of time alone.
