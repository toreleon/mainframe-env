# #57 — explicit effect digests and typed budgets

## Inventory and disposition

The execution coordinator's host-request and host-result SHA-256 preimages were
Debug strings. Both now use the frozen host canonical encoder. ScopedHostService
used Debug lengths for provider request/result bounds; both now use the same
streaming typed representation. Existing typed HostLimits remain in force.

EffectRecord consumers are the coordinator, memory-store direct and transactional
journal paths, the shared SQLite/PostgreSQL durable adapter, and explicit unknown
effect reconciliation. Every result transition checks the encoding domain.
SQLite/PostgreSQL use version-aware JSON in the existing durable-effect namespace;
there is no unrelated SQL-table migration or change to deduplication keys.

Checkpoint/artifact SHA-256 already hashes actual bytes and remains unchanged.
Lifecycle outbox notification rendering still uses a diagnostic event-kind label:
that payload is not an effect digest or a payload-size authority. The installed
CALL replay protocol's explicitly ordered JSON fingerprint and cached successful
reply protocol remain separate, unchanged compatibility domains.

## Contract and verification

The normative byte layout, limits and legacy/downgrade policy are in
`docs/contracts/EFFECT-CANONICAL-V1.md`. Candidate-specific commands, toolchain,
logs and receipts belong to the linked implementation PR and its verification
artifact; this document does not turn unexecuted commands into passing evidence.

Reproduce with the pinned toolchain:

```
cargo test --locked -p mainframe-env-host-api -p mainframe-env-store-api -p mainframe-env-store -p mainframe-env-interpreter -p mainframe-env-server
cargo clippy --locked -p mainframe-env-host-api -p mainframe-env-store-api -p mainframe-env-store -p mainframe-env-interpreter -p mainframe-env-server --all-targets -- -D warnings
python3 -B tools/check_effect_encoding.py
```

The server suite exercises #47 state ownership, #49 uncertainty/reconciliation,
and #55 actual subprocess exit/replay together. Reopened unknown receipts must
retain their canonical format and the exact canonical unknown-result digest.
The backend contract tests compare memory/SQLite transitions, reject mixed-domain
results/reconciliation, and reopen a genuine schema-1-shaped legacy receipt.

PostgreSQL is a separate explicit gate, not credited by an ignored test:

```
cargo test --locked -p mainframe-env-store --test effect_encoding_contract postgres_effect_domains_cannot_be_mixed -- --ignored --exact
```

Set MAINFRAME_ENV_POSTGRES_TEST_URL to an isolated disposable PostgreSQL instance.
The ordinary architecture guard rejects regression to Debug-based effects/bounds.
These tests establish local simulator behavior, not licensed IBM equivalence or
release acceptance of epic #46.
