# Execution Prompt — Db2 — Complete programming surface

Subsystem: **db2**
Phase: **programming**

Completion dependencies: db2.core

Use this prompt from the repository root. The
[common execution contract](../README.md#common-execution-contract) is normative.
Apply its [hardened slice acceptance](../README.md#hardened-slice-acceptance),
[early participant contract](../README.md#early-transaction-participant-contract),
and [licensed-harness preparation](../README.md#licensed-harness-preparation)
requirements alongside the version-specific boundaries below.

---

You are implementing **mainframe-env db2.programming: complete Db2 13 programming
surface** over the accepted generic db2.core engine.

## Read and verify first

Read `docs/prompts/subsystems/README.md`,
`docs/delivery/subsystems/db2/programming-plan.md`, the complete pinned Db2 catalogs and
accepted db2.core parser/binder/catalog/IR/executor/transaction evidence. Verify no
application-shaped authority remains and that db2.core artifact/state contracts are
frozen before adding advanced semantics.

## Implement in this order

1. Implement **DB2-1301** every remaining SQL/SQL PL syntax, binder rule,
   diagnostic, condition and execution handler.
2. Implement **DB2-1302** advanced query, recursive/analytic, temporal,
   XML/LOB/array, sequence/generated/identity and advanced object semantics.
3. Implement **DB2-1303** routines, triggers, transition data, global
   variables, dependencies and atomic invalidation.
4. Implement **DB2-1304/DB2-1305** packages/plans and bind lifecycle, privilege
   chains, isolation, locks/deadlocks, savepoints, logging, restart and recovery.
5. Implement **DB2-1306** pinned distributed behavior, scale, malformed/limit,
   concurrency, compatibility and complete licensed differential suites.

## Pinned platform and advanced-obligation closure

DB2-1301 consumes the frozen db2.core common/deferred obligation map and closes the
remaining obligations in the same 174-row catalog (158 SQL headings and 16 SQL
PL rows). Any correction requires reviewed source provenance; do not import Db2
LUW features merely because they share a product name. SQL module objects are
not a delivery requirement without an exact pinned Db2 for z/OS source row and
reviewed scope decision. They are distinct from database request modules (DBRMs)
and must not be confused with static package/bind metadata.

Split DB2-1302–DB2-1306 acceptance by advanced value representation and bounds,
object lifecycle, privilege/package behavior, isolation, dependency invalidation
and distributed outcome. Prove concurrent catalog changes, trigger/routine
failure, invalidation, rollback and restart at the owned transaction boundary.
XML/LOB/array/temporal support is not one happy-path feature flag. Preserve the
single accepted catalog, dependency graph, owned AST/IR and execution authority;
the existing external-substrate semantic-gap rules continue to apply.

## Reuse and architecture guardrails

- Continue the accepted and pinned db2.core parser/planner/executor decision. Do not
  introduce a second SQL parser, AST, relational IR, optimizer, expression
  runtime, catalog, transaction authority, or compatibility path for advanced
  statements.
- Extend any accepted external SQL substrate only through owned Db2 adapters and
  a new semantic-gap/differential row for each advanced family. Unsupported
  third-party syntax or execution remains explicit and cannot be counted from
  parser acceptance or generic engine success.
- Reuse one owned dependency/cycle/invalidation graph for routines, triggers,
  views, packages/plans, aliases, privileges, and schema objects. A
  graph library may implement algorithms internally but cannot define durable
  identifiers, ordering, or error semantics.
- Temporal, XML, LOB, array, analytic, isolation, lock, log, recovery, and
  distributed behavior remain Db2-owned semantics over the shared UOW,
  principal, store/migration, artifact, and evidence authorities.

## Version-specific invariants

- All remaining rows extend the single db2.core catalog/binder/relational execution
  architecture; no statement-family side engine or private catalog is allowed.
- Catalog/object/dependency changes, triggers, routines, privilege changes and
  package invalidation are atomic and preserve exact rollback/restart behavior.
- Locking and isolation have documented ordering, deadlock detection, timeout,
  cancellation, retry and unknown-outcome semantics.
- Advanced values and result sets are bounded; unsupported physical/operational
  Db2 internals remain explicit non-goals rather than simulated successes.
- Every db2.core common row and CardDemo database journey stays exact.

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
