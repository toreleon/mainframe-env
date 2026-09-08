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

After process failure, reopen the store at migration head
`0001-durable-state`. Running JES work returns to queued state until its attempt
limit, expired work leases are reclaimable only with a higher durable fencing
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

Local wakeups are reconstructible by scanning authoritative work rows.
Dead-letter work requires an operator decision; it is never treated as
completed.

PostgreSQL records the configured state-row and artifact-object bounds in
transaction-locked `store_quota` rows. Every count-changing transaction reserves
or releases quota with its data mutation. All nodes must use identical limits;
startup rejects limit drift, over-capacity legacy rows, or a quota/count
mismatch. Drain old binaries before the first quota-aware startup because they
do not maintain these reservations.
