# mainframe-env-host-api

Ownership: typed host and CICS requests/results, effect metadata, capability
descriptors, and immutable registry snapshots. Non-goals: concrete providers,
database rows, async runtimes, or broad application state. It depends only on
the execution contract.

Invariants: every request/result is bounded; mutations carry effect sequence
and idempotency identity; capability resolution is deterministic. Verify with
`cargo test -p mainframe-env-host-api`.
