# Datasets / VSAM / AMS — Dataset services progress

Subsystem: **dataset**
Phase: **data**
Target release: **0.6.0**

Status: **DAT-601 through DAT-606 complete — pass-with-licensed-differential-pending**

The isolated implementation branch `impl/0.6.0` starts from candidate
`7d50310381a44878c23c51c38aed841e3a70347f`. The accepted 0.2.0 catalog,
handler-identity, package-generation, store, source-provenance, and coverage
receipts remain frozen. The shared Conformance IR v1 foundation is available;
this workstream will add dataset/VSAM/AMS specifications and executable
bindings without rewriting 0.2 evidence.

## Work packages

| Work package | State | Next boundary |
|---|---|---|
| DAT-601 | pass | Typed public/durable schemas, generated surface, explicit capabilities, and backward-readable state foundation are complete |
| DAT-602 | pass | Five VSAM organizations plus key, RBA, RRN, and positioned sequential access pass typed, negative, restart, and Conformance IR gates |
| DAT-603 | pass | PDS/PDSE generations and aliases, catalog routing/search, GDG lifecycle, MEDS4, and shared dependency graph pass |
| DAT-604 | pass | CI/CA and spanned geometry, mixed atomic state mutation, SHAREOPTIONS, owner-scoped RLS locks, KSDS TVS, lock order, restart, and unknown-outcome reconciliation pass |
| DAT-605 | pass | Generated 31-command grammar, modal MAXCC/LASTCC execution, typed handlers, explicit capability conditions, authorization, AIX build, and snapshot flows pass |
| DAT-606 | pass | Independent reference simulation plus local failure, corruption, backup/restore, migration, scale, retry, restart, unknown-outcome, and concurrency gates pass; licensed campaign is handed to the 0.17 hard gate at 0/36 pending |

## Pinned denominator and dependencies

- Dataset/VSAM/AMS baseline:
  `ibm-zos-3.2-dfsms-ams-2026-06`.
- Immutable official denominator: 31 AMS functional commands and five primary
  VSAM organizations (36 mandatory rows).
- Accepted 0.2.0 dependency status: complete implementation candidate, with all
  CV-201 through CV-209 receipts present and no recorded blocker.
- Licensed IBM differential evidence is not inferred from local or simulated
  behavior. It remains 0/36 pending until the pinned z/OS 3.2 campaign runs in
  the 0.17 release-certify gate.

## Decisions and risks

- Dataset, catalog, allocation, locking, RLS/TVS, and recovery semantics stay in
  the provider-owned authority. SQLx/provider-state transactions and the
  accepted artifact-store port remain replaceable persistence adapters.
- AMS will invoke typed dataset operations and the shared principal/effect
  path; it will not own a second catalog or persistence model.
- Every accepted operand must either affect the typed semantic state or fail
  with an explicit unsupported capability.
- The 0.2 official catalog has no separate DCB/SMS/catalog-row denominator.
  DAT-601 must freeze the dossier's detailed programming inventory as typed
  obligations beneath the immutable official rows, without inventing official
  coverage rows or changing their identities.

Release-certification handoff: the required licensed z/OS 3.2 environment and
reviewed 36-case receipt are unavailable in this worktree. Under the approved
2026-09-01 policy this is not a 0.6 implementation blocker, but it remains a
strict 0.17/1.0 blocker.

DAT-601 focused validation passes `cargo xtask dataset-contract --check`,
`cargo xtask schemas --check`, `cargo xtask spec --check`, targeted Clippy with
warnings denied, and the affected host, dataset, and server tests. The generated
inventory maps all 31 command rows and all five VSAM organization rows without
claiming behavioral gate credit. Dataset request/result contracts are version 2;
the current provider writer is MEDS6 and readers retain MEDS1 through MEDS6
compatibility through non-destructive versioned migrations.

