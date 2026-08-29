# mainframe-env-store

Ownership: bounded in-memory stores in ME.V1 and durable SQL/artifact adapters
in later phases. Non-goals: business semantics or gateway state. Allowed
dependencies are owned store/execution contracts; infrastructure dependencies
remain private adapters when added.

Invariants: capacity is reserved before mutation, immutable artifacts never
overwrite, event order is monotonic, and idempotency conflicts fail closed.
Public surface: `MemoryStore` and `StoreLimits`. Verify with
`cargo test -p mainframe-env-store`.
