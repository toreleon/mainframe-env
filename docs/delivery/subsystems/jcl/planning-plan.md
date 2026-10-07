# JCL — Converter and planner

Subsystem: **jcl**
Phase: **planning**

Status: **Proposed**
Start gate: coverage.foundation catalog, diagnostic, source-provenance, and plan contracts frozen
Completion dependencies: coverage.foundation
Estimate: 10–16 engineer-months

## Outcome

Replace the current workload-oriented JCL path with a complete converter and
typed execution planner for the pinned z/OS 3.2 JCL and JES2 JECL inventories.

## Owned scope

- Recognize and validate all 20 pinned JCL statement forms, including nested
  procedures, in-stream data, comments, delimiter handling, and continuations.
- Cover 74 DD, 19 EXEC, 35 JOB, 76 OUTPUT parameters, and all 13 pinned JES2
  JECL statements without silently discarding operands.
- Implement symbol definition/substitution timing, procedure search and
  override rules, backward references, condition syntax, and source provenance.
- Emit typed, immutable job/step/DD/output plans with normalized diagnostics and
  capability requirements for JES execution.
- Generate parameter and statement identities from reviewed official catalogs.

## Work packages

| ID | Deliverable |
|---|---|
| JCL-701 | Complete lexer, continuation, in-stream data, and source spans |
| JCL-702 | Statement and parameter catalogs plus generated typed identities |
| JCL-703 | Procedures, INCLUDE, symbols, overrides, and search semantics |
| JCL-704 | JOB/EXEC/DD/OUTPUT validation and typed planning |
| JCL-705 | JES2 JECL recognition, validation, and planner annotations |
| JCL-706 | Differential, malformed-input, scale, and compatibility suites |

## Parallelization

Statement parsing, parameter validation, procedures/symbols, and JECL can run
as separate cohorts once diagnostic and plan schemas freeze. DD allocation
fields must share the dataset.data catalog vocabulary; changes to that vocabulary are
coordinated through its owner.

jcl.planning can run with cobol.structure, racf.security, and dataset.data. It provides a typed plan to jes.execution; it does not
need to wait for the JES runtime to be implemented.

## Exit gate

- 20/20 statements, 74/74 DD, 19/19 EXEC, 35/35 JOB, 76/76 OUTPUT parameters,
  and 13/13 JECL statements are inventoried, recognized, and validated.
- Procedure expansion, symbol timing, override precedence, conditions, source
  spans, and error recovery pass positive, negative, boundary, and differential
  matrices.
- Every accepted operand either affects the typed plan or produces an explicit
  unsupported-capability diagnostic; none is ignored.
- Existing CardDemo JCL produces a stable equivalent plan.

## Non-goals

- JES scheduling, spool lifecycle, job execution, and utility behavior, which
  belong to jes.execution.