DAT-602 adds exact LDS byte-range state, ESDS record-start RBA access and
length-preserving rewrite, fixed/variable RRDS RRN access, and explicit forward
or reverse sequential positioning with organization-specific identities. The
shared Conformance IR now binds the five official organization rows to ten
mandatory obligations and 25 cases. Recognized, validated, executed,
conditioned, and recovered are 5/5; differential remains 0/5 pending the pinned
licensed oracle. Focused/replay conformance, warnings-denied Clippy, the full
batch/conformance unit suites, and affected provider/server tests pass.

DAT-603 adds durable PDSE member generations, program-object identity and member
aliases without a duplicate selected-member projection. Master/user catalogs,
connection state, explicit and qualifier aliases, exact search order, PDS/PDSE
and GDG lifecycle, `SCRATCH`/`NOSCRATCH`, and `EMPTY`/`NOEMPTY` are exercised
across restart. One bounded dependency graph now owns catalog aliases,
catalog hierarchy, dataset/catalog placement, PDSE aliases, GDGs, and AIX/PATH
ordering. The normative surface is 131 rows and state writer/readers are
MEDS6/MEDS1-6.

DAT-604 adds deterministic CI/CA occupancy and spanned-record fragmentation;
mixed atomic put/delete/move transactions in memory, SQLite, and PostgreSQL
adapters; durable dataset/record locks with explicit logical leases and lexical
lock order; SHAREOPTIONS `(1,3)` and `(2,3)` behavior; owner-scoped RLS mutation;
and atomic KSDS TVS UOW commit/rollback. Restart reloads lock and UOW state.
Injected pre-commit failure produces a durable `Unknown` UOW that must be
explicitly reconciled, and replay returns the exact terminal receipt. Focused
host, store, dataset, and server suites pass; the PostgreSQL 18 integration test
remains environment-gated and the licensed differential remains pending.

DAT-605 adds a schema-validated grammar inventory in the exact frozen 31-command
order and a generated Rust registry. The bounded parser validates the full
control stream before effects and implements `IF`/`THEN` plus `SET` over MAXCC
and LASTCC. All 31 IDs dispatch: required commands use typed dataset/catalog/
AIX/lifecycle/snapshot operations, while six physical or adapter-dependent
forms return exact capability conditions and IDCAMS return code 12. Dataset
operands pass shared authorization. `BLDINDEX` is now a durable AIX transition;
`UPGRADE` and materialized `NOUPGRADE` state persist through `MEAIX4`.
The shared Conformance IR projection now claims all 36 official rows with 41
obligations and 180 local bindings. AMS recognized, validated, executed,
conditioned, and recovered are 31/31 through real IDCAMS job execution,
terminal-condition checks, JES/dataset reopen, and durable typed-effect checks;
the five organization rows retain their 25 prior bindings. Differential AMS
evidence remains 0/31 pending the licensed oracle.

The separate detailed-surface audit is inventory-digest-bound and closes all
131 descriptor identities exactly once: 97 required descriptors have executable
pass evidence, nine capability-gated descriptors are implemented and tested,
and 25 unavailable physical/adapter descriptors have exact conditioned evidence.
Its referenced Rust tests are resolved mechanically by `dataset-contract` and
run by the unchanged-candidate workspace suite.

DAT-606 local certification passes corruption rejection for all eight dataset
authority namespaces, SQLite integrity-checked backup/restore, MEDS1-6 migration,
64-object bounded scale/restart, injected base/AIX and GDG retry, TVS unknown
outcome reconciliation, concurrent optimistic-mutation and lock races, DISP,
and CardDemo exactness. `cargo test --workspace --all-targets`, warnings-denied
workspace Clippy, schemas, generated contracts, the 36-row spec projection,
180 focused bindings, and the accepted full-regression check all pass on the
same worktree. The local evidence is recorded at
`conformance/0.6/evidence/dataset-certification.json`.

