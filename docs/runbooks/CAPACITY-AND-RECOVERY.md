# Capacity and recovery

Size limits are explicit for HTTP bodies/concurrency, compiler input and IR,
dataset records/catalog bytes, RACF objects/audit, CICS sessions/screens/queues,
JES jobs/active work/spool/events, SQL rows/payloads, and immutable artifacts.
At 100% of any bound, admission fails before mutation. No queue or retry loop
grows automatically.

The core server owns exactly two JES workers. Their generation-scoped claim
prevents them from consuming another durable work lane. Higher JES priority is
selected first; oldest available admission tick plus work ID provides a
deterministic FIFO tie-break within a priority. Each
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

Back up `cics-session` rows containing task association state before enabling
typed `SET ASSOCIATION USERCORRDATA`. Current `MECS5` rows carry at most 64
correlator bytes plus the last mutation key and canonical request digest;
historical `MECS1`–`MECS4` rows decode with an empty correlator. If the session
write succeeds but the outer replay row does not, retain the session row and
retry only the identical key and request. A different digest is an idempotency
conflict, and manual deletion of the session row loses the authoritative value.

A `cics-scheduler` suspension from `CHANGE TASK` or `SUSPEND` is a one-shot
yield, not a terminal handoff and not a command retry. Preserve the execution
checkpoint, online exchange, and `online-machine-continuation` row, then resume
the same execution through normal bounded admission. Current `MEOM3` rows carry
the changed priority; historical `MEOM2` rows remain readable and inherit the
priority already stored by their online exchange. Deleting either continuation
can lose the post-command program counter and must be treated as failed
recovery, not permission to reissue the CICS command.

If an exchange points at a terminal execution, first finish the abandoned
COBOL/CICS run, remove its interpreter checkpoint, and delete the exchange.
Preserve `online-machine-continuation` only when the final lifecycle event is
`HandoffCompleted`; that record is the durable owner of the next
pseudo-conversation task. Ordinary completion returns success. Cancellation
and timeout retain their exact categories, while failed/dead-letter executions
return a conservative provider failure when no exact condition payload was
persisted. Never retry a terminal execution identity.

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
