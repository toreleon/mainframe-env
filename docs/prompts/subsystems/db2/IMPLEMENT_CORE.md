# Execution Prompt — Db2 — Engine and common SQL

Subsystem: **db2**
Phase: **core**

Completion dependencies: coverage.foundation, cobol.execution, racf.security

Use this prompt from the repository root. The
[common execution contract](../README.md#common-execution-contract) is normative.
Apply its [hardened slice acceptance](../README.md#hardened-slice-acceptance),
[early participant contract](../README.md#early-transaction-participant-contract),
and [licensed-harness preparation](../README.md#licensed-harness-preparation)
requirements alongside the version-specific boundaries below.

---

You are implementing **mainframe-env db2.core: generic Db2 engine and common SQL**.
Extend the current generic static-SQL provider into the declared common Db2 core
so that db2.programming can complete the pinned Db2 13 for z/OS programming surface.

## Read and verify first

Read `docs/prompts/subsystems/README.md`,
`docs/delivery/subsystems/db2/core-plan.md`, the 158-statement and SQL PL
catalogs, SQL/host/store/security contracts, and accepted coverage.foundation, cobol.execution and racf.security
evidence. Parser/catalog work may start after coverage.foundation, but host integration requires
cobol.execution and public privilege/security integration requires racf.security.

## Implement in this order

1. Freeze **DB2-1201/DB2-1202** SQL tokens, typed AST, diagnostics, catalog,
   binder, name-resolution, type/function/expression, privilege and limit schemas.
2. Implement **DB2-1203** relational IR and generic query/DML executor with
   common DDL, tables/views, constraints, indexes, null and type semantics.
3. Implement **DB2-1204** transactions, isolation core, cursors, dynamic SQL,
   COBOL host variables, SQLCA, static package/plan metadata and effects.
4. Implement **DB2-1205** bounded common SQL PL routines/control flow.
5. Implement **DB2-1206** application-data migration, hardcode removal,
   property/metamorphic/failure/recovery/compatibility and licensed differential
   suites for the accepted common subset.

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
db2.programming-deferred row/obligation sets against the pinned 174-row catalog: 158 SQL
headings plus 16 SQL PL rows. Preserve every mandatory obligation, source locator
and owner. Recognition covers the full pinned set; execution credit in db2.core is
limited to the frozen common subset. A partially implemented row remains partial;
do not choose the common subset retrospectively from passing tests. Hand every
remaining obligation to DB2-1301 without reducing the official denominator.

## Reuse and architecture guardrails

- Before DB2-1201 or DB2-1203 broad implementation, complete a checked-in
  build-versus-buy spike for `sqlparser-rs` and Apache DataFusion using 30–50
  representative pinned Db2 statements: common query/DML/DDL, host variables,
  cursors, packages, SQL PL, decimal/character types, constraints, errors, and
  negative forms. Record parse coverage, semantic gaps, bounds, MSRV/license,
  dependency cost, extension/fork requirements, and the accept/reject decision.
- Convert any third-party SQL AST immediately into an owned bounded Db2 AST.
  Third-party AST, Arrow, DataFusion plans/types/errors, or storage handles may
  not enter public contracts, semantic identity, checkpoints, durable catalogs,
  SQLCA, coverage evidence, or application packages.
- A reused parser/planner/executor may provide syntax and relational substrate
  only for rows whose Db2 semantic-gap and licensed differential matrices pass.
  Db2 types/lengths, CCSID/collation, nulls, privileges, SQLCA, packages/plans,
  cursors, SQL PL, isolation, logging, recovery, and conditions remain owned.
- Do not substitute SQLite, PostgreSQL, DuckDB, DataFusion, or another engine's
  accepted syntax, transaction result, optimizer choice, or error for Db2
  compatibility. Use the shared SQLx store only as durable infrastructure.
- Reuse the shared catalog compiler, package runtime, SAF/principal, effect/UOW,
  store/migration, dependency graph, and conformance harness. Do not create a
  Db2-private transaction coordinator or evidence system.

## Version-specific invariants

- Catalog and parse all 158 pinned SQL statement headings plus pinned SQL PL,
  while crediting execution only to rows with real generic semantics.
- Recompute the current production Db2 H1/H3 scan and keep prohibited dispatch
  at zero; the historical 29-hit audit is not a current baseline measurement.
  Application schemas, rows, packages and privileges enter through packages.
- Names, tables, statements, packages or plans never select application-specific
  branches. There is one generic catalog/binder/executor path.
- Enforce exact types, nulls, conversion, SQLCODE/SQLSTATE/SQLCA, authorization,
  constraints, cursor and transaction behavior; missing semantics fail explicitly.
- Use the common effect/UOW contract; do not create Db2-private cross-resource
  shortcuts or claim db2.programming advanced rows complete.

## Completion gate

Do not finish until all 158 headings and SQL PL constructs recognize
deterministically; the declared common DDL/DML/query/transaction rows execute and
pass conditions/recovery; production Db2 H1/H3 count is zero; common name/type/
null/constraint/cursor/static/dynamic/privilege/transaction matrices pass; and the
accepted subset passes licensed Db2 13 differentials.

At handoff, separate recognition from common-subset execution counts, provide
before/after hardcode scans, AST/catalog/state digests, migration/recovery and
oracle evidence, deferred db2.programming rows, and full unchanged-candidate validation.
