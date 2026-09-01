# mainframe-env-coverage

Ownership: immutable official-coverage evidence, the shared thin Conformance IR
v1, independent six-gate row projections, and verdict-derived ledgers.
Non-goals: IBM publication extraction, product semantics, provider routing, or
claiming compatibility from catalog presence. The v1 IR owns typed row specs,
mandatory obligations, bounded registry references, executable bindings,
replayable verdicts, and deterministic shard/cache identities.

Invariants: a row is complete only when every applicable gate's latest retained
evidence passes; differential pass evidence names a licensed pinned oracle;
evidence records are content-addressed and never replaced; and a baseline's
catalog digest and denominator cannot change inside an existing store. Generated
rows begin with zero numerators. Verify with
`cargo test -p mainframe-env-coverage --locked` and
`cargo xtask coverage --check`. From 0.3 onward also run
`cargo xtask spec --check`.
