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

## Private MQ stored-authority reader

`MQ-1505.rich-service-state-reader` prepares private single-service composition.
It captures one bounded `list_provider_state_prefix("mq-", limit + 1)` snapshot
and decodes only those records, returning exactly one normalized v1 legacy
authority or v2 rich authority. Memory's owned lock and SQLite's owned SELECT
provide that namespace snapshot; this does not serialize unrelated namespaces
or certify a later transaction. The reader never writes, migrates, opens missing
state as empty, advances a fence, or selects a public runtime. Public legacy
`MqService::open` keeps its existing explicit historical migration behavior and
rejects a v2 marker.

The v1 path shares the existing object-envelope decoder, catalog codec, legacy
types, replay validator and state invariants. Its schema adapters reject unknown,
missing and duplicate fields, including nested message/pending schemas and
duplicate or noncanonical legacy handle keys. Normalized manifests must have
explicit null `definitions`; historical flat-state conversion remains solely
with the old loader. A manifest-only uninstalled normalized state is accepted
only when the original state validator permits it. Installed state requires the
exact catalog/queue/trigger cross-references. Any rich rows beside v1 fail closed.

The v2 path strictly parses the import marker's exact fields: `schema_version`,
`target_row_prefix`, `identity`, `source_manifest_version`,
`source_catalog_version` and `legacy_next_handle`. Positive SQL-compatible source
versions are historical provenance: each current dependency must be greater
than its source version, rather than equal to its initial import successor.
Generation and recovery fence are trusted service inputs. The marker and rich
metadata must both match the identity computed from those inputs and the captured
catalog's canonical encoded bytes. Rich delivery restores through the existing row and live
checkpoint validators from that same snapshot. The trusted profile also supplies
default persistence; its default is the import's persistent policy. Ordinary
generation/fence advancement requires an explicit manager-owned atomic CAS of
the marker and rich metadata. Reads never repair disagreement.

Leftover legacy queue, handle or pending rows, missing dependencies, mixed or
unknown namespaces, orphan rich members, unsupported schemas, malformed keys,
versions, catalogs or payloads fail closed. Retained catalog, marker and replay
physical records and versions remain available as exact dependencies. Legacy
replay values are decoded with the existing recorded-result/retention validator,
without rewriting bytes, digest domains or optional owner metadata. Numeric
legacy replay handles remain historical values; they do not issue fresh opaque
handles. This MQ-prefix read does not independently prove referenced core
effect/retention records; their transaction composition remains required.

Before typed decoding, checked aggregate byte accounting includes every captured
record, separately from row counts and per-row limits. Default physical ceilings
are 114,467 records, 64 MiB per row and 128 MiB aggregate, within the store's
bounded scan contract. The aggregate permits the import's retained legacy
corpus alongside its rich row footprint, including the replacement marker's
overhead. V1 full state, v2 retained replay and rich delivery each keep their
respective 64 MiB ceilings; catalog guards and the combined physical budget
also apply to retained catalog/marker records. Smaller explicit profiles may
narrow these limits. Live resume and persistent-only cold restart/backout policies are
unchanged. Public runtime/ABI selection, non-quiescent migration, lifecycle owner
minting, typed replay, SAF, audit/coordinator composition and participant
acceptance remain integration requirements.

## Private MQ composed publication and persisted fence

`MQ-1505.rich-state-publication-fence` plans from the same captured rich
authority. Ordinary delivery candidates publish at its exact catalog,
generation and fence. A separate explicit fence-only plan derives the next
fence as checked old-plus-one in the positive SQL-compatible domain. Neither
API accepts independently supplied old marker, catalog, versions or owners.
Catalog/generation replacement is refused and requires distinct migration.

Every logical plan includes the existing delivery metadata CAS, exact old v2
marker CAS and retained catalog dependency CAS. Ordinary marker/catalog payloads
keep their captured bytes while physical versions advance by one. A fence-only
plan changes marker and metadata identity together; all delivery member payloads
and versions, pending/final decisions, cursors and live state remain unchanged.
The metadata helper never runs an implicit cold-restart/backout policy or
reprojects per-message rows. Import provenance versions remain historical lower
bounds, permitting these physical dependency advances.