DAT-606 also runs a bounded, table-driven, pure-state reference model that owns
its organization, record, catalog, AIX, GDG, alias, snapshot, register,
condition, and transition types. It imports no production dataset, AMS, host,
or provider-store authority. The model covers all five organizations and all 31
command identities, 13 deterministic property/metamorphic cases, ten explicit
physical/installation/unknown boundaries, and eight named observation perturbations (comparator checks, not implementation mutants). Focused
dataset conformance executes it before product bindings. Its differential
credit is structurally fixed at zero.

The detailed-surface completion audit additionally enables deterministic
secondary/CONTIG/ROUND/RLSE extent behavior, BUFNO/BUFSIZE reservation, SMS
placement and aggregate guaranteed-space admission, extended RBA bounds,
abstract volume labels/unit count, a global typed VTOC with volume-relative
extent placement, generic key-prefix reads, and owner/date/retention protection
across every mutation. Close and delete atomically release eligible locks;
active TVS blocks destructive changes, completed UOW audit survives deletion,
and rename atomically retargets AIX, PATH, PDSE-member, and catalog-alias
dependents. AMS allocation and ALTER populate DCB, access, allocation, SMS,
volume, and catalog fields instead of ignoring them; unknown/inapplicable
operands fail explicitly. Snapshots verify a length-delimited full-organization
manifest before import, AIX `NOUPGRADE` remains materialized until `BLDINDEX`,
and state writer/readers are MEDS6/MEDS1-6 with a reviewed non-destructive
5-to-6 migration.

`cargo xtask dataset-oracle --check` remains intentionally fail-closed until
`MAINFRAME_ENV_ZOS_AMS_ORACLE_RECEIPT` names a reviewed 36-case receipt from the
pinned licensed z/OS 3.2 environment. No such receipt or oracle configuration
is present, so differential remains exactly 0/36. The approved 0.6 disposition
is `pass-with-licensed-differential-pending`; the real campaign is mandatory at
0.17 and cannot be satisfied by this simulation.

## Controller review 1 repair

The first controller review is closed by one focused repair. Principal owner,
RLS-lock, and TVS-unit authorization now runs while the same state guard remains
held through replay admission and mutation; durable compare-and-swap still
invalidates an operation if another service instance changes the checked row.
Public-provider regressions cover concurrent unowned claim/write ordering,
owner transitions, stale RLS lock ownership, staged TVS ownership changes,
exact denial, replay-record consistency, and durable reopen.

Seed forward selection and rollback now rebuild materialized identities for
every affected `UPGRADE` AIX and PATH while leaving `NOUPGRADE` identities
materialized until explicit rebuild. Stable replacement nodes retain incoming
alias/AIX/PATH dependencies and deterministically reset their outgoing catalog
edge from the replacement definition. Public tests prove forward lookup,
rollback lookup, alias resolution, delete protection, and reopen from the exact
memory store at both selected generations.

Compatibility `Create`, GDG generation, and seed-install paths now construct and
validate the complete provider definition before graph, replay, or dataset
persistence. Invalid KSDS, ESDS, LDS, RRDS, VRRDS, and zero-CCSID requests leave
no dataset or replay row and cannot poison restart. Definition digest version 3
adds catalog creation date while retaining exact version-2 bytes when no date
was present; reviewed legacy date-bearing replays are accepted only when the
durable definition and result version prove the same request. Differential
credit remains exactly `0/36 pending`.

### Evidence terminology correction (hardening #53)

The original `mutants_killed: 8` value described observation-comparator tests,
not execution of changed transition implementations. Current reports and the
local certification fixture use `observation_perturbations_rejected` and
`mainframe-env.dataset-certification@2`. This corrects the classification of
existing evidence; it does not retroactively claim a behavioral mutation campaign.
Published artifacts and historical commits are unchanged. New source-mutation
receipts are candidate-bound and separate; see [the hardening note](../../hardening/53-dataset-mutations.md).
