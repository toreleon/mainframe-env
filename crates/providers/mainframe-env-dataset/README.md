# mainframe-env-dataset

Ownership: the dataset, VSAM, catalog, allocation, abstract-volume, lifecycle,
locking, and recovery authority. Non-goals: exposing filesystem paths, claiming
physical/tape/SMS capabilities that an adapter has not implemented, or leaking
provider internals. Allowed dependencies are owned host/execution/store
contracts and deterministic hashing.

Invariants: all definitions, names, datasets, records, members, cursors, bytes,
locks, and idempotency records are bounded; mutations publish through optimistic
durable provider state; unsupported attributes return an exact capability
diagnostic. Verify with `cargo test -p mainframe-env-dataset` and
`cargo xtask dataset-contract --check`.
