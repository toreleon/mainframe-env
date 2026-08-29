# Capacity and recovery

Size limits are explicit for HTTP bodies/concurrency, compiler input and IR,
dataset records/catalog bytes, RACF objects/audit, CICS sessions/screens/queues,
JES jobs/active work/spool/events, SQL rows/payloads, and immutable artifacts.
At 100% of any bound, admission fails before mutation. No queue or retry loop
grows automatically.

After process failure, reopen the store at migration head
`0001-durable-state`. Running JES work returns to queued state until its attempt
limit, expired work leases are reclaimable with a higher attempt, suspended
CICS sessions stay suspended, and incomplete effect intents remain explicit
unknown outcomes. Local wakeups are reconstructible by scanning authoritative
work rows. Dead-letter work requires an operator decision; it is never treated
as completed.
