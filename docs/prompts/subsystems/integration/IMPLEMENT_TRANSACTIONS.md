# Execution Prompt — Cross-resource integration — Transactions and recovery

Subsystem: **integration**
Phase: **transactions**

Completion dependencies: jes.execution, cics.system-api, db2.programming, ims.programming, mq.programming

Use this prompt from the repository root. The
[common execution contract](../README.md#common-execution-contract) is normative.
Apply its [hardened slice acceptance](../README.md#hardened-slice-acceptance),
[early participant contract](../README.md#early-transaction-participant-contract),
and [licensed-harness preparation](../README.md#licensed-harness-preparation)
requirements alongside the phase-specific boundaries below.

---

You are implementing **mainframe-env integration.transactions: complete cross-resource transaction,
security, and failure semantics** across JES, CICS, datasets, Db2, IMS, and MQ.

## Read and verify first

Read `docs/prompts/subsystems/README.md`,
`docs/delivery/subsystems/integration/transactions-plan.md`, all UOW/effect/principal/recovery/
store contracts, and accepted evidence for jes.execution, cics.system-api, db2.programming, ims.programming and
mq.programming. Verify exact provider state, transaction, migration, failure and
capability versions before integrating adapters.

Provider-specific adapter tests may be prepared as dependencies freeze, but the
public mixed-state model cannot complete on partial or substituted providers.

## Implement in this order

1. Complete **INT-1601** on the accepted common effect/UOW coordinator. Verify
   the early-frozen participant contracts, then close prepare, commit, rollback,
   compensation, in-doubt, heuristic and unknown outcomes with compatible
   durable log/state/migration schemas; do not create another coordinator.
2. Implement **INT-1602/INT-1603** principal/delegation, authorization decision,
   correlation/causality, deadlines, cancellation, idempotency, retry,
   backpressure, overload and audit propagation.
3. Implement **INT-1604** lock/effect ordering, crash points, recovery,
   reconciliation, operator resolution and bounded retention.
4. Implement **INT-1605** mixed-provider backup/restore, migration, replay,
   compatibility and rollback.
5. Implement **INT-1606** exhaustive mixed-resource commit/failure matrix, soak,
   chaos, concurrency, 2x overload, restart and differential journeys.

## Early participant handoff and coherent restore

The common early participant-contract rule applies before dependent provider
adapters integrate. INT-1601 reviews and completes that accepted boundary; integration.transactions
is not the first time CICS, Db2, IMS and MQ agree on transaction ownership,
capabilities, prepare applicability, compensation limits, fencing, idempotency,
lock order or recovery ownership. Record early slices under the existing parent;
they neither complete integration.transactions nor expose unfinished mixed-resource behavior.

INT-1605 must restore a coherent mixed-resource recovery boundary, with compatible
provider/schema generations and valid journal, checkpoint, artifact, replay and
retention references. Independent successful provider restores are insufficient.
Inject partial backup/restore, provider lag/loss and restart around that boundary;
reject inconsistent sets or retain explicit recoverable/in-doubt/unknown states.
INT-1606 proves all required capability combinations and failure obligations on
one candidate without claiming universal atomicity or exactly-once behavior.

## Reuse and architecture guardrails

- The owned effect/UOW state machine and durable intent/result log remain the
  semantic authority for prepare, commit, rollback, compensation, heuristic,
  in-doubt, and unknown outcomes. Temporal, Restate, DBOS, Apalis, or another
  workflow/job framework may be an optional scheduling adapter only after an
  ADR, license/maturity/MSRV review, and mixed-provider semantic-gap matrix.
- External workflow claims such as exactly-once execution, automatic retries,
  durable timers, or serialized object access cannot replace explicit provider
  reconciliation, idempotency, lease, cancellation, lock-order, and outcome
  evidence. Framework histories and state types cannot enter public UOW or
  checkpoint contracts.
- Use OpenTelemetry-compatible libraries for trace/metric/log correlation at
  adapter boundaries, not as audit or security authority. Validate and sanitize
  incoming propagation; never place principal credentials, grants, secrets, or
  compatibility decisions in telemetry baggage.
- Add bounded model and concurrency tests with reviewed tools such as
  Stateright, Loom, or Shuttle for the coordinator, lease, lock-order, retry,
  cancellation, crash, and recovery state spaces. Preserve replayable schedules
  and counterexamples as candidate-bound evidence.
- Reuse one shared migration, backup/restore, artifact, principal, deadline,
  cancellation, outbox, and evidence implementation across all providers.

## Version-specific invariants

- Exactly one coordinator owns cross-provider UOW state; no provider-private
  shortcut may bypass common transactions or SAF authorities.
- Preserve principal, delegation, authorization, correlation, causality,
  deadline, cancellation, idempotency and audit across every boundary.
- Define and test deterministic effect and lock order. Retries occur only at
  documented safe boundaries and cannot silently duplicate committed effects.
- Heuristic, in-doubt and unknown outcomes remain visible and recoverable; do not
  turn partial success into success or claim universal atomic/exactly-once behavior.
- Final evidence is produced from one candidate and one compatible state schema.

## Completion gate

Do not finish until the required mixed commit/rollback/compensation/heuristic/
in-doubt/unknown-outcome matrix passes at every injected failure point; identity,
deadline, cancellation, idempotency, audit and causality remain intact; restart,
retry, migration, backup/restore, overload and concurrency preserve invariants;
there is no cross-principal leak or UOW bypass; and full CardDemo journeys pass
under mixed-resource fault injection.

At handoff, provide the UOW state/effect/lock model, full matrix and crash-point
results, provider adapter versions, migration/restore evidence, known outcome
limits and full validation on the exact unchanged candidate.
