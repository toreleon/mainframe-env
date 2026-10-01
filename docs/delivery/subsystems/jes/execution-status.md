# JES2 and utilities — Jobs, spool and utilities progress

Subsystem: **jes**
Phase: **execution**
Target release: **0.8.0**

Status: **JES-801 through JES-806 complete — pass-with-licensed-differential-pending**

The isolated implementation branch `impl/0.8.0` starts from integrated
candidate `2926c54ac79f38d2689065a0f341eefc1ea82f09`. The official dependency tags
resolve to release commits `bd5e8ecd211b7da4f3e18dfcfc807352d0ebd2e8`
for 0.5.0, `ca3c061adaa63af71eefe8ee494b7c523c3e5540` for 0.6.0, and
`37247f51f6af5818f08c6072d49317ec13000327` for 0.7.0. Their accepted
authorities are present on the candidate: the shared SAF/principal database and
decision contracts, MEDS6 dataset/allocation/locking state with MEDS1-6
readers, and immutable JCL plan contract generated from baseline
`ibm-zos-3.2-jcl-jes2-2026-06`.

The starting batch baseline passed all 91 package tests. Initial discovery also
established the implementation gap: `service.rs` selected common behavior by
program name, and IEBCOPY, IEBDG, IEBEDIT, and IEBUPDTE contained summary-only
success paths.

## Work packages

| Work package | State | Next boundary |
|---|---|---|
| JES-801 | pass | Versioned JES contracts, class/initiator scheduling, durable lifecycle, per-step states, exact return-code/abend propagation, migration reader and recovery are complete |
| JES-802 | pass | Typed DD sources, allocation locks, complete DISP positions, ordered concatenation, MOD append, durable GDG/temp resolution, and partial/job cleanup are complete |
| JES-803 | pass | Artifact-backed typed spool provider, durable descriptors/output groups, JCL routing, retention, SAF controls, migration and intent-first purge are complete |
| JES-804 | pass | Durable started-task/internal-reader origins, bounded NJE/MAS topology and ownership, scheduler controls, job routing and SAF-checked control operations are complete |
| JES-805 | pass | Admission-pinned typed registrations replace execution-time name dispatch; all nine utilities have real bounded effects and unsupported controls fail explicitly |
| JES-806 | pass-with-licensed-differential-pending | Local restart, cancellation, overload, crash, backup/restore and migration matrices pass; authentic z/OS 3.2/JES2 campaign is handed to the 0.17 hard gate at 0/16 pending |

## Frozen dependency identities

- Security baseline: `ibm-zos-3.2-racf-saf-2026`; security database writer v2,
  shared SAF decision path, and 48 recognized/validated/executed/conditioned/
  recovered rows. Licensed differential remains explicitly pending under the
  accepted 0.17 certification policy.
- Dataset baseline: `ibm-zos-3.2-dfsms-ams-2026-06`; dataset request/result v2,
  MEDS6 writer with MEDS1-6 readers, durable allocation/lock/UOW authorities,
  and 36 locally complete rows. Licensed differential remains explicitly
  pending under the accepted 0.17 certification policy.
- JCL/JES2 baseline: `ibm-zos-3.2-jcl-jes2-2026-06`; immutable plan schema
  digest `sha256:4c7c382dc06c33ee7622c5108a85fe96cf5f0a9f139e2827c0c2e966b47d57a5`,
  catalog digest `sha256:1066a5b3dc378d37deecfdb1f5c67fb194e4d026d00973816a62dbddfea262d4`,
  and shared Conformance IR v1. All 237 rows are recognized and validated;
  runtime gates intentionally enter 0.8 pending.

## Decisions

- JES observable semantics remain owned deterministic state machines in
  `mainframe-env-batch`. The common store, dataset, SAF, program registry,
  effect/UOW, checkpoint and artifact authorities are extended rather than
  duplicated.
- A normative 0.8 contract/registry owns typed lifecycle, initiator, output,
  control and utility identities. Runtime dispatch uses those typed identities;
  product behavior does not inspect conformance row IDs.
- Job and output durability uses versioned provider-neutral envelopes with
  bounded readers, non-destructive migration, and explicit unknown outcomes.
- No external scheduler or workflow dependency is planned. The owned state
  machines are smaller than an adapter plus the required semantic-gap matrix.
- Licensed IBM results will not be fabricated. A fail-closed receipt adapter
  may be prepared locally, but differential credit remains pending until a
  pinned licensed z/OS 3.2/JES2 campaign passes.

## Certification handoff

- The licensed z/OS 3.2 JES2 campaign receipt is absent. The required gate is
  fail-closed at 0/16 and cannot be satisfied by local, historical, or generated
  evidence. Under the approved completion policy this is not a 0.8
  implementation blocker; it remains a strict 0.17/1.0 certification blocker.

## JES-801 validation

