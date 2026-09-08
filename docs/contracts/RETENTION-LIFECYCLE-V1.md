# Durable retention lifecycle, version 1

Status: **Implemented**
Owner: **store-contract, provider, and core-server maintainers**
Scope: **all durable source families, provider replay and recovery graphs, age observations, retained archives, and offline maintenance**
Applies from: **mainframe-env 0.8.3 development**

The `mainframe-env.retention@1` contract prevents a healthy bounded store from
becoming permanently full without weakening replay, checkpoint, effect, audit,
or provider recovery. Retention is an explicit operator operation. Normal
request handling does not silently prune history, and the server does not start
a retention scheduler or expose an unauthenticated retention route.

This contract covers the sixteen closed `RetentionTarget` families, their
dependency-safe order, conservative logical ages, archive-before-delete
transactions, legacy age observations, capacity forecasts, and the equivalent
Memory, SQLite, and PostgreSQL behavior. Provider payload meaning remains owned
by the provider which writes that payload; the generic store rechecks bounded
dependency proofs but does not substitute a partial provider decoder.

## Durable time and policy boundaries

All ages are positive ticks from the store's durable monotonic logical clock,
the same domain used by invocation deadlines and effect recovery. Wall-clock,
Unix, process-start, file-mtime, and cache-observation time are not retention
authorities. `advance_logical_clock` may return the current value when time has
not advanced, but it never returns below a supplied or previously persisted
floor. Advancing the clock consumes neither provider-state capacity nor the
provider mutation epoch.

`RetentionPolicy` has four non-zero lifetimes. At non-zero `now_tick`, it derives
four inclusive low watermarks using saturating subtraction:

| Watermark | Definition | Authority protected by the lifetime |
|---|---|---|
| `lifecycle_tick` | `now_tick - lifecycle_ticks` | terminal executions/work, lifecycle events, delivered outbox, console responses |
| `idempotency_tick` | `now_tick - idempotency_ticks` | resolved effects and replay/UOW receipts |
| `audit_tick` | `now_tick - audit_ticks` | core audit and security evidence |
| `archive_tick` | `now_tick - archive_ticks` | immutable archive batches before permanent deletion |

Saturating subtraction is not evidence that a lifetime elapsed. A target's age
window is closed until `now_tick >= lifetime`; a source row is eligible only
when its non-zero age is at or below the corresponding watermark. The two mixed
families use the stricter lifetime: RACF evidence uses
`max(audit_ticks, idempotency_ticks)` and the minimum of those watermarks;
installed COBOL and physically purged spool jobs use
`max(lifecycle_ticks, idempotency_ticks)` and the minimum of those watermarks.

A durable terminal or resolution tick is sampled at or after the terminal state
or provider result has persisted. An invocation/effect deadline is only a lower
bound: the safe age is at least `max(deadline_tick, post_persist_tick)`. A retry
may finish an unaged pending receipt once, but an ordinary hit on a fully
current receipt must not refresh that tick. Retention therefore has a fixed
idempotency lifetime rather than an attacker-extendable sliding lifetime.

## Frozen targets and dependency order

`RetentionTarget::ALL` contains every family exactly once in this order. The
stable names are also the operator JSON and archive-manifest names.

