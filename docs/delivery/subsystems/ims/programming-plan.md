# IMS — DB / TM programming surface

Subsystem: **ims**
Phase: **programming**
Target release: **0.14.0**

Status: **Proposed**
Start gate: 0.4 COBOL host ABI, 0.5 SAF, and 0.6 storage/catalog contracts frozen
Completion dependencies: cobol.execution, racf.security, dataset.data
Estimate: 20–32 engineer-months

The [common release contract](../README.md#common-release-contract) and
[hardened slice acceptance](../../../prompts/subsystems/README.md#hardened-slice-acceptance)
apply, including early participant-contract and licensed-harness preparation.
These requirements do not themselves certify implementation or waive an exit gate.

## Outcome

Implement the complete pinned 25-family IMS 15.6 programming comparison
surface with generic DBD/PSB/PCB/SSA metadata, DL/I semantics, TM, and recovery.

## Owned scope

- Generate and implement all 25 pinned call/verb families with exact PCB status,
  option, SSA, path, positioning, and condition behavior.
- Parse and validate DBD, PSB, PCB, segment, field, relationship, secondary
  index, logical relationship, and application-control metadata.
- Implement HDAM/HIDAM, HISAM/SHISAM, GSAM and pinned database organizations,
  access paths, variable-length segments, replace/delete/insert, and concurrency.
- Implement IMS TM message queues, transactions, conversational state, alternate
  PCBs, scheduling, authorization, timeout, cancellation, and restart.
- Implement checkpoint/restart, logging, recovery, reorganization/load/extract
  utilities, and application-package loading without program-name dispatch.

## Work packages

| ID | Deliverable |
|---|---|
| IMS-1401 | Generated call catalog, SSA grammar, PCB status, and diagnostics |
| IMS-1402 | DBD/PSB/PCB metadata, validation, and application packages |
| IMS-1403 | Database organizations, navigation, mutation, indexes, and paths |
| IMS-1404 | TM queues, scheduling, conversations, and alternate PCBs |
| IMS-1405 | Checkpoint/restart, logging, recovery, and utilities |
| IMS-1406 | SAF, concurrency, scale, compatibility, and IBM differentials |

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

## Parallelization

Call/SSA parsing, metadata, database organizations, TM, and recovery utilities
can run as separate cohorts after status and storage contracts freeze. DBD/PSB
catalog preparation may begin after 0.2, but integration waits for 0.4–0.6.

0.14 can run alongside 0.8–0.13 and 0.15. Cross-subsystem syncpoint behavior is
implemented against the common UOW contract and completed in 0.16.

## Exit gate

- 25/25 pinned call/verb families pass all applicable coverage gates.
- DBD/PSB/PCB/SSA, organization, path, positioning, status, TM, scheduling,
  security, concurrency, checkpoint, utility, and recovery matrices pass.
- No program, transaction, database, segment, or message identity selects a
  production semantic branch.
- Licensed IMS 15.6 differentials pass for the pinned programming surface.
- CardDemo IMS assets load from its package and retain exact observable behavior.

## Non-goals

- Whole IMS operational, installation, system-definition, or physical storage
  parity beyond the pinned programming comparison surface.
