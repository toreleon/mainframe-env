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
The process key hex-encodes UTF-8 name bytes, while the pinned IBM PROCESS and
ACTIVITY limits count source characters and permit `¬`. Consumers must decode
and validate the name by characters; a fixed 72-hex-character process-name
bound would reject a valid 36-character name containing `¬`.

The authority exposes `acquired_process_container_scope(run_unit,
owner_execution, owner_principal)` as a read projection for sibling BTS
container commands. It verifies the held acquisition, exact owner, process
row, activity and index, and pending-UOW visibility. A root acquisition,
including DEFINE PROCESS, yields read/write process-container access; an
acquired descendant yields read-only process-container access. The latter
does not satisfy GET CONTAINER (BTS) `ACQPROCESS`, which specifically requires
an acquired root in the current UOW (`INVREQ 16/15` otherwise). The API
provides no descendant command selector while that selector remains
unresolved. The caller still owns SAF/audit and must resolve the scope in its
command's current UOW; syncpoint release leaves only the acquisition epoch
tombstone. A deferred NOCHECK duplicate has no process tree to borrow.

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
unknown outcome for exact owner reconciliation. Startup recovery leaves that
outbox entry pending and continues admitting independent RUN work.

Terminal parent completion deletes settled descendants, their activity
indexes, and direct-child completion events atomically with the parent RUN
record. A live, acquired, or pending descendant leaves the transition
unresolved; the parent remains active until the descendant is reconciled.
This conservative path does not yet establish every automatic deletion case
in the pinned DELETE ACTIVITY contract. Source: baseline A catalog row `0041`,
`dfhp4_deleteactivity.html` SHA-256
`3d91328683fc209e5ee8583f96f8c77de09c99caf6ca1994b2e19e81a00e7a24`.

RUN TRANSID issues a child token through the sibling `CicsService` registration
and completion signatures. This checkout carries a compatible child-token
port while the sibling FETCH/FREE lane is separate. The token row is an
ownership and reply record only; it does not duplicate process or activity
lifecycle state. Reconciliation must retain the single `cics-bts-process-v1`
authority and the sibling's token row. The sibling's current channel-name
validator accepts fewer characters than pinned RUN TRANSID; its validator
must be widened when the modules are joined.

RUN TRANSID also owns a separate `cics-bts-transid-run-v1` request and
`cics-bts-transid-outbox-v1` pending index. The request captures a deterministic
16-byte child token, inherited principal, local transaction and program, and
the bounded channel-container snapshot at issue time. A worker claim advances
its lease epoch; a stale claim cannot finish it. Terminal completion writes
the sibling token outcome first, then closes the request and outbox atomically,
so a crash in between can reconcile the retained child outcome after restart.
The outbox reader checks that token outcome before readmitting pending work.
A terminal work item without a token outcome remains an unknown outcome;
startup recovery leaves it pending while processing other child work, and
exact per-request reconciliation reports `UnknownOutcome`. Recovery never
invents a normal completion or abend code. A request retained
before token registration may be recovered only when the parent run remains
available, otherwise it stays unresolved rather than asserting task start.
These are child task records, not process/activity lifecycle rows.

The selected server route executes the installed local transaction in a new
run unit under the inherited principal. `launch_background_task` repeats
transaction SAF authorization at attach and records security failure in the
child token. The issue-time channel identity and bounded container snapshot
are passed as invocation bindings. The in-flight parent is temporarily outside
the service's live-run map during command dispatch, so token registration uses
the authenticated `Run` directly while writing the sibling-compatible row;
the public `register_bts_child` signature retains its live-parent guard.
The isolated snapshot reader currently uses the older global transform-container
map; integration must read the container lane's run-unit-scoped channel rows
before the `CHANNEL` option is sealed. Neither channel row is a process or
activity lifecycle authority. The container authority should supply one
in-flight snapshot operation using the authenticated parent `Run` and channel
name: validate task ownership and scope, create a missing empty channel under
its capacity CAS, and copy the bounded containers at issue time. RUN TRANSID
then retains that copy in its existing request row without a fallback to the
global transform map.
The sibling now exposes `snapshot_channel_for_bts_child(&mut Run, channel)`
for that handoff. The additive v1 RUN TRANSID payload can retain each copied
container's character mode, CCSID, read-only flag, and bytes; older payloads
without the two metadata fields remain readable. New byte payloads use a
bounded base64 JSON string to fit the request row and invocation payload; the
v1 reader also accepts the earlier byte-array form. The child invocation
carries the retained map.

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
