# Capacity and recovery

Size limits are explicit for HTTP bodies/concurrency, compiler input and IR,
dataset records/catalog bytes, RACF objects/audit, CICS sessions/screens/queues,
JES jobs/active work/spool/events, SQL rows/payloads, and immutable artifacts.
At 100% of any bound, admission fails before mutation. No queue or retry loop
grows automatically.

The core server owns exactly two shared durable workers. Each poll claims JES
work first and, when none is available, claims the CICS `cics-start-v1`
generation, then the CICS `cics-delay-v1` generation; generation-scoped claims
prevent any operation from consuming a foreign durable work lane. Within a
generation, higher priority is selected first; oldest available admission tick
plus work ID provides a deterministic FIFO tie-break within a priority. Each
worker heartbeats a 30-second lease every 5 seconds through the persisted
logical clock, and admitted work has a 24-hour deadline. After an ungraceful
process exit, wait until that lease expires;
the next server advances the same durable clock and reclaims with a higher
epoch. Never edit a lease ID, epoch, heartbeat, or clock record by hand.

After process failure, reopen SQLite at `0002-retention-lifecycle` or
PostgreSQL at `0003-executable-artifact-metadata`. Running JES work returns to
queued state until its attempt limit, expired work leases are reclaimable only with a higher durable fencing
epoch, and queued work at its deadline moves directly to dead letter. Suspended
CICS sessions stay suspended. Incomplete effects remain explicit intents with
their typed capability, dispatch owner, attempt, creation tick,
recovery-not-before tick, and logical event epoch. Run the bounded stale-effect
recovery worker with the same monotonic logical tick domain and only with a
service resolver that queries an authoritative provider idempotency ledger. The
store excludes fresh, pre-boundary, or actively leased intents, fences each
claim by recovery owner and epoch, and permits an expired claim to be taken only
at a higher recovery attempt/epoch.
The resolver may record proven completion or proven failure; an unavailable or
ambiguous observation stays pending until a later lease. Recovery never
redispatches the original mutation.

An online CICS exchange with an unresolved effect retains an
`online-exchange-v1` record keyed by terminal session. A resume request first
checks the referenced durable effect. If it remains `Intent` or
`UnknownOutcome` and no matching CICS replay-ledger result exists, return
`unknown_outcome` and do not dispatch the operation. If the ledger's canonical
request digest matches, reconcile the stored result, rebuild the abandoned
CICS run with the original execution and run-unit identity, and resume. Never
delete or edit the exchange, effect, or `cics-effect-replay-v1` rows by hand.
Replay envelope `MECER002` records expose the owner execution and effect
deadline needed by retention; `MECER001` rows lack that proof and must remain
retention-ineligible.

For a suspended `cics-enqueue` exchange, also leave `cics-enqueue-v1` and
`cics-enqueue-catalog-v1` untouched. The resource row is the durable FIFO and
ownership authority; its catalog count is checked when CICS opens. Resume the
same online exchange after the resource owner dequeues. Timeout, cancellation,
session disconnect, or abandoned-task recovery invokes bounded task cleanup,
which removes the waiter or releases its owned locks and promotes the oldest
surviving waiter. Manual deletion can orphan a grant or break the catalog count
and will make the next provider open fail closed.

Treat `cics-enqueue-model-v1` and `cics-enqueue-model-catalog-v1` as one
configuration unit. Install the complete bounded model set before the first
lock exists; later calls must be exact idempotent replays. Back up and restore
the definition rows and singleton digest row together. Missing, extra,
overlapping, noncanonical, or digest-mismatched definitions make CICS open fail
closed. Do not delete the model catalog to force local routing: model-aware lock
keys contain either APPLID/SYSID or ENQSCOPE and are not compatible with the
single-region key profile.

Back up `cics-interval-start-v1` with the CICS provider state. Do not delete or
edit pending, protected-pending, ready, consumed, or cancelled rows manually. A row binds
its REQID to the producing effect and canonical request; replacing it can turn
a duplicate START into a false replay. Optional RTRANSID, RTERMID, and QUEUE
metadata live in that same row and must not be split into a separate restore or
manually synthesized; RETRIEVE uses their presence to decide ENVDEFERR before
consumption. The row's FMH bit is equally authoritative because RETRIEVE
derives EIBFMH from it. A consumed row retains the exact
consumer identity needed to close the result-journal crash gap and is not yet
eligible for generic retention. Schema rollback therefore requires stopping
admission and restoring a pre-change backup; older binaries must not write a
store containing these rows.

RETRIEVE SET capacity is fenced before that consumed transition. Preserve the
canonical `SET.MAXLENGTH` request metadata in effect journals, and preserve the
machine checkpoint's allocated base, pointer bytes, and linkage-address map as
one unit. Removing only the allocated base can turn a valid virtual pointer
into corrupt task state; reducing task storage limits can make a retained
pre-response request fail closed instead of consuming its record.

