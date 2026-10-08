# Execution Prompt — IBM MQ — MQI programming surface

Subsystem: **mq**
Phase: **programming**

Completion dependencies: cobol.execution, racf.security

Use this prompt from the repository root. The
[common execution contract](../README.md#common-execution-contract) is normative.
Apply its [hardened slice acceptance](../README.md#hardened-slice-acceptance),
[early participant contract](../README.md#early-transaction-participant-contract),
and [licensed-harness preparation](../README.md#licensed-harness-preparation)
requirements alongside the phase-specific boundaries below.

---

You are implementing **mainframe-env mq.programming: complete IBM MQ 9.4 programming
surface** for all 26 unique pinned MQI calls.

## Read and verify first

Read `docs/prompts/subsystems/README.md`,
`docs/delivery/subsystems/mq/programming-plan.md`, the canonical MQ documentation-row
deduplication and generated MQI catalog, MQ/provider/host/UOW/security contracts,
and accepted cobol.execution and racf.security evidence.

## Implement in this order

1. Freeze **MQ-1501** call/structure/selector/option/version, handle,
   completion/reason, message/effect, capability and generated registry contracts.
2. Implement **MQ-1502** queue-manager and local/alias/remote/model queue, topic,
   subscription, process and pinned channel-facing object lifecycle/resolution.
3. Implement **MQ-1503/MQ-1504** put/get/browse, descriptors, properties,
   selectors, grouping/segmentation, distribution lists, pub/sub, callbacks,
   asynchronous consumption and triggering.
4. Implement **MQ-1505** persistence, syncpoint, backout, dead-letter, expiry,
   retry, restart, logging and recovery.
5. Implement **MQ-1506** SAF, context/alternate-user authorization, malformed/
   boundary/limit, concurrency, overload, compatibility and licensed
   differential suites.

## MQ host-context and early durability acceptance

MQ-1501 must freeze call, host environment, structure version, option, handle
lifetime and syncpoint-owner applicability. Include CICS, IMS and batch/client
contexts where the pinned MQI surface applies. MQCMIT must not become a generic
commit operation in CICS or in IMS environments other than batch DL/I; use the
pinned host-owned syncpoint rules and exact rejection/status behavior. Correct
rejection in a forbidden context is required compatibility, not a missing call.

Every mutating MQ-1502–MQ-1504 slice includes its applicable persistence,
syncpoint/backout, replay, failure and restart obligations before integration.
MQ-1505/MQ-1506 complete and stress delivery/recovery/security guarantees rather
than introducing them after put/get or pub/sub has already integrated.

Any optional native IBM-client pass-through provider is an explicit, separately
selected profile with its own capability and evidence identity. Native execution
cannot increment owned-simulator coverage, become hidden fallback or supply the
product expectation for its own differential test. Preserve all 26 unique calls
and the 27 source rows in provenance without duplicate credit.

## Reuse and architecture guardrails

- MQI calls, structures, handles, completion/reason codes, object resolution,
  syncpoint, delivery, backout, triggering, ordering, duplicate, and unknown
  outcomes remain one owned queue-manager semantic authority.
- Use the supported IBM MQ client/MQI through a bounded adapter for licensed
  differential execution and an optional pass-through provider. Do not
  reimplement IBM channel wire protocols or treat native IBM code as the
  product execution fallback.
- RabbitMQ, NATS, Kafka, Pulsar, or another broker may be evaluated as a
  replaceable physical adapter only with a pinned semantic-gap/failure matrix.
  Broker acknowledgements, transactions, retries, selectors, ordering, or
  exactly-once claims do not prove MQI compatibility.
- Reuse the shared catalog compiler, package runtime, principal/SAF, store,
  artifact, effect/UOW, migration, cancellation, backpressure, and evidence
  authorities. Do not add a provider-private job scheduler or durable log.

## Version-specific invariants

- Use 26 unique MQI calls as the denominator; retain the 27 source-document rows
  in provenance without double-counting an aliased/duplicate call.
- Queue-manager/object/message state has one generic typed authority. Application
  queue/topic/process names and topology come from packages/configuration.
- Preserve exact structure-version, option legality, handles, completion/reason
  codes, syncpoint and message-ordering semantics.
- Duplicate delivery, retry, in-doubt and unknown outcomes are explicit. Never
  infer or claim exactly-once behavior without a proven contract.
- Bound messages, properties, selectors, queues, subscriptions, callbacks,
  retained state and consumers; fail closed on overload or authorization failure.

## Completion gate

Do not finish until 26/26 unique MQI calls pass all applicable gates; structure,
option, status, handle, lifecycle, point-to-point, properties, pub/sub, callback,
security, persistence, overload, syncpoint, restart and recovery matrices pass;
licensed IBM MQ 9.4 differentials pass; and CardDemo MQ assets remain exact
through application packages without production topology branches.

At handoff, provide canonical denominator/provenance evidence, per-call gate
counts, object/state/migration digests, delivery/recovery matrices, oracle
receipts and full validation on the unchanged candidate.
