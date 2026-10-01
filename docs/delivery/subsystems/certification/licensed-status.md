# Licensed certification — Differential certification progress

Subsystem: **certification**
Phase: **licensed**
Target release: **0.17.0**

Status: **CER-1701 shared harness foundation in progress; all licensed differentials pending**

This lane starts from `origin/main` commit
`5ab706b1dd069e26db7cb9a2b66e921c9001fc39` on branch
`codex/parallel-v0.17-cer1701`. It prepares only the shared licensed-environment
and oracle-harness contracts allowed to proceed alongside provider lanes. The
0.11 and 0.16 completion dependencies have not been consumed, no final
certification candidate exists, and no licensed campaign is run or credited by
CER-1701.

## Bounded slices

| Slice | Parent | Scope | State | Focused acceptance |
|---|---|---|---|---|
| `CER-1701.environment-schema` | `CER-1701` | Versioned external environment manifest, pending slot requirements, product/service/APAR/PTF, locale/CCSID, topology, authorization, capability, tool, capture, redaction, and bound limits | pass | Draft 2020-12 compilation and deterministic pending-authority validation |
| `CER-1701.oracle-bindings` | `CER-1701` | Shared harness/receipt envelope, existing-adapter read compatibility, source/fixture/oracle/environment/candidate/artifact identities, reviewed normalization and bounded capture | pass | Additive read-version and exact cross-reference checks |
| `CER-1701.harness-validation` | `CER-1701` | Synthetic zero-credit fixtures, mutation/fail-closed checks, candidate invalidation, CLI validation, and handoff documentation | pass | Focused tooling tests, schema/architecture/docs/format checks |

The bounded early CER-1701 foundation is sealed by these three slices. Actual
external environment population and exact-candidate handoff remain future
CER-1701 integration work. CER-1702 through CER-1706 remain out of scope and
pending.

## Authority and slot inventory

CER-1701 owns one shared environment-manifest contract and one shared harness
registry. It does not create a second Conformance IR, coverage ledger, product
router, security evaluator, transaction authority, or release evidence family.
The external captured environment manifest is the authority for licensed
environment facts; the checked-in requirements document records only required
fields, source identities, adapter compatibility, and explicit blockers.

The frozen additive interface versions are:

- `mainframe-env.licensed-environment-requirements@1` (checked-in pending
  authority);
- `mainframe-env.licensed-environment-manifest@1` (external captured facts);
- `mainframe-env.oracle-harness-registry@1` (shared slot/adapter authority);
- `mainframe-env.oracle-candidate-binding@1` (source/tree/artifact and harness
  identity tuple); and
- `mainframe-env.oracle-harness-receipt@1` (zero-credit CER-1701 envelope).

The additive read rule is `exact-v1-plus-cer1701-envelope`: the existing
subsystem validator must first accept its unchanged v1 artifact, and the shared
validator then requires the v1 artifact digest plus every missing environment,
candidate, artifact, normalizer, and registry binding. Accepted historical
read versions are COBOL licensed receipt v1, RACF campaign v1, dataset receipt
v1, JES2 receipt v1, and CICS capture v1. Db2, IMS, MQ, z/OSMF, and
cross-resource readers remain `pending-no-reader`.

| Slot | Required source/product family | Environment | Differential |
|---|---|---|---|
| COBOL | Enterprise COBOL 6.5 and LE | missing | **0/153 pending** |
| RACF/SAF | z/OS 3.2 RACF/SAF | missing | **0/48 pending** |
| dataset/VSAM/AMS | z/OS 3.2 DFSMS/AMS | missing | **0/36 pending** |
| JES2 | z/OS 3.2 JES2 | missing | **0/16 pending** |
| CICS | CICS TS 6.x plus Enterprise COBOL | missing | pending; scoped pilot only, no whole-family numerator claimed |
| Db2 | Db2 13 for z/OS | missing | pending |
| IMS | IMS 15.6 | missing | pending |
| MQ | IBM MQ 9.4 for z/OS | missing | pending |
| z/OSMF | z/OSMF 3.2 | missing | pending |
| cross-resource | exact shared provider composition | missing | pending |

Pending means no empty, local, modeled, documentation-derived, historical, or
synthetic result can become licensed credit. The four recorded historical
numerators are preserved exactly and are not recomputed by this lane.

## IBM source review

Offline `ibm_docs.py search`, `status`, and `read` were run from a temporary
path-and-content-addressed cache hard-linked to the retained archive at
`/Users/tore/Library/Caches/mainframe-env/ibm-docs-archive/raw`. The repository
parser verified the committed topic hashes and pinned TOCs for COBOL, CICS,
JCL/JES2, dataset/VSAM/AMS, RACF/SAF, z/OSMF, IMS, and the selected MQ topic.
Relevant reviewed topics are:

- `ibm-enterprise-cobol-6.5-2026-05-31`,
  `SS6SG3_6.5/lr/ref/rllancp.html`,
  `sha256:90f8387ccca8dc02cf2c42834056284960c4f2bce460ca3c5ccd49a75c2c3641`;
- `ibm-zos-3.2-racf-saf-2026`,
  `SSLTBW_3.2.0/com.ibm.zos.v3r2.icha400/adduser.htm`,
  `sha256:12c201c5a7fb21ced7d84ef23606d2eb0442cdff870dc2180a0809372cd1d977`;
- `ibm-zos-3.2-dfsms-ams-2026-06`,
  `SSLTBW_3.2.0/com.ibm.zos.v3r2.idai200/code.htm`,
  `sha256:3b815f7af2a27841c9ea11b7b25db6cce4b2a1a93915bd83a8d068b68e066bac`;
