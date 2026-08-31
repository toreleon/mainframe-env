# mainframe-env-host-api

Ownership: typed host and CICS requests/results, effect metadata, capability
descriptors, generated official semantic identities, and immutable capability
and subsystem-handler registry snapshots. Non-goals: concrete providers,
database rows, async runtimes, broad application state, or deriving semantic
coverage from generated identities. It depends only on the execution contract.

Invariants: every request/result is bounded; mutations carry effect sequence
and idempotency identity; capability resolution is deterministic; official and
custom semantic namespaces cannot overlap; and no generated identity installs a
handler. Verify with `cargo test -p mainframe-env-host-api` and
`cargo xtask semantic-identities --check`.
