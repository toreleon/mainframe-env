# Execution Prompt — IMS — DB / TM programming surface

Subsystem: **ims**
Phase: **programming**

Target version: **0.14.0**
Completion dependencies: cobol.execution, racf.security, dataset.data

Use this prompt from the repository root. The
[common execution contract](../README.md#common-execution-contract) is normative.
Apply its [hardened slice acceptance](../README.md#hardened-slice-acceptance),
[early participant contract](../README.md#early-transaction-participant-contract),
and [licensed-harness preparation](../README.md#licensed-harness-preparation)
requirements alongside the version-specific boundaries below.

---

You are implementing **mainframe-env 0.14.0: complete IMS 15.6 programming
surface** for the 25 pinned DL/I call/verb families, generic metadata, TM, and
recovery.

## Read and verify first

Read `docs/prompts/subsystems/README.md`,
`docs/delivery/subsystems/ims/programming-plan.md`, the pinned IMS comparison catalog,
IMS/provider/store/host/security contracts, and accepted 0.4.0, 0.5.0 and 0.6.0
evidence. DBD/PSB/SSA catalog preparation may start after 0.2, but public host,
security and storage integration requires all listed dependencies.

## Implement in this order

1. Freeze **IMS-1401/IMS-1402** call identities, SSA grammar, PCB status,
   diagnostics, DBD/PSB/PCB/segment/field/relationship metadata, package and
   migration schemas.
2. Implement **IMS-1403** pinned database organizations, navigation/path/
   positioning, variable segments, secondary indexes/logical relationships and
   insert/replace/delete with exact concurrency.
3. Implement **IMS-1404** TM queues, transactions, scheduling, conversational
   state, alternate PCBs, authorization, timeout and cancellation.
4. Implement **IMS-1405** checkpoints, logging, restart/recovery and pinned load,
   extract, reorganization and recovery utilities.
5. Implement **IMS-1406** SAF, malformed/boundary/limit, concurrency, scale,
   compatibility and licensed differential suites.

## IMS context and positioning matrix

IMS-1401/IMS-1402 must freeze applicability across the 25 call families,
execution environments, pinned database organizations, PCB kinds/options and
SSA forms. Use reviewed equivalence classes, mandatory boundaries and bounded
pairwise combinations instead of an exhaustive Cartesian product.

IMS-1403 proves exact PCB status and retained position after both successful
and unsuccessful calls, including GN/GNP navigation, path/qualified SSAs,
Get Hold and update restrictions. IMS-1404 adds alternate PCBs, TM scheduling
and conversations; IMS-1405 adds checkpoint/restart position and status. A test
that returns the expected segment but leaves the wrong position does not pass.
Bind forbidden-context/no-mutation cases to the same reviewed matrix.

Shared storage/locking/UOW authorities do not prohibit IMS-owned hierarchy,
index or positioning algorithms. They prohibit competing stores, lock services,
coordinators and migration authorities. Each mutating database/TM/utility slice
includes minimum failure, replay and restart proof; IMS-1405/IMS-1406 complete
and stress recovery rather than first introducing it.

## Reuse and architecture guardrails

- Build IMS database organizations, indexes, paths, locking, logging, migration,
  and backup/restore on the accepted 0.6 dataset/storage primitives and common
  effect/UOW protocol. Do not create an IMS-private page manager, object store,
  lock service, transaction coordinator, or migration runner.
- Generate call/status identities, SSA grammar metadata, DBD/PSB/PCB schemas,
  handler closure, documentation, and coverage rows through the shared contract
  compiler and package runtime. Application metadata cannot become generated
  production branches.
- A graph or ordered-key/value library may provide internal algorithms or
  physical storage only after a semantic-gap matrix. IMS hierarchy, positioning,
  path calls, logical relationships, PCB status, concurrency, and recovery
  remain owned observable semantics.
- TM queues, conversations, scheduling, timeout, cancellation, principal, and
  checkpoint behavior reuse the common runtime contracts. An external broker or
  workflow engine cannot define IMS delivery, syncpoint, or restart results.

## Version-specific invariants

- All 25 families share generated call/status identities and one generic
  DBD/PSB/PCB/SSA authority; application program/database/segment names never
  select semantic branches.
- Preserve exact PCB status, SSA qualification, path and position across normal,
  not-found, duplicate, invalid, cancellation, checkpoint and restart behavior.
- Database and TM state use typed 0.6 storage/locking and 0.5 principals; provider
  code cannot bypass either authority.
- Checkpoint/restart and cross-resource syncpoints report heuristic/unknown
  outcomes honestly; final mixed-resource closure belongs to 0.16.
- Utilities perform real bounded state transitions, not summaries.

## Completion gate

Do not finish until 25/25 pinned call/verb families pass all applicable gates;
DBD/PSB/PCB/SSA, organizations, paths/positioning, status, TM/scheduling,
security, concurrency, checkpoint, utility and recovery matrices pass; licensed
IMS 15.6 differentials pass; and CardDemo IMS assets load from packages with
exact observable behavior and zero production name dispatch.

At handoff, report per-call/per-gate counts, metadata and state digests,
checkpoint/migration/recovery evidence, oracle receipts, explicit exclusions and
full validation on the unchanged candidate.
