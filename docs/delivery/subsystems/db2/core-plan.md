# Db2 — Engine and common SQL

Subsystem: **db2**
Phase: **core**
Target release: **0.12.0**

Status: **Proposed**
Start gate: 0.2 catalog/handler contracts, 0.4 COBOL host ABI, and 0.5 SAF frozen
Completion dependencies: coverage.foundation, cobol.execution, racf.security
Estimate: 24–36 engineer-months

The [common release contract](../README.md#common-release-contract) and
[hardened slice acceptance](../../../prompts/subsystems/README.md#hardened-slice-acceptance)
apply, including early participant-contract and licensed-harness preparation.
These requirements do not themselves certify implementation or waive an exit gate.

## Outcome

Extend the current generic static-SQL provider and complete the common SQL/SQL PL
core needed to finish the pinned Db2 13 for z/OS programming surface.

## Owned scope

- Implement a generic lexer/parser, typed AST, binder, schema catalog,
  relational IR, optimizer boundary, executor, diagnostics, and SQLCA mapping.
- Implement common query, expression, DML, DDL, transaction, cursor, dynamic
  SQL, host-variable, null, type, constraint, index, and view semantics.
- Implement static package/plan metadata and a bounded SQL PL procedural core.
- Apply table, view, routine, package, plan, schema, and administrative
  authorization through SAF-backed principals and Db2 privilege evaluation.
- Recompute the current H1/H3 scan, remove actual prohibited dispatch and retain
  the zero-hardcode ratchet; load application schemas/data through packages.

## Work packages

| ID | Deliverable |
|---|---|
| DB2-1201 | SQL lexer/parser, typed AST, official statement catalog, diagnostics |
| DB2-1202 | Schema catalog, binder, names, types, functions, and expressions |
| DB2-1203 | Relational IR, query/DML executor, constraints, indexes, and views |
| DB2-1204 | Transactions, cursors, dynamic SQL, host ABI, SQLCA, packages/plans |
| DB2-1205 | Common DDL and bounded SQL PL routine/control-flow core |
| DB2-1206 | De-hardcoding, compatibility, property, failure, and differential suites |

## Current baseline and common-subset freeze

Inspect the [current provider](../../../../crates/providers/mainframe-env-db2/README.md)
and its `Db2CatalogGeneration`, atomic package/generation selection, static SQL,
cursor, object-row persistence, replay and rollback contracts. Extend these
accepted authorities with the parser/binder/relational execution work; preserve
package/catalog upgrades, bounded readers, migration and rollback compatibility.
Do not replace the generic provider with a parallel engine or reimplement its
already-accepted baseline merely to satisfy stale roadmap prose.

The 29 CardDemo H1/H3 hits are a historical audit, not a measured post-#131 defect
count. DB2-1206 requires a fresh candidate-bound scan, remediation of actual
prohibited dispatch and a zero-prohibited-dispatch ratchet. Application schemas,
rows, packages and privileges continue to enter through versioned packages.

Before broad DB2-1203 execution, DB2-1201/DB2-1202 must freeze exact common and
0.13-deferred row/obligation sets against the pinned 174-row catalog: 158 SQL
headings plus 16 SQL PL rows. Preserve every mandatory obligation, source locator
and owner. Recognition covers the full pinned set; execution credit in 0.12 is
limited to the frozen common subset. A partially implemented row remains partial;
do not choose the common subset retrospectively from passing tests. Hand every
remaining obligation to DB2-1301 without reducing the official denominator.

## Parallelization

The parser, catalog/binder, relational executor, host ABI, and application-data
migration can run as separate cohorts after AST and diagnostic schemas freeze.
Parser/catalog preparation may begin after 0.2, while host integration waits
for 0.4 and authorization integration waits for 0.5.

0.12 can run alongside 0.8, 0.9, 0.14, and 0.15. Shared transaction behavior
must use the common effect/UOW contract rather than a Db2-private shortcut.

## Exit gate

- All 158 pinned SQL statement headings plus the pinned SQL PL constructs are
  cataloged and parse deterministically; common SQL rows execute generically.
- A current candidate-bound production Db2 H1/H3 scan reports zero prohibited
  dispatch; equivalent CardDemo behavior comes from its signed package.
- Names, types, nulls, expressions, constraints, cursors, dynamic/static SQL,
  SQLCA, privilege, commit/rollback, concurrency, and recovery matrices pass.
- No statement or table name selects an application-specific execution branch.
- The accepted common subset passes licensed Db2 13 differentials.

## Non-goals

- Completion of every advanced Db2 13 row; advanced SQL, objects, utilities,
  packages, and distributed behavior finish in 0.13.
