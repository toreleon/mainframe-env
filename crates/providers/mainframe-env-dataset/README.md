# mainframe-env-dataset

Ownership: the 0.1 dataset/catalog authority for sequential, partitioned, and
selected keyed records. Non-goals: exposing filesystem paths, HSM/RMM/SMS, or
provider internals. Allowed dependencies are owned host/execution/store
contracts and deterministic hashing.

Invariants: all names, datasets, records, members, cursors, bytes, and
idempotency records are bounded; mutations publish through optimistic durable
provider state. Verify with `cargo test -p mainframe-env-dataset`.
