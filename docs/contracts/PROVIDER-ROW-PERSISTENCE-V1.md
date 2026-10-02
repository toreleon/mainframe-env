# Provider object-row persistence, version 1

Status: **Implemented**
Owner: **Db2, IMS, MQ, and store-contract maintainers**
Scope: **provider object-row envelopes, manifests, migrations, and CAS boundaries**
Applies from: **mainframe-env 0.8.3 development**

Db2, IMS, and MQ persist independently versioned objects instead of rewriting
one provider-wide JSON snapshot. Every object payload is a strict JSON envelope
with `schema_version`, `object_key`, and `value`. The envelope schema is
provider-specific and frozen at version 1; `object_key` must exactly match the
`ProviderStateRecord` key. The store record version is the object's CAS and must
advance by one on replacement.

Each provider retains its historical namespace/key as a small manifest so an
upgrade can distinguish the legacy blob from row storage:

| Provider | Manifest schema | Principal row namespaces |
|---|---|---|
| MQ | `mainframe-env.mq-row-store@1` | queues, handle index by run, unit of work by run, replay by idempotency key |
| IMS | `mainframe-env.ims-row-store@1` | databases, session index by run, checkpoints, unit of work by run, replay by idempotency key, retained application metadata generations and selection |
| Db2 | `mainframe-env.db2-row-store@1` | tables, schemas, installations, catalog generations, provenance, legacy snapshots, unit of work by run, cursors, cursor declarations, replay by idempotency key |

The concrete namespaces use a provider prefix and `v1`; the static architecture
guard locks their names and prevents a return to whole-state serialization.
Object rows are bounded by the provider's state-byte limit and collections are
bounded by the existing provider limits.

Replay object values also retain the originating invocation deadline as the
conservative age authority for the
[durable retention lifecycle](RETENTION-LIFECYCLE-V1.md). Rows written before
that metadata existed remain readable but cannot be automatically expired.
Before each dispatch, an open provider refreshes the replay namespace from the
durable store; a retention transaction therefore releases the live replay-map
limit without requiring a restart.

## Atomic changes and in-memory ownership

A logical provider mutation computes only changed object rows and submits their
CAS puts/deletes in one `mutate_provider_states_atomic` call. Rows outside the
mutation's write/dependency set keep both their payload and record version.
Db2 conservatively advances unchanged table/schema rows that participate in a
foreign-key dependency so concurrent related-table commits cannot publish a
referential-integrity violation. Replay receipts, queues/databases/tables,
session indexes, cursors, and unit-of-work state otherwise commit together
without a provider-wide persistence record.

The in-memory maps hold large values behind `Arc`; transaction snapshots are
copy-on-write. A normal request clones map keys and references, then copies only
an object it changes. Db2 pending work contains only touched tables, and IMS undo
work contains only touched databases. This bounds temporary amplification by
the mutation scope rather than total provider state.

## Legacy migration and failure behavior

Opening a store follows exactly one of these paths:

1. No manifest and no v1 rows means an empty provider.
2. An exact v1 manifest loads strict object envelopes and validates their keys,
   counts, cross-references, indexes, cursors, and provider invariants.
3. Otherwise, the historical manifest record is decoded as the legacy state
   blob, normalized with the existing compatibility migration, and fully
   validated. Every target v1 namespace must be empty. The provider then creates
   all object rows and replaces the legacy blob with the v1 manifest in one
   backend transaction.

Unknown manifest versions, malformed envelopes, mismatched row keys, excess
rows, orphan v1 rows beside a legacy/absent manifest, or invalid reconstructed
state fail closed. The provider never treats such state as empty. A failed
atomic migration leaves the legacy manifest unchanged and publishes no partial
row set under the `ProviderStateStore` atomic-mutation contract.

Migration does not redesign public Db2, IMS, or MQ request/result contracts and
does not introduce cross-provider transactions. A deployment must retain enough
provider-state row capacity for the configured object/replay limits.

## Private MQ rich delivery import boundary

MQ's private `MQ-1505.legacy-delivery-import` planner accepts only validated,
normalized v1 row state with a retained catalog, no live handles and no pending
UOWs, including empty pending units. Empty handle indexes can be CAS-retired.
Non-quiescent conversion requires explicitly completing/retiring work through
the existing legacy authority; the planner never backs it out or discards it.

Legacy queues stored only body, message ID and correlation ID. Import preserves
those bytes and queue order exactly. Its explicit compatibility representation
is persistent, unlimited expiry, queue-default priority, absent format/group ID,
empty properties and no group/segment metadata. This preserves the legacy
persistent restart behavior; it makes no claim about historical MQMD defaults
or IBM wire representation. Entry IDs come from the existing kernel allocator.
The retained catalog, including aliases, processes and trigger identities, is
unchanged. Importing already queued data does not issue a new PUT operation.

The planner returns a composable batch: existing rich per-object rows and finite
metadata, exact legacy queue/empty-handle deletion CAS, an exact-byte retained
catalog dependency CAS, and manifest CAS to `mainframe-env.mq-row-store@2`.
Every legacy logical publication now CAS-advances the small manifest fence,
including insertion of a previously absent pending or replay row. This introduces
conservative contention between disjoint legacy writers: a stale writer gets an
explicit conflict and no partial publication, never automatic mutation replay.
Migration and any old writer have one winner. The old reader rejects the rich
marker; an already-open old writer cannot publish across its changed version.
Replay rows, digest domains, payloads, versions and retention/core references
are untouched by the import.

No open path automatically invokes this plan. The manager owns v2 reading and
single service selection, admission/lease fencing, catalog generations,
audit/effect transaction composition and participant/public acceptance. Before
publishing, that authority must exclude independent rich writers, atomically
compose all mutations and audit/replay dependencies, and adopt the returned
kernel/target generation only after commit. Missing versions, malformed source,
preexisting rich rows, narrowed limits and stale dependency CAS fail closed.
Rollback after publication requires stopping admission and restoring the
pre-import backup with its retained replay/core references; an old reader is
not a downgrade path.
