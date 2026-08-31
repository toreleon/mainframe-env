# mainframe-env-mq

Ownership: bounded durable MQ queues, handles, triggers, correlation,
syncpoint, replay, failure reconciliation, and restart state.

The provider owns versioned Apache-2.0 behavioral compatibility definitions
for CMQGMOV, CMQMDV, CMQODV, CMQPMOV, CMQTML, and CMQV. Callers explicitly add
the source library to a compilation closure; the compiler has no embedded MQ
ABI, and the inventory grants no semantic coverage credit.
