# mainframe-env-store

Ownership: bounded in-memory stores in ME.V1 and durable SQL/artifact adapters
in later phases. Non-goals: business semantics or gateway state. Allowed
dependencies are owned store/execution contracts; infrastructure dependencies
remain private adapters when added.

Invariants: capacity is reserved before mutation, immutable artifacts never
overwrite, event order is monotonic, and idempotency conflicts fail closed.
PostgreSQL uses transaction-locked quota rows for both state and dedicated
shared artifact objects; incompatible limits or count drift fail on open. Local
artifacts publish through an atomic no-replace link after file sync and sync the
containing directory. Public adapters include `MemoryStore`,
`SqliteStateStore`, `PostgresStateStore`, `PostgresArtifactStore`, and
`LocalArtifactStore`. Verify with `cargo test -p mainframe-env-store` and the
opt-in PostgreSQL storage-profile contract.
