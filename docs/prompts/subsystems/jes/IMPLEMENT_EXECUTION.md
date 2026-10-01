# Execution Prompt — JES2 and utilities — Jobs, spool and utilities

Subsystem: **jes**
Phase: **execution**

Target version: **0.8.0**
Completion dependencies: racf.security, dataset.data, jcl.planning

Use this prompt from the repository root. The
[common execution contract](../README.md#common-execution-contract) is normative.

---

You are implementing **mainframe-env 0.8.0: complete JES2 execution and real
utility semantics** over accepted JCL plans, dataset authorities, and SAF.

## Read and verify first

Read `docs/prompts/subsystems/README.md`,
`docs/delivery/subsystems/jes/execution-plan.md`, and accepted evidence/contracts from
0.5.0, 0.6.0, and 0.7.0. Verify their exact versions for principals/SAF,
allocation/locking, typed plans, conditions, durable state, and migrations.

If any dependency is incomplete, only isolated scheduler/utility fixtures and
private adapters may proceed. Do not merge a public JES execution route.

## Implement in this order

1. Freeze job, step, queue, initiator, spool, output, utility, checkpoint,
   condition/abend, cancellation, and durable-state contracts.
2. Implement **JES-801/JES-802** lifecycle scheduling, classes/priorities,
   initiators, conditions, DD allocation/DISP/concatenation/temp data and cleanup.
3. Implement **JES-803/JES-804** spool/output/routing/retention, held/released/
   cancelled state, started tasks, internal reader, NJE/MAS abstractions and
   authorized control operations.
4. Implement **JES-805** registered real utility framework and semantic families;
   remove program-name dispatch and all summary-only routes.
5. Implement **JES-806** restart, cancellation, overload, crash-point,
   backup/restore, migration and licensed differential suites.

## Approved 2026-09-04 completion policy

For this development cycle, a licensed z/OS 3.2/JES2 receipt is unavailable.
The user-approved 0.8 disposition is
`pass-with-licensed-differential-pending`:

- preserve the licensed differential numerator at exactly 0/16 and keep
  `cargo xtask jes-oracle --check` fail-closed;
- never treat Hercules, MVS 3.8J, modeled/generated output, CardDemo,
  historical transcripts, or current-product output as licensed z/OS 3.2/JES2
  differential evidence;
- complete JES-806 from the bounded local restart, cancellation, overload,
  crash-point, backup/restore, migration, CardDemo, and unchanged-candidate
  gates while reporting the licensed row explicitly pending; and
- defer the real licensed 16-scenario campaign to the 0.17 CER-1702
  release-certification hard gate, where it remains mandatory before 1.0.

## Reuse and architecture guardrails

- Extend the common execution/work stores, leases, outbox, scheduler lanes,
  checkpoint envelope, cancellation protocol, artifact/object store, principal,
  effect/UOW, migration, and evidence harness. JES must not create parallel
  authorities for work ownership, retries, timers, idempotency, or blobs.
- JES class/initiator selection, job/step state, DISP, return-code/abend,
  restart, spool semantics, and operator outcomes remain owned deterministic
  state machines. A background-job or durable-workflow framework may be an
  adapter only after an ADR, license/maturity review, and semantic-gap matrix.
- External queue/workflow claims such as exactly-once, automatic retry, or
  durable timers never replace explicit intent/result, unknown-outcome,
  cancellation, lease, and reconciliation evidence.
- Utility implementations register through the shared program/catalog runtime
  and typed host services. Do not add program-name routing or a private utility
  plugin mechanism.

## Version-specific invariants

- Every executable plan node routes to a typed handler with real effects and
  exact return-code/abend propagation; missing handlers fail explicitly.
- Submission, selection, execution, output access, and control operations apply
  SAF; dataset allocation and locks use 0.6 authorities.
- Job, step, spool, checkpoint and output state have bounded retention and one
  durable authority. Restart cannot duplicate committed effects silently.
- Cross-resource outcomes follow the common UOW contract; 0.16 may complete the
  matrix but this version cannot hide heuristic or unknown outcomes.

## Completion gate

Do not finish until there is no batch program-name dispatch or utility summary
success; scheduling, spool, output, real utilities, SAF, DISP, return-code/abend,
restart, cancellation, overload and unknown-outcome matrices pass; durable job/
output state passes migration and backup/restore; and CardDemo batch journeys
remain exact. Under the approved policy, the licensed z/OS 3.2/JES2
differential remains explicitly pending at 0/16 and is a hard 0.17
release-certification dependency.

At handoff, include route/utility inventories, lifecycle and failure matrices,
state/migration digests, the pending differential reason and exact candidate,
and full unchanged-candidate validation. Do not claim physical JES2 sysplex,
spool implementation, Hercules, or licensed z/OS equivalence.
