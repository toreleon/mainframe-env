# Execution Prompt — Implement mainframe-env 0.15.0

Target version: **0.15.0**  
Completion dependencies: 0.4.0, 0.5.0

Use this prompt from the repository root. The
[common execution contract](README.md#common-execution-contract) is normative.

---

You are implementing **mainframe-env 0.15.0: complete IBM MQ 9.4 programming
surface** for all 26 unique pinned MQI calls.

## Read and verify first

Read `docs/prompts/coverage-versions/README.md`,
`docs/delivery/coverage-versions/0.15.0.md`, the canonical MQ documentation-row
deduplication and generated MQI catalog, MQ/provider/host/UOW/security contracts,
and accepted 0.4.0 and 0.5.0 evidence.

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
