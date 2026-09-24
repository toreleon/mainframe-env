# CIC-906 durable-profile validation slice

This bounded validation slice was authored from `ec6225b0` and integrated on
the sealed `3f749203` aggregate with 175 typed, 0 legacy, and 88 unready CICS
application routes. It adds no command semantics, registry
entries, schema migration, or final-candidate credit. The full 0.9 backend,
licensed differential, and release gates remain pending.

## Contract inventory

| 0.9 boundary | Already present at the base | Added in this slice |
|---|---|---|
| Shared immutable artifacts | `postgres_quota_and_shared_artifact_contract` proves competing adapters, content conflict, quota, rollback, reopen, and corruption rejection. | `bts_selected_link_reconciles_outer_receipt_after_postgres_reopen` binds separate PostgreSQL state and artifact adapters on both CICS service opens, validating the installed program artifact before replay. |
| Concurrent ownership and fencing | `postgres_work_deadlines_and_fencing_contract`, `concurrent_global_enqueue_uses_one_postgres_owner_and_fifo_promotion`, `concurrent_task_association_uses_postgres_cas_and_reopen`, and BTS child ownership cover durable CAS and stale owners. | No duplicate test. |
| Restart and unknown-outcome replay | `postgres_durable_resume_blocks_until_reconciled`, `postgres_stale_effect_recovery_known_success_is_not_redispatched`, BTS LINK/child and remote refusal PostgreSQL reopen selectors exist. SQLite has a fault-injected cross-family crash/replay selector. | The BTS LINK PostgreSQL selector now reopens both independent adapters and verifies that the outer receipt does not redispatch the selected link. |
| Schema and read versions | PostgreSQL migration and executable metadata codecs have focused selectors; the storage contract checks migration startup and incompatible quotas. | `postgres_artifact_read_versions_are_compatible_and_fail_closed` reads a committed schema-v1 artifact beside a schema-v2 executable artifact across adapter reopen, and rejects unknown or malformed row versions. Migration rollback remains the documented stop-admission/backup restore operation. |
| Replay and checkpoint retention | `postgres_retention_contract` covers core checkpoint, audit, effect, and lifecycle protection plus reopen; Memory and SQLite have CICS replay migration validation. | `*_checkpoint_protects_cics_replay_until_release` checks the store's `CicsReplay` deletion target: a checkpoint rejects a provider-supplied candidate, and release permits deletion on Memory, SQLite, and PostgreSQL. Provider codec and dependency selection remain separately tested. |

Run each ignored PostgreSQL selector with `--ignored --exact` and an explicit
`MAINFRAME_ENV_POSTGRES_TEST_URL` pointing at a task-owned PostgreSQL 18.6
database. Reset only that database between selectors because the storage
compatibility tests intentionally leave corrupt rows. `MAINFRAME_ENV_TEST_POSTGRES_URL`
is the separate environment name for generic store lease and effect selectors.
The PostgreSQL artifact adapter uses the same database as the state adapter;
no node-local artifact directory participates in this profile.

These selectors establish local adapter and CICS contract behavior only. An
adapter reopen in one test process does not claim a separate-process crash
result. The SQLite fault-injection selector and generic PostgreSQL durable
resume selector remain distinct evidence. Full product composition, all
command-family coverage, licensed IBM differential, and exact final-candidate
acceptance remain pending under the 0.9 matrix.

## Separate-process BTS LINK validation

`tools/cics_postgres_process_restart.sh` creates a task-owned temporary
PostgreSQL 18.6 cluster, socket, port and database, runs the selected test and
the earlier same-process reopen control, then stops only that cluster and clears
this checkout's Cargo target. The ignored
`bts_selected_link_reconciles_after_postgres_process_exit` selector launches two
exact test-binary children. After a selected `LINK ACQPROCESS` has dispatched
once and persisted its outer CICS replay receipt, the first child observes the
injected `UnknownOutcome`, writes its PID to the task scratch directory and
exits with code 86 without dropping service adapters. The parent bounds each
child to 45 seconds and can terminate only its own child handle. A new process
opens the same PostgreSQL state and artifact adapters, rejects replay under a
different execution owner, and reconciles the original request. It checks the
unchanged BTS process row, zero restart-side program dispatches, retained outer
replay and audit records, and identical executable artifact bytes and metadata.
This LINK path does not create a task checkpoint, so checkpoint retention remains
covered by the dedicated backend selectors above. On the 184 typed / 79 unready
candidate, this is one selected typed route. It does not establish full
cross-family or process-crash coverage, licensed differential, or release credit.