The batch suite passes 99 tests, including bounded class/initiator selection,
durable selected/output recovery, legacy v1-to-v2 migration, per-step
checkpoints, `COND=ONLY` abend cleanup, and exact return-code/abend projection.
The server suite passes 26 tests. Strict batch Clippy, Draft 2020-12 schemas,
dependency architecture, runtime architecture, formatting, and patch hygiene
pass. The de-hardcoding gate now enumerates the Git candidate/index rather than
untracked developer files; this keeps candidate validation exact without
touching unrelated workspace content.

## JES-802 validation

The typed DD contract classifies dataset, inline, DUMMY and SYSOUT sources
before effects. OLD/SHR/NEW/MOD and PASS/KEEP/CATLG/DELETE/UNCATLG are
position-validated with explicit defaults. Allocation ownership uses the 0.6
dataset lock authority; CATLG/UNCATLG use dataset lifecycle state; compatible
concatenations use the bounded ordered dataset request; MOD uses append; GDG
and temporary resolutions are retained in durable job v2; and abnormal,
partial-allocation, and terminal temporary cleanup release all known resources.

The focused JES-802 matrix records 14 passing rows in
`conformance/0.8/evidence/jes-802-matrix.json`. The affected batch suite passes
108 tests and the dataset authority passes 55 tests, including explicit lock
conflict, ordered concatenation, MOD append, catalog/uncatalog, partial
allocation rollback, and terminal temporary cleanup cases. Strict Clippy for
both affected crates, Draft 2020-12 schemas, dependency architecture, runtime
architecture, formatting and patch hygiene pass.

## JES-803 validation

The typed `host.spool.read` and `host.spool.write` provider keeps bounded,
versioned metadata in `ProviderStateStore/jes-spool` and exact record bytes in
immutable `ArtifactStore` chunks. Durable job projections retain stable
job/step/DD descriptors and output groups. DD and OUTPUT routing resolves
class, destination and held disposition; authorized controls cover hold,
release, reroute, selection, completion, access, retention cleanup and purge.
Every public spool boundary checks SAF before artifact access or mutation.

Append replay and conflict, restart reads, output transitions, SAF denial,
legacy embedded-spool migration, retention, and intent-first partial purge are
covered by the 13-row matrix in
`conformance/0.8/evidence/jes-803-matrix.json`. The affected spool suite passes
3 tests, batch passes 112 tests, and server passes 26 tests. Strict Clippy for
the affected crates and workspace compilation pass. Draft 2020-12 schemas,
dependency architecture, runtime architecture, inventory, evidence,
formatting, and patch hygiene pass.

## JES-804 validation

Durable jobs now distinguish external batch, internal-reader, and started-task
origins. Internal-reader children retain their parent job and producing step;
started tasks retain their authorized task name. The bounded
`mainframe-env.jes-topology@1` contract models enabled/connected NJE nodes,
per-node inbound limits, MAS members, member capacity, and the origin,
execution, output, and selected-member projection on each job. Topology and
initiator enablement survive restart in versioned provider-state records.

The public controls cover hold/release/cancel/purge, class and priority change,
job and output routing, output selection/completion, initiator start/stop,
started-task start/stop, and topology installation. SAF is evaluated before
each transition. The focused 12-row matrix in
`conformance/0.8/evidence/jes-804-matrix.json` covers started-task recovery,
internal-reader provenance and denial, node/member routing and ownership,
topology/scheduler restart, job mutation controls, authorization denial, and
legacy defaults. The affected batch suite passes 117 tests; server and
conformance validation remain part of the work-package seal.

## JES-805 validation

Every step resolves once at admission into a versioned, typed durable
registration. Execution dispatches only `utility`, `program-service`, `sdsf`,
`db2-tso`, `ims-controller`, or explicit `unsupported` handlers. Restart
validates the frozen registration against its immutable step; missing or
substituted mappings fail before effects. Unknown application programs retain
the ordinary typed ProgramService route.

All nine required utilities now have semantic behavior. IEBCOPY performs an
exact INDD-to-OUTDD copy, IEBDG generates bounded deterministic DSD/FD/CREATE
records, IEBEDIT selects an exact job/range and optional step, and IEBUPDTE
writes an explicit ADD/REPL member body. The existing IEFBR14, IEBGENER,
IEBCOMPR, IDCAMS, and SORT handlers retain real allocation, copy, comparison,
catalog, and sorting effects. Unsupported controls return an explicit problem
without generic success. The 15-row matrix is recorded in
`conformance/0.8/evidence/jes-805-matrix.json`; the affected batch suite passes
121 tests, server passes 26, and conformance passes 168 with 2 explicit manual
or PostgreSQL tests ignored. Strict Clippy, Draft 2020-12 schemas, dependency
and runtime architecture, registry/de-hardcoding, inventory, evidence,
formatting, and patch hygiene pass.

## JES-806 validation