| Order | Stable target | Watermark | Intrinsic or conservatively observed age authority | Principal protection |
|---:|---|---|---|---|
| 1 | `db2-replay` | idempotency | full replay envelope's post-persist resolution, no earlier than its deadline | exact completed core effect, or exact CICS nested provenance |
| 2 | `ims-replay` | idempotency | full replay envelope's post-persist resolution, no earlier than its deadline | exact completed core effect, or exact CICS nested provenance |
| 3 | `mq-replay` | idempotency | full replay envelope's post-persist resolution, no earlier than its deadline | exact completed core effect, or exact CICS nested provenance |
| 4 | `dataset-replay` | idempotency | full dataset replay's post-persist resolution, no earlier than its deadline | exact completed core effect, or exact CICS nested provenance |
| 5 | `cics-unit-of-work` | idempotency | finalized UOW observation, no earlier than the outer effect deadline | exact terminal UOW, owning execution/run, and no undo recovery |
| 6 | `cics-replay` | idempotency | outer replay's post-persist resolution, no earlier than its deadline | same-key completed `host.cics.execute` effect and exact canonical digests |
| 7 | `racf-evidence` | stricter audit/idempotency | terminal transaction/recovery tick; audit retention-observation or event tick | full RACF aggregate codec; every recovery dependency precedes its transaction |
| 8 | `cobol-lifecycle` | stricter lifecycle/idempotency | call completion, protocol/run end, or cancellation tick; live instance rows have no retention age | owner and child executions, run unit, protocol, run state, and an empty dynamic instance namespace at run end |
| 9 | `spool-jobs` | stricter lifecycle/idempotency | trusted post-purge `purged_tick`, at or after `purge_boundary_tick`, or an exact legacy sidecar | physical artifacts absent; purge intent/replay recovery complete |
| 10 | `console-log` | lifecycle | current console row's durable `observed_tick`, or an exact legacy sidecar | explicit direct-product provenance, or its execution/run dependency |
| 11 | `resolved-effects` | idempotency | `EffectRecord.resolved_tick`, or an exact legacy sidecar | only Completed/Failed; no provider replay, unknown result, checkpoint, or live owner dependency |
| 12 | `delivered-outbox` | lifecycle | `OutboxRecord.delivered_tick`, or an exact legacy sidecar | delivered only; owner is terminal and checkpoint-free |
| 13 | `audit` | audit | typed `AuditRecord.observed_tick`, or an exact legacy sidecar | owner/effect recovery and checkpoint dependencies are clear |
| 14 | `terminal-work` | lifecycle | `WorkRecord.terminal_tick`, or an exact legacy sidecar | terminal only; no live work, unresolved effect, replay, or checkpoint dependency |
| 15 | `lifecycle-events` | lifecycle | typed event tick, or an exact legacy sidecar | event owner is terminal and all execution recovery dependencies are gone |
| 16 | `terminal-executions` | lifecycle | `ExecutionRecord.terminal_tick`, or an exact legacy sidecar | every event, work, effect, outbox, audit, checkpoint, and provider owner/child dependency is gone |

The order is provider-first and owner-last. Child enterprise receipts precede
their CICS UOW, the UOW precedes the outer CICS replay, all provider and effect
receipts precede their execution-owned journals, and the execution identity is
last. One pass can expose a row which was blocked at the start of that pass, so
operators repeat bounded passes until forecasts show no eligible remainder.
Changing this order, omitting a target, or inventing an unregistered target is a
contract change.

## Provider full codecs and dependency proofs

The core server asks the owning provider to fully decode every provider source
row before constructing a plan. A descriptor binds the exact namespace, key,
CAS/source version, SHA-256 of the exact payload, supported codec version,
terminal state, owner execution and run unit where applicable, non-zero age,
and canonical request/result digests. Full decoding enforces configured field,
record, and payload limits; canonical serialization; exact key/payload binding;
and rejection of unknown versions, duplicate fields or output names, trailing
bytes, partial current metadata, and forged owners or digests. A malformed row
fails the target scan closed. It is not relabeled as legacy.

The provider families have these codec-owned boundaries:

- Db2, IMS, and MQ use their complete versioned object-row decoders. Current
  rows distinguish `CoreEffect` from an explicitly attested `CicsNested`
  origin and bind owner execution, run unit, sequence, deadline/resolution,
  request digest, result digest, and the whole metadata binding digest.
- Dataset uses the full `MEDR3` replay codec with the same explicit owner kind,
  exact nested outer-effect key, and request/result/binding digests. `MEDR1`
  and transitional `MEDR2` rows are protected unless an owning validator can
  attest them without redispatch.
- CICS outer replay uses `MECER003`; CICS UOW uses exactly the supported
  pending/terminal `MECU2` versions; undo rows are fully decoded. Legacy
  `MECER001`/`MECER002` and `MECU1`, an unobserved terminal row, a pending UOW,
  or any live undo authority stay protected.
- Installed COBOL validates `cobol-call-replay@1`,
  `cobol-call-protocol@2`, `cobol-run-state@1`, `cobol-cancel@1`, and every
  bounded `cobol-instance@1:` namespace. Its descriptor enumerates both owner
  and child `Execution` dependencies plus exact run/protocol/state rows. An
  instance is always active retention state: `finish_run_unit` must atomically
  remove every instance before publishing the terminal run/protocol state, and
  a leftover instance therefore protects the lifecycle rather than inheriting
  a guessed age.
- Spool fully validates the canonical `mainframe-env.spool-state@2` job,
  replay versions, and artifact-empty purge state. Live, purge-pending,
  recovery-capable, or legacy rows without an exact no-owner sidecar are never
  retention candidates.