Planning does not access or mutate the store. It reuses the strict reader on
immutable captured row projections and the existing delivery delta/checkpoint
validation, including the retained reader profile and combined snapshot budget.
It returns one composable mutation batch and one validated next authority for
adoption only after the full transaction succeeds. Retained replay payloads,
versions, owner metadata and core references are unchanged; numeric legacy
handles do not become fresh opaque handles. Stale plans fail explicit CAS with
no partial publication, never automatic mutation replay or premature adoption.

The private publication profile permits at most 1,024 mutations, 64 MiB per
put and 64 MiB aggregate put bytes, with smaller profiles allowed. Marker,
catalog and metadata dependencies count in these limits. This is smaller than
the shared audited ceiling of 4,096 mutations; the service must also bound its
FULL composed batch including replay/UOW additions. Oversized logical batches
fail before publication, with no silent batching or cap increase. Reader/import
combined 128 MiB capacity and v1 semantics are unchanged.

Memory and SQLite regression fixtures compose these plans through the existing
`AuditedProviderPublication` under real retained, attributed canonical core
intents and typed audits. Both race orders, dependency CAS failure, trailing
composed failure and audit saturation roll back rows and audit together. This
proves the private store composition, not an admitted public MQ operation.
The persisted fence is not a coordinator permit, UOW decision or owner lease.
Fresh plans under one still-live intent are not intent-wide deduplication:
actual original-effect/replay/UOW binding, lifecycle retirement, SAF, service
selection, core completion and participant/licensed acceptance remain required.

## Private MQI typed result storage

`MQ-1505.typed-result-replay-codec` supplies only pure private storage conversion
under `mainframe-env.mq-mqi-result-storage@1`. It does not select a namespace,
write a receipt, prune history, authorize dispatch or calculate a status pair.
The codec takes explicit finite HostLimits, MqMqiLimits and storage-byte bounds;
profiles may narrow existing ceilings, never widen them. MQI limits remain part
of the original full host result identity and must match the service's original
result profile. The stored digest is recomputed over
`Ok(HostResult::MqMqi(MqMqiHostResult { result, limits }))` in the existing shared
canonical result domain, including the wrapper and limits. A standalone MQI
digest cannot replace it.

Completed and StatusPending keep their exact distinct outcomes and representable
Put, Got, Distribution, Property, Buffer, Attributes, UnitOfWork,
PublicationsRequested and NoOutput values. ReviewedStatus reconstructs only an
already admitted symbolic pair through the existing call-specific table; pending
or mismatched identities fail closed. Pending, UnknownOutcome and
DuplicatePossible never become completed success. CallbackReturned remains a
private notification value validated by the frozen standalone result validator;
the unchanged public host validator still rejects callback notifications as
application MQI effects. Canonical framing alone grants no public admissibility.

The additive ReviewedOutput storage kind retains an admitted call-specific
completion/reason pair together with its exact typed output. It uses the same
strict output projection, full host digest and original finite limits, including
SQL-compatible UOW/cursor bounds. Reviewed GET truncation buffers, descriptors,
properties and required/copied lengths do not become status-only records. The
existing result validator rejects mismatched status/output even with a coherent
digest; provider admission must additionally validate the original request.
Issued handle outputs decode only to historical observations, and symbolic
Default/Unassociated reconstruction is still refused. Every older storage kind
and byte representation stays unchanged; older readers reject this new kind.

The private ordinary z/OS selected complete GET profile admits only empty
structured properties and stored BackoutCount values from zero through 255.
The input GET descriptor counter is ignored/output-only. Exact full storage and
replay continue to preserve arbitrary signed observations and ordered properties;
their representability does not authorize native GMO/RFH2/property behavior.
An original selected local BACK uses the explicit live z/OS kernel candidate to
increment removed syncpoint complete messages once, capped at 255, under current
logical ownership, mandatory SAF and the existing atomic audited publication.
CMIT, partial payloads and storage-only/cold backout policy remain unchanged.
Only known physical success adopts; retained Unknown decisions never authorize
redispatch or recount. This is not HardenGetBackout, task-end or crash accuracy.

