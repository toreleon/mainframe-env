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
the sole published handle identity. The registry supplies identity data only.
The additive validator now checks source-bound ordered signatures, structure
identities and versions, option families and documented combinations without
registering a handler; numeric wire legality and execution remain pending.

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

The provider recognizes both the typed `mainframe-env.mq.host-context@1`
binding and the existing typed `mainframe-env.cics.execution-context@1`
binding. Direct `MQCMIT` and `MQBACK` calls are checked before authorization,
replay, lock acquisition, or queue-state access. CICS returns MQCC 2 / MQRC
2012; non-batch IMS and host-coordinator-owned contexts are rejected by the
same source-bound matrix. The existing CICS SYNCPOINT dispatch carries nested
and outer effect-origin bindings; MQ replay validation binds them to the exact
run, sequence, idempotency key, and outer effect before persistence. Missing,
contradictory, partial, or malformed provenance fails closed.

`MqSyncpointCall`, `MqHostEnvironment`, and `MqSyncpointOwner` define the shared
direct-call applicability matrix for `MQBACK`, `MQBEGIN`, and `MQCMIT`. Batch,
IMS batch DL/I, and other queue-manager-owned bindings admit these calls. CICS
and non-batch IMS reject application commit/backout; MQ client bindings also
reject `MQBEGIN`. An external coordinator rejects all three direct calls with
MQCC 2 / MQRC 2012. The current provider route enforces this matrix for direct
commit and backout in z/OS batch, IMS batch DL/I, CICS, IMS, MQI client and
other bindings. `MQBEGIN` has no executable public request route yet.

## Object lifecycle kernel

`mainframe-env-mq::object` owns bounded, case-sensitive object names and typed
definitions for queue managers, local, alias, remote, and model queues, topics,
subscriptions, and processes. Its catalog resolves aliases and remote routes
deterministically with cycle and depth rejection. Model instances have explicit
owner and close rules. A strict versioned snapshot codec rejects noncanonical
names, corrupt rows, and unsupported schema versions before restoration.

The existing queue service uses the same name rule for definitions, lookups,
request queue selectors, and trigger programs. It removes permitted trailing
blanks or a null ending significant data, preserves case, and rejects leading
or embedded blanks before durable mutation. The service persists the typed
catalog in the shared provider-row store and routes the compatibility queue
operations through local queues and aliases, including atomic migration from
the legacy queue-only manifest. Dynamic, remote, topic, subscription, process,
distribution-list and MQINQ execution remain fail-closed or pending.

## Frozen object and message request contracts

The host API now exposes a bounded, source-bound MQOPEN/MQCLOSE vocabulary for
object lookup, access, context, dynamic names and close lifecycle. It also
exposes bounded message descriptors, typed properties, identifier selection,
browse/wait/truncation, grouping/segmentation, distribution outcomes and
explicit duplicate or unknown states. These are non-executable contracts:
unsupported forms and pending provider authorities stay explicit, and their
presence grants no behavioral or licensed coverage.

## Reviewed completion wire identities

The existing completion/reason catalog now uses private source-projection schema
`mainframe-env.mq-completion-reason-catalog@2`. Its additive wire projection
reviews `MQCC_OK=0`, `MQCC_WARNING=1` and `MQCC_FAILED=2` from
`SSFKSJ_9.4.0/refdev/q090560_.html` in the separately pinned
`ibm-mq-9.4-programming-supplements-2026-09-12` scope. Decimal and eight-digit
hexadecimal identities agree and fit the API's signed 32-bit MQCC and signed SQL
integer representation; no SQL storage or general structure layout changes.
`MQCC_UNKNOWN=-1` is recorded as excluded from ordinary reviewed call returns.
MQCMIT catalog row `0007` corroborates the MQLONG output role; MQCBC field
context remains callback input and never creates a return pair for MQCB_FUNCTION.

`MqCompletion::wire_number` and `from_wire_number` map these three identities.
`MqReviewedStatus::wire_pair` emits their MQCC with the already admitted reason;
`from_wire_pair` applies the existing call-specific reason admission. Unknown,
negative or out-of-range completion values, reason aliases needing explicit
symbols, pending collisions and callback notifications fail closed. All ten
reason declarations remain pending and all 1,030 pairs retain their source pins.

The generator validates the unchanged original `@1` call-return artifact digest
by reconstructing its exact JSON representation. `MQ_STATUS_CATALOG_SHA256`
continues to bind that identity in existing canonical status bytes; the additive
completion projection has its own source digest. Canonical encoders, outcome
forms and old golden bytes are unchanged. Offline checks validate artifact
closure; cache-backed generator/verifier checks independently reproduce the
selected constants and corroborating fragments before comparison. This maps
identities only and does not calculate runtime results or register an ABI.
The archive provenance remains in-progress, without independent browser
reproduction and predating the MQINQ re-pin; no freshness, same-snapshot,
behavioral, licensed or execution claim follows from this review.

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
