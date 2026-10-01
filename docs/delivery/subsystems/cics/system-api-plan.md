# CICS — SPI and FEPI

Subsystem: **cics**
Phase: **system-api**
Target release: **0.10.0**

Status: **Proposed**
Start gate: 0.9 resource, condition, handler, and CICS state contracts frozen
Completion dependencies: cics.application-api
Estimate: 18–30 engineer-months

The [common release contract](../README.md#common-release-contract) and
[hardened slice acceptance](../../../prompts/subsystems/README.md#hardened-slice-acceptance)
apply, including early participant-contract and licensed-harness preparation.
These requirements do not themselves certify implementation or waive an exit gate.

## Outcome

Complete the pinned CICS system programming and FEPI surfaces: 269 unique SPI
commands and 39 FEPI commands over the same typed CICS authorities as 0.9.

## Owned scope

- Generate and implement 269 unique SPI commands with exact option constraints,
  resource schemas, response/condition mappings, and audit effects.
- Cover inquiry, create/install, set, discard, enable/disable, acquire/release,
  quiesce, scan, dump, trace, monitoring, statistics, and system-control families.
- Model CSD/bundle-defined resource lifecycle, topology and region operations,
  recovery coordination, and authorized administrative mutation.
- Implement all 39 FEPI commands, terminal pools, sessions, conversations,
  targets, data flow, timeouts, and failure recovery.

## Work packages

| ID | Deliverable |
|---|---|
| SPI-1001 | Generated SPI/FEPI grammars, options, conditions, and registry |
| SPI-1002 | Resource inquiry, lifecycle, set, and administrative families |
| SPI-1003 | Monitoring, statistics, trace, dump, and system-control families |
| SPI-1004 | CSD, bundles, topology, quiesce, and recovery operations |
| SPI-1005 | Complete FEPI pools, sessions, conversations, and targets |
| SPI-1006 | Authorization, concurrency, failure, scale, and IBM differentials |

## Lifecycle and FEPI slice acceptance

SPI-1001 must bind each operation to resource-state preconditions, authorized
intent, concurrent-reader/writer observations, lock order, implicit syncpoints,
quiesce/drain behavior and restart outcome. SPI-1002–SPI-1004 prove the exact
operation-specific atomicity and recoverability boundary; do not imply that
every administrative mutation is reversible in the caller's transaction.

Declare bounded SPI-1005 pool/target, session, conversation/data-flow and
failure/recovery slices in the existing status. Keep FEPI conditions, timeout,
cancellation and retained state explicit at each transition. Every mutating SPI
or FEPI slice carries its own hardened acceptance tests before integration;
SPI-1006 completes cross-family and licensed campaigns rather than introducing
those guarantees. All 269 SPI and 39 FEPI commands retain their mandatory
obligations under the shared source-backed gate-applicability rule.

## Parallelization

SPI resource-family cohorts and FEPI can run independently after the resource
and condition schemas freeze. Commands that mutate shared region state must use
one lifecycle authority and one lock order.

0.10 can run alongside 0.13, 0.14, and 0.15. z/OSMF CICS route adapters may be
prepared concurrently, but their completion waits for the accepted handlers.

## Exit gate

- 269/269 unique SPI and 39/39 FEPI commands pass all applicable gates
  and mandatory obligations in the reviewed lifecycle/context matrix.
- Lifecycle, topology, authorization, audit, concurrent mutation, quiesce,
  failure, recovery, timeout, and resource-bound matrices pass.
- No administrative route, resource type, or FEPI target is selected through
  application-specific string dispatch.
- Licensed CICS TS 6.x SPI and FEPI differentials pass.
- The 0.9 application API remains unchanged and green.

## Non-goals

- Full emulation of IBM internal region implementation details that are not
  observable through the pinned programming interfaces.
