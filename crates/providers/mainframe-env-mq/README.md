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

Direct MQ commit/backout calls with a valid CICS execution-context binding
return MQCC 2 / MQRC 2012 without changing UOW state. Internal CICS SYNCPOINT
dispatch carries nested and outer effect-origin attestations, which MQ replay
validation binds to the exact run, sequence, and effect identities.

The MQ-1502 object catalog exports typed queue manager, queue, topic,
subscription, and process definitions with deterministic alias/remote
resolution, model-instance lifecycle, and a strict restart snapshot codec.
It is a provider-owned kernel; the existing host request routes do not yet
execute these object forms. Queue service names preserve case and strip only
permitted trailing blanks or a null ending significant data. Invalid names
are rejected before state mutation.

## Documentation

[Documentation portal](../../../docs/README.md) · [Current package map](../../../docs/architecture/PACKAGE-MAP.md) · [Embedding guide](../../../docs/guides/EMBEDDING.md) · [Contribution and verification](../../../CONTRIBUTING.md)
