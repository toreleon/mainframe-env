# mainframe-env-store-api

Ownership: execution, event, work, checkpoint, session, artifact, generation,
and idempotency store contracts. Non-goals: SQL, filesystem paths, object-store
types, or a concrete transaction mechanism. It depends only on execution
contracts.

Invariants: state transitions are monotonic; artifacts are immutable; work is
at-least-once; unknown mutation outcomes stay explicit. Verify with
`cargo test -p mainframe-env-store-api`.