- `ibm-zos-3.2-jcl-jes2-2026-06`,
  `SSLTBW_3.2.0/com.ibm.zos.v3r2.ieab600/xj2comd.htm`,
  `sha256:2502d339172d47138dab35386ddc671bddb1871b6bda4ae3bd5a6c1a41f1a93b`;
- `ibm-cics-ts-6x-2026-08-31`,
  `SSJL4D_6.x/reference-diagnostics/eib/dfha8mf.html`,
  `sha256:78f90b09987b1a56da7cd9f0a2a36fa43a549966fa7dbeff2ad9608106ef7c25`;
- `ibm-zosmf-3.2-2026-07-27`,
  `SSLTBW_3.2.0/com.ibm.zos.v3r2.izua700/izuprog_API_GetRetrieveVersionInfo.htm`,
  `sha256:560b2423660dc7318dbc6cbded05819556be8728eaeaf1b4539eb1738643e9ed`;
- `ibm-ims-15.6-dli-2026-08-31`,
  `SSEPH2_15.6.0/com.ibm.ims156.doc.apg/ims_comparingexecdlicmdsanddlicalls.htm`,
  `sha256:ce4a179eac0f18ed4ff71bed9ca576b31bcbbdb076d305dbcbf942e26ce13e30`;
- `ibm-mq-9.4-mqi-2026-08-31`,
  `SSFKSJ_9.4.0/refdev/q101760_.html`,
  `sha256:fa0cdd2c5e19326dfb91e5ad0b921fd47a1a3a918682e13c4ff5e36c2ba40347`.

The retained Db2 scope is incomplete (52 topic bodies and its pinned TOC are
absent), so the searched Db2 topic cannot be accepted through `ibm_docs.py
read`. One of 27 MQ topic bodies is also absent, although the selected MQCONN
topic and pinned TOC verify. These are source-cache availability blockers for
future Db2/MQ field review; no refresh was attempted and no missing content was
inferred.

## Candidate invalidation and exact-candidate handoff

A later source commit/tree, compiled artifact, official catalog or spec,
independent fixture, adapter/normalizer, environment manifest, or harness
registry change invalidates every receipt bound to that component. Reuse is
allowed only after exact identity comparison plus an explicit compatibility
rule proves the changed component irrelevant. A historical receipt is never
relabelled to a later candidate.

The eventual CER-1702 runner must receive an external captured environment
manifest, protected-runner receipt/capture, exact source commit and source-tree
digest, exact shipped artifact digests, catalog/spec/fixture/adapter/normalizer
digests, and the frozen CER-1701 registry digest. The subsystem's existing
fail-closed adapter remains the semantic oracle authority. CER-1701 validation
can establish only schema/identity/plumbing readiness and always reports zero
licensed differential credit.

## Current blockers and next step

All ten licensed environments, protected runner identities/attestations, final
candidate commit/tree, shipped artifacts, and final independent fixtures are
missing. Db2 retained documentation is incomplete and one MQ source topic is
missing. The environment schema, pending slot authority, shared registry, and
additive receipt validator pass their focused registry and cross-authority
tests. RACF and CICS have reviewed exact-observation normalization policies;
COBOL, dataset, JES2, Db2, IMS, MQ, z/OSMF, and cross-resource normalization
remains explicitly pending review. The repository-wide
`cargo xtask schemas --check` reaches the pre-existing 0.8 CardDemo base-batch
artifact and fails because its `supersession.reason` text exceeds that
historical schema's 256-character limit; CER-1701 does not modify either file.
The synthetic environment, CICS v1 capture, and CER-1701 envelope pass both the
unchanged CICS adapter-contract reader and shared validator at zero credit.
Mutations of candidate identity, environment bytes, legacy capture bytes, case
closure, pending-slot readiness, and licensed-origin metadata all fail closed.
`cargo xtask licensed-harness --check` is the fast deterministic plumbing gate.

The next executable step belongs to each provider lane: supply its independent
fixture manifest, reviewed normalization policy, bounded adapter/read version,
and environment requirements through the shared registry. Later CER-1701
integration must replace no historical receipt; it supplies external captured
environment facts and a new envelope for the exact candidate. CER-1702 may run
only after those inputs and the protected licensed runners exist.

## Focused validation

The final CER-1701 foundation ran these affected checks:

- `cargo test -p mainframe-env-conformance licensed_harness -- --nocapture`:
  five passed, covering registry closure, pending-adapter rejection, the
  synthetic envelope plus unchanged CICS v1 reader, binding/case mutations,
  and pending/forged-origin rejection;
- `cargo test -p xtask --bin xtask licensed_environment_schema_tests --
  --nocapture`: two passed, covering the source/slot/schema authority and the
  zero-credit CLI fixture;
- `cargo xtask licensed-harness --check`: pass, reporting
  `licensed-credit=0 differential=pending`;
- `cargo xtask docs --check`: pass after regenerating the owned documentation
  manifest;
- `cargo fmt --all -- --check` and `git diff --check`: pass.

Three broader baseline checks were diagnosed once and not weakened or repaired
outside CER-1701 scope. `cargo xtask schemas --check` reaches an unchanged 0.8
CardDemo artifact whose `supersession.reason` exceeds its schema's
256-character limit. `cargo xtask architecture --check` reaches an unchanged
`carddemo-operator-submit` mention in dataset `replay_index.rs`. The standalone
module-boundary check reaches unchanged CICS pilot growth (1,345 production
lines versus its recorded 1,321-line ceiling); the new licensed-harness module
is below the ordinary 1,200-line limit. These failures are not licensed or
candidate evidence and do not change any pending numerator.