- Console fully validates `mainframe-env.console-log@2`, including exact row
  identity, payload digest, provenance kind, and observed tick. A direct
  product route is explicitly `DirectProduct`; absence of an `ExecutionRecord`
  for that route is not corruption and is not generalized to other targets.
- RACF fully decodes its bounded `racf-database-v2` aggregate and emits exact
  standalone descriptors for `racf-audit`, `racf-transaction`, and
  `racf-recovery`. Archival CAS-replaces the aggregate while archiving those
  exact extracted bytes; recovery rows must be terminal and old before the
  transaction which references them can move.

Every provider-built `ProviderRetentionRow` carries one generic dependency for
the store to recheck atomically:

- `CoreEffect` requires a same-key canonical Completed effect whose execution,
  run unit, sequence, request digest, and result digest match the descriptor.
- `CicsNested` is available only to a child written through the internal CICS
  dispatch path. CICS rejects the reserved
  `cics.nested-effect-origin` and `cics.outer-effect-origin` bindings at every
  external entry, then injects both internally. The first payload is exactly
  `cics:{run_unit}:{sequence}`; the second is the exact outer CICS effect key.
  A key prefix alone grants no exemption. Planning requires the exact finalized
  UOW/provenance row, its owning execution/run and nested sequence, the matching
  canonical Completed outer `host.cics.execute` effect, and absence fences for
  same-key core-effect and UOW undo recovery. Its safe age is the
  maximum of child resolution, UOW finalization, and outer-effect resolution.
- `ProviderGraph` supplies bounded exact provider rows and additional terminal
  execution owners which must outlive an owned row, including the complete
  installed-COBOL parent/child lifecycle graph.
- `DirectProduct` is limited to the explicitly encoded console route.
- `None` is limited to provider-local terminal evidence, such as a physically
  purged spool row or RACF evidence whose internal graph was fully validated.

The plan is fenced by the provider-state mutation epoch before and after full
decoding. The store then rechecks exact source bytes and CAS versions, owners,
effects, required rows, and required absences in the archive transaction. A
single bounded safety index prefilters non-terminal owners, run mismatches,
checkpoints, and intent/unknown effects before age ordering, so an unsafe oldest
row cannot repeatedly starve a later safe row while the atomic store validator
remains the final authority. A
live provider refreshes its durable replay/job index before ordinary dispatch,
so deletion by another process cannot be served from a stale cache or continue
to consume provider-local capacity.

Before forecasting or pruning a dependency-sensitive core family, the shared
store-only retention planner builds a bounded `CoreRetentionDependencySnapshot`
from every full provider
descriptor, including dynamic COBOL namespaces. It lists every blocked owner or
child execution and every same-key or outer CICS effect. Corrupt, legacy, or
otherwise unattributed provider evidence sets the conservative `unowned` fence.
The store accepts that inventory only at its exact provider-state epoch and
rechecks it inside the core archive transaction; it never tries to reconstruct
provider ownership from prefixes or partial payload fields.

## Legacy age observations

Absence of a trustworthy intrinsic tick protects a structurally valid legacy
row; it never makes the row old. Partial new metadata is corruption, not a
legacy encoding. The explicit reconciliation path records a
`RetentionObservation` in the dedicated observation authority without
rewriting the live payload or fabricating provenance.

An observation binds target, logical namespace and key, exact source version,
SHA-256 of the exact logical source payload, a positive durable observed tick,
and the verified owner execution when that family requires one. Observation
insertion is source-CAS- and epoch-fenced. A stale source version/digest, wrong
owner, unsupported family, corrupt full decode, or missing recovery dependency
rejects reconciliation. Legacy direct-console rows can be aged by an exact
sidecar without inventing an execution owner. Ordinary replay does not refresh
an existing age.

Observations have independent row and byte accounting and appear in forecasts.
Archiving a source removes its matching observation atomically. Provider-owned
cleanup can remove a stale or orphaned observation only after an exact
present-source comparison or an exact absent-source assertion under the same
epoch/CAS fence. A corrupt or mismatched observation protects the source and
fails the scan; it is never accepted as approximate evidence.

## Archive-before-prune transaction

