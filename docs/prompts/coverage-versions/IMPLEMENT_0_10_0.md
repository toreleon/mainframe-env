# Execution Prompt — Implement mainframe-env 0.10.0

Target version: **0.10.0**
Completion dependencies: 0.9.0

Use this prompt from the repository root. The
[common execution contract](README.md#common-execution-contract) is normative.
Apply its [hardened slice acceptance](README.md#hardened-slice-acceptance),
[early participant contract](README.md#early-transaction-participant-contract),
and [licensed-harness preparation](README.md#licensed-harness-preparation)
requirements alongside the version-specific boundaries below.

---

You are implementing **mainframe-env 0.10.0: complete CICS SPI and FEPI** over
the accepted 0.9 CICS application and resource authorities.

## Read and verify first

Read `docs/prompts/coverage-versions/README.md`,
`docs/delivery/coverage-versions/0.10.0.md`, the pinned/generated SPI and FEPI
catalogs, resource lifecycle/topology/condition contracts, and accepted 0.9.0
evidence. Verify the 0.9 command registry, CICS state, SAF, resource, condition,
effect, and recovery contracts before integration.

## Implement in this order

1. Freeze **SPI-1001** generated SPI/FEPI grammar, options, resource schemas,
   EIB/response/condition identities, audit effects, and exhaustive registry.
2. Implement **SPI-1002/SPI-1003** inquiry, create/install, set, discard,
   enable/disable, acquire/release, monitoring, statistics, trace, dump, scan,
   quiesce, and system-control command families.
3. Implement **SPI-1004** CSD, bundles, topology/region lifecycle, recovery
   coordination, authorized mutation, concurrency, and lock ordering.
4. Implement **SPI-1005** all FEPI pools, targets, sessions, conversations,
   data flow, timeouts, cancellation, failure, and recovery.
5. Implement **SPI-1006** authorization, audit, malformed, bounds, concurrency,
   scale, restart and licensed differential suites.

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

## Reuse and architecture guardrails

- Extend the exact 0.9 CICS command, condition, resource, registry, package,
  principal, effect/UOW, state, migration, and evidence authorities. Do not
  create a separate SPI/FEPI dispatcher, resource database, response mapper, or
  recovery engine.
- Generate SPI and FEPI identities, option/resource schemas, audit effects,
  handler closure, documentation, and coverage rows through the shared contract
  compiler. A resource family may add handlers and typed state only behind the
  common CICS authority.
- Reuse reviewed HTTP, timer, tracing, bounded-channel, and transport libraries
  at adapters. SPI lifecycle, quiesce, monitoring/statistics identity, FEPI
  sessions, conversations, timeouts, conditions, and recovery remain owned
  CICS-visible semantics.
- External topology, terminal, workflow, or messaging systems may support test
  adapters or optional providers only after a semantic-gap matrix; their names
  and success states cannot select or prove product behavior.

## Version-specific invariants

- Cover 269 unique SPI and 39 FEPI commands from reviewed catalogs; aliases or
  duplicate documentation rows must not distort denominators.
- All application API and system programming commands share one CICS resource
  authority and condition model. Do not create an administrative shadow state.
- Administrative changes are authorized and audited, with the pinned operation's
  atomicity, implicit syncpoint and recovery boundaries; no unavailable backend
  or unimplemented command returns generic success.
- FEPI target/session identities are configuration/package data, never hardcoded.
- z/OSMF CICS adapters may prepare privately but cannot advertise a route until
  its accepted SPI handler exists.

## Completion gate

Do not finish until 269/269 unique SPI and 39/39 FEPI commands pass all
applicable gates and mandatory obligations; lifecycle, topology, authorization, audit, concurrency,
quiesce, timeout, failure, recovery and resource-bound matrices pass; licensed
CICS TS 6.x SPI/FEPI differentials pass; and all 0.9 API behavior remains green.

At handoff, report canonical denominator deduplication, per-family gate counts,
registry/resource-schema digests, concurrency/recovery results, oracle receipts,
and full validation on the unchanged candidate.
