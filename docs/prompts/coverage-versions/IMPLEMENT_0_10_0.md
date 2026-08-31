# Execution Prompt — Implement mainframe-env 0.10.0

Target version: **0.10.0**  
Completion dependencies: 0.9.0

Use this prompt from the repository root. The
[common execution contract](README.md#common-execution-contract) is normative.

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

## Version-specific invariants

- Cover 269 unique SPI and 39 FEPI commands from reviewed catalogs; aliases or
  duplicate documentation rows must not distort denominators.
- All application API and system programming commands share one CICS resource
  authority and condition model. Do not create an administrative shadow state.
- Administrative changes are authorized, audited, atomic and recoverable; no
  unavailable backend or unimplemented command returns generic success.
- FEPI target/session identities are configuration/package data, never hardcoded.
- z/OSMF CICS adapters may prepare privately but cannot advertise a route until
  its accepted SPI handler exists.

## Completion gate

Do not finish until 269/269 unique SPI and 39/39 FEPI commands pass all six
applicable gates; lifecycle, topology, authorization, audit, concurrency,
quiesce, timeout, failure, recovery and resource-bound matrices pass; licensed
CICS TS 6.x SPI/FEPI differentials pass; and all 0.9 API behavior remains green.

At handoff, report canonical denominator deduplication, per-family gate counts,
registry/resource-schema digests, concurrency/recovery results, oracle receipts,
and full validation on the unchanged candidate.
