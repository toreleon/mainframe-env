# Execution Prompt — Implement mainframe-env 0.8.0

Target version: **0.8.0**
Completion dependencies: 0.5.0, 0.6.0, 0.7.0

Use this prompt from the repository root. The
[common execution contract](README.md#common-execution-contract) is normative.

---

You are implementing **mainframe-env 0.8.0: complete JES2 execution and real
utility semantics** over accepted JCL plans, dataset authorities, and SAF.

## Read and verify first

Read `docs/prompts/coverage-versions/README.md`,
`docs/delivery/coverage-versions/0.8.0.md`, and accepted evidence/contracts from
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
output state passes migration and backup/restore; licensed z/OS 3.2/JES2
differentials pass; and CardDemo batch journeys remain exact.

At handoff, include route/utility inventories, lifecycle and failure matrices,
state/migration digests, differential receipts and full unchanged-candidate
validation. Do not claim physical JES2 sysplex or spool implementation parity.
