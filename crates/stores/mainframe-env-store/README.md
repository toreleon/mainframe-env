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
containing directory. Artifact health performs a bounded write probe and
reports the enforced object/byte quotas, so saturation is not advertised as
ready. Public adapters include `MemoryStore`,
`SqliteStateStore`, `PostgresStateStore`, `PostgresArtifactStore`, and
`LocalArtifactStore`. Memory, SQLite, and PostgreSQL implement the same bounded
retention policy: terminal lifecycle/outbox state, resolved effects, and expired
Db2/IMS/MQ replay rows move atomically into content-verified archive batches;
live retries, unresolved effects, and checkpoint-owned rows remain protected.
Verify with `cargo test -p mainframe-env-store` and the opt-in PostgreSQL
storage-profile and retention contracts.
