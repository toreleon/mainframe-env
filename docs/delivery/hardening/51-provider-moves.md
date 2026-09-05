# Provider-state move contract hardening (#51)

The corrected surface is `move_provider_state` and the `Move` member of
`mutate_provider_states_atomic`, across memory, SQLite and PostgreSQL. This is
not blanket parity certification of all storage operations.

All six entry paths use the store contract's `ProviderStateRecord::validate_move`.
Empty namespace/source/destination identifiers, equal source/destination,
nonpositive or nonsuccessor versions, and versions outside the shared positive
signed-64-bit SQL range are `Conflict`. A valid-shape record exceeding the
configured per-record payload bound is `PayloadTooLarge`. Missing/stale source
and occupied destination are compare-and-swap `Conflict` on every backend.

This deliberately makes the memory missing-source error match the SQL CAS
contract (previously `NotFound`) and the SQL oversized-payload error match
memory (previously `Conflict`). Callers must not interpret a CAS conflict as a
successful retry. No affected error path publishes a partial move. A later
failure in a mixed atomic transaction rolls back earlier puts and moves.

No schema migration, automatic deletion or rewrite of existing rows is performed.
Historical malformed rows remain readable through existing inspection APIs;
repair requires an explicit operator-reviewed migration. Routine moves do not
silently normalize empty identifiers or change their interpretation.

The same black-box cases run against all implementations in
`tests/provider_move_contract.rs`. PostgreSQL is an explicitly ignored test
outside a provisioned database tier and MUST be run explicitly before granting
PostgreSQL parity credit:

```sh
cargo test --locked -p mainframe-env-store
MAINFRAME_ENV_TEST_POSTGRES_URL=... cargo test --locked -p mainframe-env-store \
  --test provider_move_contract postgres_move_contract -- --ignored --exact
```

The suite checks typed errors, exact retained state, payload boundaries,
version boundaries, stale/missing/colliding moves, retry behavior, multi-move
sequences and transaction rollback. Receipt logs must identify the candidate,
toolchain, and actual PostgreSQL service image; a skipped test is not a pass.
