# Execution Prompt — Implement mainframe-env 0.13.0

Target version: **0.13.0**
Completion dependencies: 0.12.0

Use this prompt from the repository root. The
[common execution contract](README.md#common-execution-contract) is normative.

---

You are implementing **mainframe-env 0.13.0: complete Db2 13 programming
surface** over the accepted generic 0.12 engine.

## Read and verify first

Read `docs/prompts/coverage-versions/README.md`,
`docs/delivery/coverage-versions/0.13.0.md`, the complete pinned Db2 catalogs and
accepted 0.12 parser/binder/catalog/IR/executor/transaction evidence. Verify no
application-shaped authority remains and that 0.12 artifact/state contracts are
frozen before adding advanced semantics.

## Implement in this order

1. Implement **DB2-1301** every remaining SQL/SQL PL syntax, binder rule,
   diagnostic, condition and execution handler.
2. Implement **DB2-1302** advanced query, recursive/analytic, temporal,
   XML/LOB/array, sequence/generated/identity and advanced object semantics.
3. Implement **DB2-1303** routines, triggers, transition data, modules, global
   variables, dependencies and atomic invalidation.
4. Implement **DB2-1304/DB2-1305** packages/plans and bind lifecycle, privilege
   chains, isolation, locks/deadlocks, savepoints, logging, restart and recovery.
5. Implement **DB2-1306** pinned distributed behavior, scale, malformed/limit,
   concurrency, compatibility and complete licensed differential suites.

## Reuse and architecture guardrails

- Continue the accepted and pinned 0.12 parser/planner/executor decision. Do not
  introduce a second SQL parser, AST, relational IR, optimizer, expression
  runtime, catalog, transaction authority, or compatibility path for advanced
  statements.
- Extend any accepted external SQL substrate only through owned Db2 adapters and
  a new semantic-gap/differential row for each advanced family. Unsupported
  third-party syntax or execution remains explicit and cannot be counted from
  parser acceptance or generic engine success.
- Reuse one owned dependency/cycle/invalidation graph for routines, triggers,
  modules, views, packages/plans, aliases, privileges, and schema objects. A
  graph library may implement algorithms internally but cannot define durable
  identifiers, ordering, or error semantics.
- Temporal, XML, LOB, array, analytic, isolation, lock, log, recovery, and
  distributed behavior remain Db2-owned semantics over the shared UOW,
  principal, store/migration, artifact, and evidence authorities.

## Version-specific invariants

- All remaining rows extend the single 0.12 catalog/binder/relational execution
  architecture; no statement-family side engine or private catalog is allowed.
- Catalog/object/dependency changes, triggers, routines, privilege changes and
  package invalidation are atomic and preserve exact rollback/restart behavior.
- Locking and isolation have documented ordering, deadlock detection, timeout,
  cancellation, retry and unknown-outcome semantics.
- Advanced values and result sets are bounded; unsupported physical/operational
  Db2 internals remain explicit non-goals rather than simulated successes.
- Every 0.12 common row and CardDemo database journey stays exact.

## Completion gate

Do not finish until all mandatory rows among the 158 pinned SQL headings and
pinned SQL PL portfolio pass every applicable gate; advanced types/objects,
routines/triggers, packages/plans, privileges, isolation/deadlock, logging,
restart, distributed and scale matrices pass; catalog/invalidation/rollback is
atomic; and the full pinned surface passes licensed Db2 13 differentials.

At handoff, report every remaining row closed by gate, catalog/state/migration
digests, concurrency/recovery results, oracle receipts, regression results and
full validation on one unchanged candidate. Do not claim whole Db2 operations or
physical optimizer/storage identity.