An eligible move creates one immutable `RetentionArchive` before removing live
state. It contains the target, archive tick, source watermark, and every exact
source namespace, key, version, payload, retention tick, and owner required for
evidence. `archive_id` is a domain-separated SHA-256 commitment over the batch
metadata and ordered source content. Loading or deleting an archive recomputes
its target domains, row count, accounted bytes, and identifier. Corrupt,
truncated, reordered, cross-target, or oversized stored archives fail closed.

Archives and observations do not occupy the live `provider_state` authority.
Memory uses dedicated bounded maps; SQLite and PostgreSQL use dedicated
`retention_archive`, `retention_archive_row`, and `retention_observation`
tables. Row counts and conservative accounted bytes are enforced independently
for live sources, archives, and observations. Consequently an eligible full
live provider-state table can still shrink, provided the dedicated archive
authority has row and byte headroom.

For independent rows, archive insertion, matching observation removal, and all
source CAS deletes commit atomically. For an aggregate such as RACF, archive
insertion, observation removal, and the exact next aggregate generation commit
atomically. An epoch change, CAS conflict, missing dependency, quota failure,
payload error, or process/database failure commits neither archive nor source
removal. An archive is evidence, not a replay cache: after the documented
idempotency lifetime and source pruning, reuse of the old key is a new
operation.

### Batch and historical-archive rules

- A new source transaction moves no more than the request bound, configured
  `max_batch`, or hard `MAX_RETENTION_BATCH` (4,096), whichever is smallest.
- When a multi-row archive does not fit the dedicated row/byte authority, the
  implementation may halve the selected batch and retry. If one exact source
  row still cannot fit, the operation fails and leaves it live; rows are never
  truncated to make an archive fit.
- Archive listing and permanent pruning count source rows cumulatively across
  whole archive batches, not archive manifests. They never split, rewrite, or
  partially delete an archive.
- A historical archive may have been created under a larger former
  `max_batch`. Listing may return that oldest indivisible archive alone so the
  operator can inspect its exact content-addressed identity. Permanent deletion
  does not infer an override: it returns `AuthorizationRequired` with that
  archive ID, source-row count, and requested bound, without deleting anything.
  A retry must supply exactly that ID through
  `--authorize-oversized-archive`; a wrong, stale, or unnecessary ID conflicts.
  The authorized operation deletes only the reviewed oldest archive and reports
  that the authorization was consumed. Every historical archive was still
  created within the hard 4,096-row contract maximum.
- Permanent deletion requires a non-zero current tick, the archive window to
  have opened, and `archive.archived_tick <= archive_tick`. All source bytes
  remain in the verified archive until that later operation succeeds.

## Forecasting and capacity

`retention_capacity_health` proves provider-state insert, update, delete, quota,
and epoch-trigger authority inside one rolled-back transaction, then reports
constant-cost source capacity for all sixteen targets in frozen order plus the
shared archive and observation row and byte authorities. The collision-safe
probe leaves no row, consumes no provider-state quota, and does not advance the
provider mutation epoch. `retention_forecast` additionally reports active, eligible,
and protected records; source headroom; target and shared archive/observation
usage and headroom; all four watermarks; saturation; and bounded projections
from an operator-supplied row-growth rate. Zero growth yields no time estimate;
zero headroom yields zero ticks. Saturation is the worst of source, archive-row,
archive-byte, observation-row, and observation-byte pressure.

SQLite and PostgreSQL source targets share the bounded provider-state table, so
their effective headroom includes unrelated provider namespaces. Provider
forecasts also apply the provider-local family limit. Archive and observation
usage is both target-attributed and globally accounted. An explicit forecast
may monotonically advance only the dedicated durable logical clock; it never
changes a source, archive, observation, provider epoch, cache, or recovery row,
and never claims that protected recovery rows are reclaimable capacity.

## Offline operator protocol and receipts

The library exposes the typed controls on all three store implementations. The
standalone `mainframe-env-server ... retention` CLI accepts only SQLite and
PostgreSQL durable databases; Memory and every in-memory SQLite URL form are
rejected. It opens and migrates only the state store, then constructs the
shared store-only planner. Before an explicit action it does not open artifact
storage, package trust, Product, providers, authentication/session indexes,
application publication, outbox recovery, caches, listeners, or workers. It
emits one compact JSON document and provides:

```text
mainframe-env-server CONFIG retention forecast --observed-growth-per-tick 10
mainframe-env-server CONFIG retention maintain --max-records 1024
mainframe-env-server CONFIG retention maintain --max-records 1024 \
  --authorize-oversized-archive sha256:REVIEWED_ARCHIVE_ID
mainframe-env-server CONFIG retention legacy --max-records 1024
mainframe-env-server CONFIG retention reconcile \
  --target db2-replay --namespace db2-v1-replay --key KEY \
  --expected-version VERSION --owner-execution EXECUTION
```