Also retain the matching shared work row whose ID is `cics-start:<REQID>` and
generation is `cics-start-v1`. A queued row makes the pending interval record
eligible at its expiration tick; a claimed row is fenced by its lease ID and
epoch; a completed row records that promotion was attempted. On restart, let
the shared workers reclaim an expired lease normally. Never promote the
provider row by hand or enqueue a replacement with a different execution,
deadline, or payload identity. Current workers make the record retrievable but
do not launch the target transaction automatically.

A protected-pending START intentionally has no matching work row before an
explicit commit. Do not synthesize one: a successful SYNCPOINT transitions the
record and idempotently admits its deterministic work. If recovery finds a
pending record from that run with no work after an interrupted commit, retry
the same syncpoint so admission is healed. SYNCPOINT ROLLBACK removes only
still-protected records; restoring only the UOW row or only the interval rows
can reverse that decision.

Typed ABEND also deletes still-protected rows for its exact issuing run before
returning the ABEND disposition or transferring to an installed exit. No work
row should exist for those deleted requests. If manual recovery restores such a
provider row without its pre-abend task state, leave it quarantined rather than
admitting work.

Normal task completion and highest-level RETURN are implicit committing
syncpoints for protected START. Admit the deterministic work before removing
the volatile run. Known execution failure is rollback: delete still-protected
rows and never enqueue them. Suspension is neither outcome. Disconnect/timeout
cleanup remains separate. If execution terminalization and CICS cleanup are
separated by a crash, quarantine the protected rows until the durable terminal
disposition is reconciled; do not infer commit solely from a missing run.

Do not force CANCEL across a protected-pending row. IBM semantics allow CANCEL
only after that START is committed. Before commit, preserve the row and return
NOTFND; after commit, restore and cancel the pending interval row together with
its deterministic work row and cancellation flag.

For START without REQID, preserve the mutation idempotency key and canonical
request bytes: together they deterministically regenerate the eight-character
EIBREQID and therefore the provider-row and work IDs. A hash collision with a
different retained producer fails through the duplicate-REQID fence; never
rename only one side or invent a replacement EIBREQID during recovery.

A suspended RETRIEVE WAIT has no provider-side waiter row. Preserve its online
exchange, machine checkpoint, and execution state together; the checkpoint
rewinds to the same RETRIEVE statement, while the interval row remains ready or
absent until ordinary worker promotion. Recovery must not synthesize ENDDATA,
consume a record on behalf of the task, or close the exchange. Resume is
currently explicit; automatic wake and shutdown/deadlock completion are not
part of this bounded child.

For START USERID, recover the selected execution principal from the interval
row; do not substitute the issuer after a successful surrogate check. A denied
check is pre-mutation and must leave no interval or work row. Replaying an
accepted producer must preserve both its canonical USERID operand and stored
principal, while a conflicting identity under the same REQID remains an IOERR
duplicate rather than an identity rewrite.

A typed local CANCEL leaves the interval row as a cancelled replay tombstone
and calls the work store's cancellation transition. Queued work becomes
cancelled immediately; claimed work retains its lease with
`cancellation_requested=true`, and the failed promotion/release path observes
that flag. Restore the interval and work rows together. Deleting the tombstone
can turn a crash-gap retry into NOTFND, while requeueing canceled work creates a
permanent failed-promotion loop. Immediate reuse of a cancelled REQID is not
supported by this bounded slice.

Back up `cics-delay-v1` provider rows with their matching shared work rows of
the same generation. A row binds one task/run-unit and source-statement identity
to its packed interval, optional application REQID, expiration tick, producing
effect, and deterministic work ID. Pending, ready, consumed, and abandoned
states are CAS-versioned; ready/consumed rows also retain any other-task CANCEL
identity needed for exact replay and RESP2 23. Replacing either row can wake the
wrong source cycle or turn a later loop iteration into a replay. On restart, let
the shared workers reclaim expired leases and promote only due work. Do not mark
a delay ready, consumed, abandoned, or completed by hand. Restore named-delay,
work, session, online-exchange, and machine-continuation rows together.
Disconnect and timeout abandon outstanding task-owned delays and cancel their
work; repeating those cleanup paths is safe. Current resume is request-driven:
the original online exchange must be invoked again after due promotion or local
named cancellation. Automatic redispatch is not yet provided.