A delivery-owned facade shares the original strict message/property projection.
It records persistence, expiry and priority explicitly and restores them exactly,
including nonpersistent and abstract pending/default values. Its neutral inner
projection cannot override those fields. Descriptor-only values require empty
body/properties rather than discarding hidden payload. Message identifiers,
format, group/segment/order metadata, typed property bytes/order, truncated copied
and required lengths, buffers and attribute arrays are preserved. Cold/live
checkpoint schemas, bytes, historical defaults and recovery policies are unchanged.

Readers deny unknown, missing and duplicate fields, including explicitly nullable
fields, unsupported schemas/tags, trailing data, malformed call/output/status
combinations, non-SQL local UOW/cursor identities and digest disagreement.
Before typed allocation a bounded streaming structural pass checks collection,
string, nesting, field and aggregate input limits without a Value tree. Encoding
uses a bounded writer. The original frozen validators still own public shape
and resource legality; this storage schema is not IBM wire or a second public
request encoding.

The complete producer output selects `mainframe-env.mq-mqi-result-storage@5`
iff its stored output class is `Produced`. Version4 is reserved for the separate
RFH2 owner and is rejected here. Versions1/2/3 retain their exact bytes and output
classification; readers reject a producer relabeled as any older class, or an
older output relabeled as5. The sole strict codec includes full MD bytes through
the owned value encoder, exact fixed48 resolved names, delivery disposition,
explicit undefined z/OS destination counters and preserved ignored-input counter
disposition under the FULL original host result digest. Required fields, width,
schema/output disagreement, duplicates, unknowns, malformed tags, quotas and
digest changes fail closed. Receipt namespace/provenance/retention are unchanged.

The same catalog's native attribute version2 is an explicit additive deployment
profile; version1 stays byte-exact and contains no guessed defaults. In this
feature native metadata is installed through the existing explicit catalog
installer before legacy rich import and the globally quiescent empty-queue
complete-profile upgrade. It does not supply operator or core permission, replace
an active catalog, rewrite populated partial queues, or migrate on startup.
Deployed replacement still requires a separately admitted drain/backup/migration
protocol covering all live owners, units, cursors and receipts. Older catalog
readers reject2; downgrade requires stopping writers and restoring a verified
pre-upgrade backup or retaining a compatible reader, never stripping attributes
or relabeling schema. Rich marker, namespaces, catalog hash, row envelopes and
ordinary exact catalog/meta/marker/control dependencies retain their owners.

The historical-handle extension preserves issued Connected, Opened with exact
nullable dynamic metadata, MessageHandle and both Subscribed roles in the SAME
codec through host-owned `MqHandleObservation`. Non-handle bytes and schema stay
unchanged. Only the bounded strict observation supports Serde, not executable
tokens. Successful decode returns the actual typed output with an irreversible
historical disposition: exact original registry/slot/generation/epoch and role
remain canonical, but every registry access denies permission even for coincident
live IDs. Symbolic Default/Unassociated connection reconstruction remains
explicitly unsupported and cannot acquire the current CICS task's connection.

Readonly resolution can return only an already-existing exact-owner/role/epoch
entry on an independently admitted live connection. It allocates/resurrects
nothing. Service must first prove the exact retained receipt/core occurrence,
original actor/run, request/result and current frame under the same locked
registry; observation or digest alone attests none of them. Historical result
identity is retained even if a service/ABI separately resolves an existing live
alias. Restart exposure still requires service-owned persisted epoch advancement;
numeric legacy handles, process counters and aliases never establish authority.
See [ADR 0033](../decisions/0033-mq-historical-handle-observation.md).

The manager still must compose typed receipt rows, exact original effect/UOW/core
references, CAS and existing retention proofs. The strict stored-authority reader
accepts no new namespace here. Public service/host dispatch, SAF, durable UOWs,
replay publication/retention, participant and CardDemo remain required. Only the
licensed IBM MQ differential oracle is human-skipped, with zero licensed credit.
