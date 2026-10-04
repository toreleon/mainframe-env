# Durable storage profile

- Status: **Implemented**
- Applies from: mainframe-env 0.8.3 hardening
- Owner: store adapters and core-server composition
- Scope: PostgreSQL quotas, shared immutable artifacts, and local publication durability

## Audited provider publication

The deliberately configured synchronous root-publication framework additionally
uses JournalStore's bounded core-owned admission/enrollment/Closing and one
atomic terminal publication. Memory and SQLite compose original legal core
steps, exact lifecycle outbox, provider settlement and two typed terminal audit
subjects under their existing single lock/transaction, with whole graph/epoch/
Exact-or-Absent dependency checks. Other backends refuse without fallback.
This prerequisite does not confer pending-PUT/removed-GET native root,
recovery or participant acceptance.
Versioned ownership/audit subjects, drain/backup/downgrade requirements and
remaining proof boundaries are specified in
[ADR 0034](../decisions/0034-mq-root-terminal-publication.md).

Memory and SQLite implement the additive
`ProviderStateStore::publish_provider_states_audited` boundary described in
[ADR 0029](../decisions/0029-audited-provider-publication.md). Its request carries
the exact observed canonical coordinator intent, typed audit, finite observed
tick and at most 4,096 provider puts/deletes/moves. Empty mutations permit an
audit-only decision; a denial cannot mutate rows. PostgreSQL currently returns
the default `InvalidTransition` without mutation. Sequential fallback is forbidden.

The physical lock/transaction asserts entire retained-intent equality and the
current running execution/run/attempt/principal attribution. The audit must
match the intent's capability, resource digest/format, invocation key and effect
sequence. Missing, legacy, terminal, recovered, stale, expired or mismatched
ownership fails closed. Observation must equal the audit tick, be at or after
positive creation and before the finite recovery boundary, fit the signed SQL
clock domain, and not regress the retained logical clock or execution lease.
Trusted authorization and cancellation remain outside the store.

Memory uses its existing touched-entry undo log and typed audit authority.
SQLite reads the existing core effect/execution rows under its writer lock and
uses the existing row loop and audit row codec in that same transaction. Audit
payloads and ordinals retain `AuditSink` compatibility. SQLite audits consume
its shared provider-table row/payload quotas; Memory retains its separate audit
count/encoded-payload limits and provider byte quotas. Any row CAS, quota,
encoding, epoch or clock failure restores provider/audit rows and all affected
counters. Core result, lifecycle and outbox completion remains coordinator-owned;
the mutation batch cannot target core `durable-` or legacy clock identities.

No schema migration or canonical/durable byte change is required. The older
row and audit APIs remain available, so service rollout must stop legacy
sequential writers before claiming atomic publication. Callers supply their own
CAS-protected UOW/replay result dependency; this method is not another dispatcher
or an intent-wide deduplication protocol. MQ service composition and PostgreSQL
execution remain pending, and this primitive grants no participant acceptance.

## PostgreSQL quotas

`PostgresStateStore` records the configured `provider-state` row bound in
`store_quota`. `PostgresArtifactStore` records an independent
`artifact-object` bound. A count-changing operation locks its quota row inside
the same transaction, verifies the immutable configured maximum, adjusts used
capacity, performs the data mutation, and commits both together. Conflicts,
capacity failures, and aborted transactions explicitly roll back before their
connection is returned.

On first quota-aware open, the adapter locks and initializes the row from the
actual legacy table count. Later opens require the same maximum and exact
recorded/actual count. A missing row, incompatible maximum, negative count,
count drift, or legacy count above the maximum fails closed. Deployments must
drain pre-quota writers before upgrading.

## Migration and compatibility

The SQLite state profile is at `0002-retention-lifecycle`. The PostgreSQL
profile is at `0003-executable-artifact-metadata`: the state adapter applies
`0001`, `0002`, then `0003`, while the shared-artifact adapter applies its
relevant `0001` and `0003` migrations. Both adapters may race at startup;
`0003` takes an access-exclusive table lock and rechecks its versioned
constraint marker, so exactly one compatible schema remains.

PostgreSQL schema-v1 artifact rows remain readable. New artifact publications
use schema v2 and persist the bounded executable metadata envelope. Before the
first v2-capable startup, drain older writers and take a database backup. The
expand migration is idempotent but is not a promise that an old binary can
safely resume writes: rollback requires stopping admission and restoring the
pre-migration backup. Never drop `executable_metadata` while any installed
artifact or durable continuation can reference its manifest.

## Shared immutable artifacts

The PostgreSQL product profile uses the dedicated `artifact_object` table,
never `artifact_root`. Objects are keyed by their lowercase `sha256:` identity
and store raw payload bytes, media type, digest, and schema version. Publication
is one `INSERT ... ON CONFLICT DO NOTHING` under artifact quota reservation.
Concurrent identical publication is idempotent; a different envelope for the
same content address conflicts. Every read recomputes the payload digest and
rejects malformed schema, media, digest, key, or oversized payload. Separate
adapter and product instances observe the same committed object.

`ServerConfig` accepts `artifact_profile = "local" | "shared"` and the
`MAINFRAME_ENV_ARTIFACT_STORE` override. Memory and SQLite require `local`;
PostgreSQL requires `shared`. Invalid pairings fail configuration validation,
and PostgreSQL startup never silently creates a node-local artifact tree.

## Local publication

`LocalArtifactStore` writes a unique temporary file beside the destination,
syncs its bytes, and atomically hard-links it to the final digest path. A hard
link cannot replace an existing name. The adapter syncs the containing
directory after publication and temporary removal, and syncs the object-root
directory when a new digest-prefix directory is created. A crash can leave only
an unreferenced temporary file; it cannot expose a partial final object or
replace an already-published object.
