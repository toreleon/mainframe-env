# JES2 and utilities — Jobs, spool and utilities

Subsystem: **jes**
Phase: **execution**

Status: **Proposed**
Start gate: racf.security SAF, dataset.data allocation/locking, and jcl.planning typed-plan contracts frozen
Completion dependencies: racf.security, dataset.data, jcl.planning
Estimate: 14–22 engineer-months

## Outcome

Execute converted jobs through a deterministic JES2 lifecycle with real
utilities, spool, restart, authorization, and failure semantics.

## Owned scope

- Implement converter, interpreter, and execution phases; job/step state,
  return-code and abend propagation; COND/IF evaluation; and restart behavior.
- Implement job queues, classes, initiators, priority, held/released/cancelled
  states, spool allocation, output selection, routing, retention, and purge.
- Cover started tasks, internal readers, NJE/MAS abstractions, checkpoints,
  operator-visible status, and bounded scheduling behavior in the pinned scope.
- Replace program-name summaries with registered utility implementations for
  common dataset, catalog, copy, sort, generation, and diagnostic utilities.
- Apply SAF checks and dataset authorities at submission, selection, execution,
  output, and control boundaries.

## Work packages

| ID | Deliverable |
|---|---|
| JES-801 | Job/step scheduler, initiator, class, and lifecycle state machine |
| JES-802 | DD allocation, DISP, concatenation, temporary data, and cleanup |
| JES-803 | Spool, output groups, routing, retention, and purge |
| JES-804 | Started tasks, internal reader, NJE/MAS, and control operations |
| JES-805 | Registered real utility framework and utility semantic families |
| JES-806 | Restart, cancellation, overload, recovery, and IBM differentials |

## Parallelization

Scheduling, spool/output, utility families, and restart/failure injection can
run independently behind the frozen plan and state interfaces. DD lifecycle
and authorization changes are serialized with the dataset and SAF owners.

After its dependencies freeze, jes.execution can run alongside cics.application-api, db2.core, ims.programming, and
mq.programming. Cross-resource commit behavior is exercised here but finalized in integration.transactions.

## Exit gate

- All supported JCL plan nodes execute through real handlers; there is no
  program-name dispatch, summary-only success, or generic-success fallback.
- Scheduling, spool, output, utilities, SAF, DISP, return-code/abend, restart,
  cancellation, overload, and unknown-outcome matrices pass.
- Job and output state survives supported restart/backup/restore paths with
  versioned migrations.
- Existing CardDemo batch journeys retain exact observable behavior.
- Under the user-approved 2026-09-04 completion policy, the licensed z/OS
  3.2/JES2 differential remains explicit at 0/16 pending, the disposition is
  `pass-with-licensed-differential-pending`, the standalone oracle stays
  fail-closed, and the real campaign is handed to the certification.licensed
  release-certification hard gate.

## Non-goals

- Installation and operation of a physical JES2 sysplex or byte-for-byte spool
  implementation outside the pinned programming surface.
- Claiming that Hercules, MVS 3.8J, modeled behavior, or local product output is
  equivalent to licensed z/OS 3.2/JES2.