Back up `cics-session` rows containing task association or HANDLE state before
enabling typed `SET ASSOCIATION USERCORRDATA` or durable handlers. Current
`MECS7` rows carry at most 64 correlator bytes, the last mutation key and
canonical request digest, and the complete bounded condition/AID/IGNORE/typed
ABEND PUSH/POP state. Historical `MECS1`–`MECS4` rows decode with an empty
correlator, `MECS5` and `MECS6` retain theirs, versions 1–5 have empty HANDLE
state, and `MECS6` retains label-only exits. If the session write succeeds but
the outer replay row does not, retain the session row and retry only the
identical key and request. A different digest is an idempotency conflict, and
manual deletion of the session row loses both authoritative values.

A `cics-scheduler` suspension from `CHANGE TASK` or `SUSPEND` is a one-shot
yield, not a terminal handoff and not a command retry. Preserve the execution
checkpoint, online exchange, and `online-machine-continuation` row, then resume
the same execution through normal bounded admission. Current `MEOM4` rows carry
the changed priority and optional staged program transfer; `MEOM3` priority
rows and historical `MEOM2` rows remain readable. For a nonempty transfer
marker, retain both rows: recovery must observe the predecessor's exact
`HandoffCompleted` event and CAS the recorded next exchange before clearing the
marker. Deleting either continuation can lose the post-command program counter
and must be treated as failed recovery, not permission to reissue the CICS
command.

If an exchange points at a terminal execution, first finish the abandoned
COBOL/CICS run, remove its interpreter checkpoint, and delete the exchange.
Preserve `online-machine-continuation` only when the final lifecycle event is
`HandoffCompleted`; retain the session's HANDLE state in that case because it
belongs to the same logical task. Clear HANDLE state for every other terminal
outcome before admitting a fresh task. That continuation record is the durable
owner of the next pseudo-conversation task. Ordinary completion returns
success. Cancellation and timeout retain their exact categories, while
failed/dead-letter executions return a conservative provider failure when no
exact condition payload was persisted. Never retry a terminal execution
identity.

Local wakeups are reconstructible by scanning authoritative work rows.
Dead-letter work requires an operator decision; it is never treated as
completed.

PostgreSQL records the configured state-row and artifact-object bounds in
transaction-locked `store_quota` rows. Every count-changing transaction reserves
or releases quota with its data mutation. All nodes must use identical limits;
startup rejects limit drift, over-capacity legacy rows, or a quota/count
mismatch. Drain old binaries before the first quota-aware startup because they
do not maintain these reservations.

## Retention and saturation

The configured retention policy has three distinct lifetimes: terminal
lifecycle/outbox history, mutation idempotency receipts, and retained archive
batches. Do not set them from storage pressure alone. The idempotency lifetime
must exceed every supported retry window; lifecycle retention must exceed
checkpoint and incident-recovery needs; archive retention must exceed the
audit/backup recovery horizon.

Use `ProductServer::operator_retention_forecast` from an authenticated embedding
control plane or the standalone binary's offline `retention forecast` command.
Alert at the configured low watermark and start bounded archive batches before
the high watermark. Forecasts report source, archive, and conservative
observation headroom; PostgreSQL source usage includes unrelated namespaces in
the shared quota. A full live store can compact an eligible multi-row batch on
its net row delta because archives and observations have dedicated bounded
authorities, but operators should act before saturation.

Run `operator_archive_and_prune` in `RetentionTarget::ALL` order, or use one
offline `retention maintain` pass, which uses that frozen provider-first order
and deletes only whole archive batches past `archive_ticks`, cumulatively
bounded by source rows. Inspect every returned archive identifier and retain
the machine-readable receipts. If the oldest historical archive alone exceeds
the lowered bound, inspect the reported content-addressed ID and rerun with
`--authorize-oversized-archive EXACT_ID`; a missing, wrong, stale, or
unnecessary ID deletes nothing. Corrupt batches fail closed and require
forensic repair. Never manually delete active provider-state rows or archive
manifests.

Maintenance accepts only durable SQLite and PostgreSQL profiles (not Memory or
in-memory SQLite) and opens only the migrated state store plus the shared
store-only planner. It does not open Product, providers, artifact/package
authorities, recovery caches, a listener, or workers. Drain all writers first;
bounded jittered conflict retries do not
guarantee progress under online write traffic. If more eligible rows remain,
run another bounded pass. See the [operations runbook](OPERATIONS.md) for the
exact commands, backup gate, partial-pass behavior, and restart procedure.

Retention will not remove pending notifications, stale effect intents, unknown
outcomes, non-terminal executions, checkpoint-owned state, or legacy rows with
no trustworthy age. If those rows cause saturation, resolve the owning recovery
workflow rather than weakening the watermark. Use the bounded `retention
legacy` listing and explicit CAS-fenced `retention reconcile` command only after
verifying the exact row's provenance; the command never guesses an execution
owner. See the
[retention contract](../contracts/RETENTION-LIFECYCLE-V1.md) for exact
eligibility and concurrency semantics.
