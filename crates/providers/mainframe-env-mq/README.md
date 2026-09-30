# mainframe-env-mq

Ownership: bounded durable MQ queues, handles, triggers, correlation,
syncpoint, replay, failure reconciliation, and restart state.

Queues, run-scoped handles/units of work, and replay receipts persist as
independently versioned object rows under the
[provider row contract](../../../docs/contracts/PROVIDER-ROW-PERSISTENCE-V1.md).

The provider owns versioned Apache-2.0 behavioral compatibility definitions
for CMQGMOV, CMQMDV, CMQODV, CMQPMOV, CMQTML, and CMQV. Callers explicitly add
the source library to a compilation closure; the compiler has no embedded MQ
ABI, and the inventory grants no semantic coverage credit.

The IBM MQ 9.4 MQI denominator and provider authority boundary are defined in
the [MQ programming-surface architecture](../../../docs/architecture/MQ-PROGRAMMING-SURFACE.md).
The generated 26-call host registry preserves all 27 source-list positions but
does not advertise execution or grant coverage.
