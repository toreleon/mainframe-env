# mainframe-env-coverage

Ownership: immutable official-coverage evidence, independent six-gate row
projections, denominator-bound snapshots, and append-only ledger generations.
Non-goals: IBM publication extraction, semantic handlers, runtime dispatch, or
claiming compatibility from catalog presence.

Invariants: a row is complete only when every applicable gate's latest retained
evidence passes; differential pass evidence names a licensed pinned oracle;
evidence records are content-addressed and never replaced; and a baseline's
catalog digest and denominator cannot change inside an existing store. Generated
rows begin with zero numerators. Verify with
`cargo test -p mainframe-env-coverage --locked` and
`cargo xtask coverage --check`.
