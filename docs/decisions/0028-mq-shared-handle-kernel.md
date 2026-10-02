# Shared MQ handle-family kernel

Status: **Proposed**
Owner: **MQ contract and provider maintainers**
Scope: **private v0.15 MQ kernel composition, not public route acceptance**
Applies from: **mainframe-env 0.15.0**

## Decision

Compose volatile message properties inside the pub/sub queue-manager kernel,
using the existing `MqHandleRegistry` once. A connection can then address both
families, and one slot budget governs connection, object, subscription and
message handles. Registry owner, generation and epoch checks remain unchanged.

Connection, processing-unit and epoch retirement uses the enclosing kernel's
methods, reclaiming properties, retired subscription bindings and callbacks.
Durable subscriptions survive handle retirement; non-durable definitions with
no remaining binding are removed. Epoch retirement does not decide staged
transactions: the existing UOW owner must explicitly commit or back out.
The low-level registry accessor remains for compatibility, not a new product
dispatch route. Message properties have no durable snapshot or wire claim.

## Sources and compatibility

Baseline `ibm-mq-9.4-mqi-2026-08-31`, catalog rows `0008` MQCONN, `0010` MQCRTMH,
`0012` MQDISC and `0025` MQSUB bind connection reuse and lifetime. Pinned topics
are `q101760_`, `q101780_`, `q101800_` and `q101930_` below
`SSFKSJ_9.4.0/refdev/`, reviewed offline against their manifest hashes.
The frozen registry retains CICS default-connection no-op semantics and explicit
unassociated HMSG lifetime. No numeric options, channels, shared transaction
coordinator, private log or store are added. Existing pub/sub snapshot bytes
are unchanged; restore still creates a fresh stopped volatile handle registry.

## Remaining acceptance

Public MQI routing, trusted context/SAF/audit, participant acceptance, durable
service integration and licensed differentials remain separate required gates.
Kernel tests provide no official per-call or licensed coverage credit.
