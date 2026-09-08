# mainframe-env-store-api

Ownership: execution, event, work, checkpoint, session, artifact, generation,
idempotency, and bounded retention store contracts. Non-goals: SQL, filesystem
paths, object-store types, or a concrete transaction mechanism. It depends only
on execution contracts.

Invariants: state transitions are monotonic; artifacts are immutable; work is
at-least-once; unknown mutation outcomes stay explicit; and retention moves
only expired, recovery-independent rows into a verifiable archive transaction.
Verify with `cargo test -p mainframe-env-store-api`.

`ArtifactStore::health` fails closed by default. A production artifact
authority proves readable and writable access and reports every object-count or
aggregate-byte quota it enforces; readiness requires headroom in each reported
dimension.
