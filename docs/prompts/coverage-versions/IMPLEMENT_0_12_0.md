# Execution Prompt — Implement mainframe-env 0.12.0

Target version: **0.12.0**
Completion dependencies: 0.2.0, 0.4.0, 0.5.0

Use this prompt from the repository root. The
[common execution contract](README.md#common-execution-contract) is normative.

---

You are implementing **mainframe-env 0.12.0: generic Db2 engine and common SQL**.
Replace CardDemo-shaped database logic with a generic architecture that can
complete the pinned Db2 13 surface in 0.13.

## Read and verify first

Read `docs/prompts/coverage-versions/README.md`,
`docs/delivery/coverage-versions/0.12.0.md`, the 158-statement and SQL PL
catalogs, SQL/host/store/security contracts, and accepted 0.2.0, 0.4.0 and 0.5.0
evidence. Parser/catalog work may start after 0.2, but host integration requires
0.4 and public privilege/security integration requires 0.5.

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

## Version-specific invariants

- Catalog and parse all 158 pinned SQL statement headings plus pinned SQL PL,
  while crediting execution only to rows with real generic semantics.
- Eliminate all 29 audited production Db2 H1/H3 hits. Application schemas, rows,
  packages and privileges enter only through versioned application packages.
- Names, tables, statements, packages or plans never select application-specific
  branches. There is one generic catalog/binder/executor path.
- Enforce exact types, nulls, conversion, SQLCODE/SQLSTATE/SQLCA, authorization,
  constraints, cursor and transaction behavior; missing semantics fail explicitly.
- Use the common effect/UOW contract; do not create Db2-private cross-resource
  shortcuts or claim 0.13 advanced rows complete.

## Completion gate

Do not finish until all 158 headings and SQL PL constructs recognize
deterministically; the declared common DDL/DML/query/transaction rows execute and
pass conditions/recovery; production Db2 H1/H3 count is zero; common name/type/
null/constraint/cursor/static/dynamic/privilege/transaction matrices pass; and the
accepted subset passes licensed Db2 13 differentials.

At handoff, separate recognition from common-subset execution counts, provide
before/after hardcode scans, AST/catalog/state digests, migration/recovery and
oracle evidence, deferred 0.13 rows, and full unchanged-candidate validation.
