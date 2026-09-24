# ADR-0015: Shared BTS lifecycle authority

Status: **Proposed for the incremental CIC-904.bts-lifecycle slice**
Owner: **CICS provider and execution maintainers**
Scope: **shared BTS process/activity state for 23 lifecycle application rows**
Applies from: **mainframe-env 0.9.0 development**

## Context

The 23 assigned BTS controls create, acquire, inspect, run, suspend, reset, and
remove process or activity state. Existing event-control rows persist event
pools, but have no process tree or lifecycle owner. The pinned CICS TS 6.x
ACQUIRE, DEFINE PROCESS, and DEFINE ACTIVITY topics make acquisition UOW scoped
and defer definition commitment to a successful syncpoint. Sibling BTS command
families need the same process and activity identity.

## Decision

`mainframe-env-cics::bts_lifecycle` owns one versioned CAS process row per
process-type/name pair. Each row contains the bounded root and descendant
activity tree, monotonically increasing process and activation epochs,
checkpoint references, and exact mutation replay. A separate versioned index
resolves each opaque 52-character activity ID to its process without an
unbounded scan. A per-run-unit acquisition row retains an epoch tombstone
across syncpoints so stale UOW owners cannot recreate an old lease identity.

DEFINE PROCESS writes the pending process, root index, and defining UOW
acquisition atomically. A successful syncpoint publishes the definition and
releases the acquisition atomically; rollback removes the pending process and
root index in the same transaction. The existing CICS UOW coordinator remains
the syncpoint owner. BTS supplies no universal prepare, automatic
compensation, or automatic redispatch after an unknown outcome. The local
participant declaration is `BTS_PARTICIPANT`.

DEFINE PROCESS and ACQUIRE retain their exact operation, effect key, request
digest, and selected process/activity identity in the acquisition row in the
same atomic transition. A retry with the same key and request returns the
saved result after reopen or UOW release; changed request bytes conflict.
An already-held acquisition with another key returns `INVREQ 16/22`.
Duplicate process names return `PROCESSERR 108/2`. The effect survives an
acquisition epoch tombstone, including rollback, until its replay window is
explicitly retired.

The `cics-bts-repository-name-v1` row reserves a name against the underlying
repository resource, so two process types sharing one repository cannot each
publish the same process name. Normal DEFINE reserves it in the same atomic
write as the pending process and acquisition; syncpoint publishes the
reservation or rollback deletes it with the process. Its row is a uniqueness
index. The `cics-bts-process-v1` row remains the only process/activity state
authority. The additive acquisition effect field identifies the repository
for settlement and old rows without that field remain readable.

NOCHECK records the pending process, root index, and acquisition without a
repository-name reservation. If the same process key already exists, it saves
only a deferred-duplicate acquisition with the new root identity; it does not
expose the existing process as the provisional one. Commit preflight checks
the repository name before the CICS UOW intent is recorded and returns
`PROCESSERR 108/2` for a known collision. Successful syncpoint publishes the
name with the process; rollback removes the pending process or deferred
acquisition. A collision arising after preflight is an uncertain UOW outcome
requiring reconciliation, rather than an asserted successful commit. Old
acquisition rows without NOCHECK fields retain their original behavior.

The CICS syncpoint path settles BTS child definitions and its one acquisition
before finalizing the UOW. Reconciliation reuses the durable UOW owner metadata
and BTS owner rows to finish an interrupted settlement. Each step is
idempotent; a multi-provider syncpoint can still return unknown outcome and
must be reconciled rather than redispatched automatically.

The opaque root ID binds the full process key to its defining UOW, so a later
incarnation does not reuse that activity identity. Child IDs bind the root ID
to a monotonic child sequence. A child definition and its index are one CAS
batch; a syncpoint publishes or removes pending child/index pairs with the
acquisition change. A reset or delete removes descendant indexes with the
process row. Activity completion and coordinator checkpoint references carry
activation and lease epochs. The first v1 writer omitted an activity-level
pending-UOW field; the reader derives it only for its pending root and rewrites
the additive shape at the next mutation.

An attached run binds to an active activity through one versioned
`cics-bts-activity-context-v1` row. The binding names the process, activity,
activation epoch, owner execution and principal, and coordinator lease epoch.
Readers verify the process row and exact checkpoint epoch on every use. A
recovered worker may advance the lease epoch for the same activation after the
process checkpoint has advanced; stale owners and changed identities fail
closed. Closing retains a tombstone until replay and checkpoint retention
allow safe pruning. The event pool remains a separate content row keyed from
this authority; it does not define process or activity lifecycle state.
Existing event commands resolve a registered run's BTS context first and use
the 52-character lifecycle activity ID as their event-pool key. Closed BTS
bindings block fallback to the older standalone event context. Input-event
delivery checks the indexed active activity before changing event state.
An activity completion event is an `Activity` event-pool record bound to its
exact child ID. DEFINE ACTIVITY creates it with the child and index in one
store mutation. Rollback and DELETE remove it, RESET clears its fired state,
and forced CANCEL fires it with the process transition. Composite memberships
and reattachment queues change in the same event-row mutation.

RUN uses a versioned `cics-bts-run-request-v1` record and bounded
`cics-bts-run-outbox-v1` pending index. Moving an activity to ACTIVE, firing
its named dormant input event, saving the exact request, and indexing work are
one atomic provider-state transaction. The CICS runtime admits work from the
outbox on reopen; the server claims `cics-bts-run-v1` work, advances the
checkpoint lease epoch, and starts the selected program in a separate run
unit. Completion updates the process, completion event, request, and outbox
in one transaction. A terminal work row with a still-pending RUN returns an
unknown outcome for explicit reconciliation.

The process row schema and namespace are version 1. Readers reject unknown
fields, invalid relationships, cycles, stale checkpoint epochs, malformed
identities, and excess state. There is no prior BTS process row to migrate.
Rollback to a build without this feature requires stopped admission and a
compatible backup; it must not silently ignore live process/checkpoint rows.
Retention must preserve pending definitions, acquisition tombstones, replay
records, and checkpoint references until their owning UOW and effect windows
close. The initial implementation does not prune those rows automatically.

## Consequences

Every BTS lifecycle and sibling handler uses this authority for process or
activity state. Existing event rows retain only their event-pool content and
will bind to the lifecycle identity when the public command routes land.
The store's atomic provider mutation works on Memory, SQLite, and PostgreSQL
through one contract. No command becomes executable merely by installing the
authority; typed registration follows selected-route tests and acceptance
gates. Licensed differential credit remains pending.

## Sources

IBM CICS TS 6.x baseline `ibm-cics-ts-6x-application-api-sources-a-2026-09-10`,
catalog rows `0002`/`0003` (`dfhp4_acquire.html`, SHA-256
`646a460e9ba3dd34548c5d73939a9a102538fda186d9e1e6b0721c20a12e927a`),
`0032` (`dfhp4_defineactivity.html`, SHA-256
`85c07bc78fc04f6a2496240292a0766033eff1b4f4f0c2ab5daff19402667260`),
and `0037` (`dfhp4_defineprocess.html`, SHA-256
`a31702315bb8d6cb0ac499858593eed0e2d4c2e3b65145474e95a39ecbd71f56`).
The manifest pins and external raw HTML matched and were parsed offline with
`ibm_docs.py`. The full assigned source map is in the 0.9.0 status.