Step/job recovery now writes digest-bound `mainframe-env.jes-checkpoint@1`
records through the common `CheckpointStore`. A lost checkpoint acknowledgement
leaves the already-persisted running job visible as `unknown-outcome`; warm
restart retains completed steps and does not repeat their program or spool
effects. Ahead-of-job, substituted, or corrupt checkpoints fail closed, and
bounded attempt exhaustion becomes terminal without reexecution.
The recovery inventory binds the durable-job, aggregate runtime, and spool
state schemas plus both migration manifests to five recomputed SHA-256 digests;
`cargo xtask schemas --check` rejects any drift.

Cancellation is explicit before selection and when observed from a provider,
including step termination, job/output state, and checkpoint projection. Queue,
job, spool, event, effect, NJE inbound, and MAS active bounds reject growth.
Every lifecycle/control event now reserves bounded durable capacity before its
state mutation; selection advances from the persisted selected snapshot so the
operator-visible selected event cannot disappear on the running transition.
Purge uses a durable job intent across spool and checkpoint deletion so partial
failure remains retryable. The supported SQLite backup profile restores job,
spool metadata and payload chunks, checkpoint, scheduler, and topology state
only after integrity and semantic validation.

The `ProgramService` compatibility boundary carries validated job/step context
and installs a bounded synthetic PSA/TCB/TIOT chain only for COBOL modules that
declare the complete conventional linkage layout. It derives DD names from the
typed allocation plan, preserves unrelated initial values, and fails closed on
partial or invalid layouts; it does not claim physical JES2 or MVS control-block
parity. CardDemo also required typed IDCAMS cluster/AIX/path behavior and
structured dataset lock ordering when one dataset name prefixes another; both
are covered by affected package regressions.

The local recovery matrix is recorded in
`conformance/0.8/evidence/jes-806-matrix.json`. Licensed differential credit is
still **0/16**: `conformance/0.8/oracles/jes-licensed-differential.json` forbids
generated, historical, or local results from satisfying it. The fail-closed
`cargo xtask jes-oracle --check` gate requires an external reviewed receipt
bound to the exact staged Git-index candidate printed by
`cargo xtask jes-oracle-candidate`. The domain-separated digest rejects
unstaged tracked source, ignores unrelated untracked workspace files, and
excludes only this status report and the JES-806 result matrix so their
pending-to-pass transition cannot invalidate an otherwise unchanged campaign.
The frozen campaign candidate is
`sha256:2733803af3a110d6fc752d8ec9695f824ffc23f87554eaa026eb1fd3402bcd4d`.
Both excluded reports remain covered by the final work-package seal. Under the
user-approved 2026-09-04 policy, JES-806 and the 0.8 implementation exit as
`pass-with-licensed-differential-pending`: the standalone oracle remains
fail-closed, Hercules/MVS and local output receive zero licensed credit, and the
authentic 16-scenario campaign is mandatory at the 0.17 release-certification
hard gate before 1.0.

Workspace-wide tests and all-target compilation pass. The affected batch,
dataset, interpreter, server, and in-memory/SQLite store suites pass 136, 56,
12, 27, and 15 tests respectively; environment-dependent PostgreSQL and live
server cases remain explicitly ignored. Workspace-wide strict Clippy,
full-regression, Draft 2020-12 schemas, profiles, architecture,
runtime-architecture, inventory, evidence, registry/de-hardcoding,
migration-rollback, formatting, and patch-hygiene gates pass. The 0.8 package
and profile overlays are bound to strict Draft 2020-12 schemas and reconcile to
the Cargo dependency closure without rewriting frozen historical inventories.

The pinned external CardDemo corpus at commit
`59cc6c2fd7ebd7ef7925cad552a01a4b8b6e4d5e` passes the exact base-batch gate:
3 journeys, 12 initialization jobs, 9 operational jobs, 2 CICS file controls,
and one each of internal submission, warm restart, rollback, and cancellation.
Its journey-shape digest is
`sha256:863465f11c4bed015f70d66e1c68918f46729685499a613871d43344cde1a14d`;
the canonical schema-bound 0.8 receipt digest is
`sha256:f1416d04cadf4103dfbc47c6f2bacaee31351496aff16442bb6e0f784046eaf7`.
The gate verifies that the accepted CD-023 evidence remains byte-identical to
its certification commit and that the unchanged journey/count/corpus
projection is superseded explicitly rather than rewriting historical evidence.

## Certification follow-up

At 0.17, run the 16-scenario campaign on licensed z/OS 3.2 JES2, review its
receipt, set `MAINFRAME_ENV_JES_LICENSED_ORACLE_RECEIPT`, and rerun
`cargo xtask jes-oracle --check`. Until then, retain the exact 0/16 pending row
and do not infer equivalence from Hercules, MVS, modeled, historical, CardDemo,
or current-product observations.
