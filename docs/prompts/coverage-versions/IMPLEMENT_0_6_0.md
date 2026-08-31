# Execution Prompt — Implement mainframe-env 0.6.0

Target version: **0.6.0**  
Completion dependencies: 0.2.0

Use this prompt from the repository root. The
[common execution contract](README.md#common-execution-contract) is normative.

---

You are implementing **mainframe-env 0.6.0: complete dataset, VSAM, catalog,
locking, and AMS programming surface**.

## Read and verify first

Read `docs/prompts/coverage-versions/README.md`,
`docs/delivery/coverage-versions/0.6.0.md`, the pinned dataset/VSAM and 31-command
AMS inventories, store/allocation/effect contracts, durability/security ADRs,
and existing provider tests. Verify accepted 0.2 catalog, handler, package, and
store receipts before exposing new public behavior.

## Implement in this order

1. Freeze **DAT-601** catalog, allocation, DCB, SMS, volume, capability, state,
   diagnostic, and migration schemas.
2. Implement **DAT-602/DAT-603** organizations and access: KSDS, ESDS, fixed and
   variable RRDS, LDS, sequential, PDS/PDSE, GDG, alias, catalog and lifecycle.
3. Implement **DAT-604** keys/RBA/RRN/sequential access, spanned records, CI/CA,
   AIX, SHAREOPTIONS, record locks, RLS/TVS, concurrency, recovery, and lock order.
4. Implement **DAT-605** complete generated AMS grammar, modal language, and all
   31 functional command handlers over the same authorities.
5. Implement **DAT-606** differential, scale, corruption, backup/restore,
   migration, retry, restart, and unknown-outcome suites.

## Version-specific invariants

- Dataset, catalog, allocation, lock, record, index, generation, and volume
  state each have one typed authority and atomic mutation path.
- Every accepted DCB/SMS/catalog/AMS operand affects semantics or returns an
  explicit unsupported capability; silently ignored attributes are forbidden.
- Base and alternate-index mutations, GDG selection, DISP cleanup, allocation,
  and catalog changes remain consistent across every injected failure point.
- Abstract volume behavior is deterministic; physical/tape/SMS adapters declare
  capabilities honestly and cannot manufacture success.
- Application dataset names and layouts come from packages, never provider code.

## Completion gate

Do not finish until every pinned organization, access mode, DCB/SMS/catalog row
and 31/31 AMS commands pass all applicable gates; base/AIX, locks, RLS/TVS, GDG,
DISP, restart, unknown-outcome and concurrent-mutation matrices pass; licensed
z/OS 3.2 dataset/VSAM/AMS differentials pass; and CardDemo records, keys,
generations, aliases, and bytes remain exact.

At handoff, report inventory counts by gate, state/migration versions, lock and
recovery evidence, provider-capability gaps, oracle receipts, and full unchanged-
candidate validation. Do not claim whole DFSMS parity.
