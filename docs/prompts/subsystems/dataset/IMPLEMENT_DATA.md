# Execution Prompt — Datasets / VSAM / AMS — Dataset services

Subsystem: **dataset**
Phase: **data**

Target version: **0.6.0**
Completion dependencies: coverage.foundation

Use this prompt from the repository root. The
[common execution contract](../README.md#common-execution-contract) is normative.

---

You are implementing **mainframe-env 0.6.0: complete dataset, VSAM, catalog,
locking, and AMS programming surface**.

## Read and verify first

Read `docs/prompts/subsystems/README.md`,
`docs/delivery/subsystems/dataset/data-plan.md`, the pinned dataset/VSAM and 31-command
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

## Approved 2026-09-01 completion policy

For this development cycle, a licensed z/OS 3.2 dataset/VSAM/AMS receipt is
unavailable. The user-approved completion disposition is
`pass-with-licensed-differential-pending`:

- preserve the exact differential numerator as 0/36 and keep
  `cargo xtask dataset-oracle --check` fail-closed;
- never treat modeled, simulated, documentation-derived, historical, or
  current-product output as licensed differential evidence;
- require an independent bounded pure-state reference simulation for the five
  organization rows, all 31 AMS identities, applicable state/recovery
  properties, explicit capability/unknown boundaries, and representative
  mutants; and
- defer the real licensed 36-row campaign to the 0.17 `release-certify` hard
  gate, which remains mandatory before 1.0 certification.

## Reuse and architecture guardrails

- Dataset, VSAM, catalog, allocation, lock, RLS/TVS, and recovery semantics
  remain provider-owned. Physical persistence is a replaceable adapter: reuse
  SQLx transactions and the accepted artifact/object-store port rather than
  building a page manager, WAL, cloud client, or filesystem protocol into the
  semantic core.
- Reuse one shared dependency/cycle graph utility for catalog aliases, GDGs,
  base/AIX relations, migration ordering, and invalidation, while preserving
  owned deterministic identifiers and ordering at the contract boundary.
- AMS extends the shared catalog compiler, diagnostics, registry, principal,
  effect/UOW, migration, and evidence authorities. It must not introduce a
  second catalog database or bypass typed dataset operations.
- Any external ordered-key/value or storage engine requires a semantic-gap and
  crash/recovery matrix. Its transactions, locks, snapshots, or success codes
  are not evidence of VSAM compatibility by themselves.

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
and 31/31 AMS commands pass recognized, validated, executed, conditioned, and
recovered; base/AIX, locks, RLS/TVS, GDG, DISP, restart, unknown-outcome and
concurrent-mutation matrices pass; the approved independent reference
simulation and mutants pass; and CardDemo records, keys, generations, aliases,
and bytes remain exact. The licensed differential remains pending at exactly
0/36 under the approved policy and is a hard 0.17 release-certify dependency.

At handoff, report inventory counts by gate, state/migration versions, lock and
recovery evidence, provider-capability gaps, oracle receipts, and full unchanged-
candidate validation. Report the 0/36 pending differential explicitly and do
not claim whole DFSMS parity.
