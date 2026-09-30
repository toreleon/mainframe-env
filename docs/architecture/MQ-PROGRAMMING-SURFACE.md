# IBM MQ programming-surface ownership

Status: **Normative identity boundary; semantics are implemented incrementally**

Owner: `mainframe-env-host-api` contracts and `mainframe-env-mq` provider

Scope: MQI denominator, source provenance, host context and semantic authority

Applies from: mainframe-env 0.15.0

## Denominator and provenance

The immutable 0.2 official catalog defines 26 unique IBM MQ 9.4 MQI calls.
The pinned source call list displays 27 rows because it lists `MQMHBUF` twice.
That duplicate is retained as source provenance and never increments coverage.

`conformance/0.15/mq/source-call-list.json` records the 27 positions, their
normalized official rows, and the exact pinned call-list topic. The generated
`MqMqiCallIdentityDescriptor` registry joins those positions to the per-call
topic paths and SHA-256 pins in the immutable MQ topic manifest. The registry
is identity-only: it neither selects a handler nor advertises execution.

`conformance/0.15/mq/structure-status-catalog.json` adds the ordered,
source-bound signatures for the same 26 calls. The generated host API exposes
169 parameter descriptors with structure and version symbols, options,
selectors, completion and reason families, and handle roles. All 26 pinned
call topics have matching retained HTML, including the re-pinned MQINQ topic
`SSFKSJ_9.4.0/refdev/q101840_.html` (SHA-256
`03e3347bbf16d2f8e3a9061e921dbfca7a3afd0fe3bc13418ebdf47bb652ce1b`).
The MQBUFMH spelling anomaly is recorded in the catalog, while `MQHMSG` is
the sole published handle identity. The registry supplies identity data only;
option legality and executable handlers remain pending.

## Authority boundary

`mainframe-env-mq` is the one owned semantic authority for queue managers,
objects, handles, messages, callbacks, properties, delivery and recovery.
Stable host contracts live in `mainframe-env-host-api`; application packages
provide topology; shared store, principal/SAF, canonical effect and UOW
contracts retain their existing ownership.

A native IBM MQ client can appear only behind an explicit licensed adapter and
profile. It is not an owned-simulator fallback, does not add simulator coverage,
and cannot act as both product and expectation in a differential test. A
commodity broker can be only a replaceable physical adapter after a reviewed
semantic-gap and failure matrix; its acknowledgements or transaction model do
not establish MQI compatibility.

## Host-owned syncpoint rule

The pinned `MQCMIT` and `MQBACK` topics restrict those calls on z/OS to batch,
including IMS batch DL/I. CICS applications use CICS syncpoint commands;
non-batch IMS applications use IMS coordination calls. `MQBEGIN` distinguishes
queue-manager-coordinated local and global units from externally coordinated
units and is invalid in an MQ client environment.

The typed call contract must therefore carry the execution context and
syncpoint owner. A forbidden context returns the exact MQ completion/reason
condition without mutating queue-manager state. `MQCMIT` must never be exposed
as a generic cross-subsystem commit.

The provider recognizes the typed `mainframe-env.cics.execution-context@1`
invocation binding. A direct `MQCMIT` or `MQBACK` in CICS returns MQCC 2 / MQRC
2012 before authorization, replay, or queue state access. The existing CICS
SYNCPOINT dispatch carries nested and outer effect-origin bindings; MQ replay
validation binds them to the exact run, sequence, idempotency key, and outer
effect before persistence. Partial or malformed provenance fails closed.

`MqSyncpointCall`, `MqHostEnvironment`, and `MqSyncpointOwner` define the shared
direct-call applicability matrix for `MQBACK`, `MQBEGIN`, and `MQCMIT`. Batch,
IMS batch DL/I, and other queue-manager-owned bindings admit these calls. CICS
and non-batch IMS reject application commit/backout; MQ client bindings also
reject `MQBEGIN`. An external coordinator rejects all three direct calls with
MQCC 2 / MQRC 2012. IMS and MQ client provider enforcement remain pending.

## Coverage boundary

The MQ-1506 licensed adapter at
`conformance/0.15/oracles/mq-licensed-differential.json` binds the 26-call
denominator to independent fixture identities and a bounded external receipt.
Its verifier requires an authorized IBM MQ 9.4 environment, exact service and
candidate identities, distinct product and oracle runners, normalized
digest-only observations, and one observation per call. An absent or rejected
receipt grants zero differential credit. The in-repository fixture index and
mutant tests validate the contract; they are not licensed execution evidence.

Catalog and generated-registry checks prove only:

- the 26-call normalized denominator;
- the exact 27-row source provenance;
- official row, label, topic-path and topic-digest joins; and
- deterministic generated identity bytes.

Behavioral credit requires independently bound Conformance IR obligations and
verdicts for each applicable recognized, validated, executed, conditioned,
recovered and differential gate. Missing licensed or source evidence remains
pending; it is never inferred from registry presence or broad workload success.
