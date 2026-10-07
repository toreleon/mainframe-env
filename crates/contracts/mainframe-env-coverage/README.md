# mainframe-env-coverage

Ownership: immutable official-coverage evidence, the shared thin Conformance IR
v1, independent six-gate row projections, and verdict-derived ledgers.
Non-goals: IBM publication extraction, product semantics, provider routing, or
claiming compatibility from catalog presence. The v1 IR owns typed row specs,
mandatory obligations, bounded registry references, executable bindings,
replayable verdicts, deterministic shard/cache identities, and the separate
bounded `ScenarioSpec` cross-subsystem binding boundary.

Invariants: a row is complete only when every applicable gate's latest retained
evidence passes; differential pass evidence names a licensed pinned oracle;
evidence records are content-addressed and never replaced; and a baseline's
catalog digest and denominator cannot change inside an existing store. Generated
rows begin with zero numerators. Verify with
`cargo test -p mainframe-env-coverage --locked` and
`cargo xtask coverage --check`. From 0.3 onward also run
`cargo xtask spec --check`.

## Documentation

[Documentation portal](../../../docs/README.md) · [Current package map](../../../docs/architecture/PACKAGE-MAP.md) · [Embedding guide](../../../docs/guides/EMBEDDING.md) · [Contribution and verification](../../../CONTRIBUTING.md)