For guaranteed progress, operators must drain every server, worker, and tool
which can mutate the store; for PostgreSQL this means every node sharing the
database. Take and verify a backup first. Bounded jittered conflict retries are
collision tolerance, not an online progress guarantee. Shortening any lifetime
also requires draining old binaries and active clients, proving the new
watermark is beyond every supported retry/checkpoint/effect/audit recovery
horizon, and preserving the pre-change backup. Storage pressure alone is not a
safe reason to shorten a lifetime.

`maintain` first prunes eligible whole archives, then invokes every target once
in `RetentionTarget::ALL` order. Its `max_records` is a per-operation bound; a
complete pass can therefore commit several separately bounded transactions.
Legacy listing is instead cumulatively bounded across targets. Each successful
source operation returns a `RetentionReceipt` with target, watermark, examined,
archived, pruned, protected, optional archive ID, `observations_created`,
`observations_reused`, and `stale_observations_removed`; `archived == pruned`
for a successful move, and a no-op has no archive ID. Observation creation or
cleanup is returned as an observation-only receipt and ends that target's
operation; archival waits for a later pass, so a subsequent archive failure
cannot hide a committed observation mutation. Source pruning plus created and
removed observations never exceeds that operation's `max_records`.

A maintenance pass is not globally atomic. A later target can fail after prior
archive/source transactions committed. Success documents use the `@2` command
schemas. An error after prior processing uses
`mainframe-env.retention-error@2`, status `partial`, the failing phase, the
expired-archive receipt, and every completed target receipt in contract order;
the process exits nonzero. `AuthorizationRequired` before mutation remains
status `error` and includes the exact retry flag. These reports are not proof
that nothing changed: already committed archives remain the durable receipts.
Inspect the failing target and archive inventory, repair the owning
recovery/corruption/capacity condition, then reforecast and repeat the full
ordered pass. Never manually delete a live source, observation, archive
manifest, or archive row. The clock-free `legacy` command performs no durable
clock write.

Legacy reconciliation requires the exact token returned by `retention legacy`
and an independently verified owner where required. The command does not infer
owners from keys. Pending outbox, intent/unknown effects, active work,
checkpoints, live provider recovery, corrupt rows, and unsupported legacy
families are repaired through their owning workflow rather than by retention.
Legacy-only RACF state is a target-local migration-pending condition: the offline
planner fully decodes and protects those rows while other families remain
maintainable. Normal startup performs one final-state-quota-checked atomic
v1-to-v2 replacement and embeds the exact bounded v1 source rows in the migration
record as rollback/downgrade evidence.

See the [capacity and recovery runbook](../runbooks/CAPACITY-AND-RECOVERY.md),
[operations procedure](../runbooks/OPERATIONS.md), and
[provider-row persistence contract](PROVIDER-ROW-PERSISTENCE-V1.md) for the
surrounding deployment and storage rules.

## Backend parity and required verification

Memory, SQLite, and PostgreSQL implement the same target order, eligibility,
full archive identity, observation binding, whole-archive rule, and error
contract:

| Backend | Atomic authority | Restart/operational rule |
|---|---|---|
| Memory | clone-and-stage the complete in-process state, then publish once | API/reference backend; offline CLI rejects it because state cannot survive restart |
| SQLite | one database transaction over live, archive, observation, epoch, and logical-clock tables | migrations and reopen must preserve archive/observation accounting and free source capacity |
| PostgreSQL | one transaction with locked quota/retention authority and exact CAS predicates | all nodes use identical limits; concurrent plans have one winner and conflicts publish no partial move |

The shared contract suite must exercise all three backends, with PostgreSQL run
by the database parity gate. Required cases include every target and stable
order; inclusive watermark and pre-window tick-zero behavior; live/checkpoint/
unknown/pending protection; exact legacy observations; provider full-codec and
CICS nested provenance corruption; source/archive/observation row and byte
saturation; multi-row fallback and single-row rollback; lowered-policy
historical archives; stale epochs and concurrent winners; restart and capacity
reuse, including an empty COBOL instance prefix after run end and SQLite reopen;
partial operator passes; and archive corruption before permanent delete.
