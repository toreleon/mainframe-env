# IBM coverage release plans

Status: **0.8.3 pre-0.9 hardening in development; 0.2.0 through 0.8.2 released; 0.9.0 through 1.0.0 planned**
Applies after: `mainframe-env 0.1.1`
Planning authority:
[`IBM-OFFICIAL-COVERAGE-ROADMAP.md`](../../research/IBM-OFFICIAL-COVERAGE-ROADMAP.md)
Machine roadmap:
[`ibm-official-coverage-roadmap.json`](../../../conformance/roadmap/ibm-official-coverage-roadmap.json)
Operational tracker:
[GitHub Project synchronization](GITHUB-PROJECT.md)

These documents prepare the implementation and certification work for each
minor version. They do not authorize a tag, publication, deployment, remote
push, or a compatibility claim.

## Version plans

| Version | State | Primary result | Plan |
|---|---|---|---|
| 0.2.0 | Released | Coverage authority and zero application hardcode | [0.2.0](0.2.0.md) |
| 0.3.0 | Released | Complete COBOL grammar, directives, clauses, types, and layouts | [0.3.0](0.3.0.md) |
| 0.4.0 | Released | Complete COBOL execution and differential semantics | [0.4.0](0.4.0.md) |
| 0.5.0 | Released | Complete RACF command language and SAF | [0.5.0](0.5.0.md) |
| 0.6.0 | Released | Complete dataset, VSAM, catalog, locking, and AMS surface | [0.6.0](0.6.0.md) |
| 0.7.0 | Released | Complete JCL converter and planner | [0.7.0](0.7.0.md) |
| 0.8.0 | Released | Complete JES2 execution and real utility semantics | [0.8.0](0.8.0.md) |
| 0.9.0 | Planned | Complete CICS application API | [0.9.0](0.9.0.md) |
| 0.10.0 | Planned | Complete CICS SPI and FEPI | [0.10.0](0.10.0.md) |
| 0.11.0 | Planned | Complete z/OSMF 3.2 REST portfolio | [0.11.0](0.11.0.md) |
| 0.12.0 | Planned | Generic Db2 parser, catalog, executor, and common SQL | [0.12.0](0.12.0.md) |
| 0.13.0 | Planned | Complete Db2 13 programming surface | [0.13.0](0.13.0.md) |
| 0.14.0 | Planned | Complete IMS 15.6 programming surface | [0.14.0](0.14.0.md) |
| 0.15.0 | Planned | Complete IBM MQ 9.4 programming surface | [0.15.0](0.15.0.md) |
| 0.16.0 | Planned | Complete cross-resource transaction and failure semantics | [0.16.0](0.16.0.md) |
| 0.17.0 | Planned | Licensed IBM differential certification and 1.0 rehearsal | [0.17.0](0.17.0.md) |
| 1.0.0 | Planned | Stable pinned programming-surface release | [1.0.0](1.0.0.md) |

0.8.0 published on 2026-09-04: the runtime merged in
[#30](https://github.com/toreleon/mainframe-env/pull/30), the release commit and
tag `mainframe-env-v0.8.0` landed in
[#31](https://github.com/toreleon/mainframe-env/pull/31), and the GitHub Release
carries both target evidence archives. It shipped with three confirmed defects
open against its own claims; they are listed in the release's known-limitations
section and detailed in the [0.8.0 review](review/0.8.0.md).

Patch 0.8.1 published on 2026-09-05 after
[#43](https://github.com/toreleon/mainframe-env/pull/43) resolved all nine
review findings. It preserves the approved licensed-differential-pending
disposition without assigning Hercules or modeled results equivalence credit.

Patch 0.8.2 published on 2026-09-06 with runtime, persistence, provider-move,
and source-distribution hardening. It published a locked Cargo source bundle,
not new native binaries; the exact compatibility limits are in the
[0.8 release notes](../../releases/0.8.md). Later changes on `main` remain
unreleased and do not retroactively change the 0.8.2 tag or its evidence.

The current workspace is `0.8.3` development. PR
[#131](https://github.com/toreleon/mainframe-env/pull/131) merged the hardening
changes; integrated entry acceptance remains tracked in
[status/0.9.0.md](status/0.9.0.md) and
[#137](https://github.com/toreleon/mainframe-env/issues/137).
Neither the merge nor this planning amendment is a release or compatibility claim.

See [Parallel implementation plan](PARALLEL-IMPLEMENTATION.md) for the work DAG,
safe concurrency lanes, merge discipline, and critical path.

See [GitHub Project synchronization](GITHUB-PROJECT.md) for the epic, pull
request, and review-issue history behind each version.

Ready-to-run coding-agent instructions for every minor are indexed in
[`docs/prompts/coverage-versions/README.md`](../../prompts/coverage-versions/README.md).
The 1.0.0 dossier has no implementation prompt because it is a release promotion
gate rather than a minor feature program.

## Common release contract

Each version plan uses the following terms:

- **Start gate**: the contracts and authorities that must be frozen before its
  implementation can safely begin.
- **Completion dependencies**: versions whose accepted behavior must be present
  before this version can pass its exit gate. A team may begin an isolated
  parser/catalog workstream earlier when the version plan explicitly permits it.
- **Owned scope**: behavior that this version must finish. A later version may
  not be used to hide an incomplete row.
- **Parallel-safe work**: files/contracts that can be developed independently
  after the start gate.
- **Integration points**: shared contracts or state authorities where work must
  be serialized or merged through one owner.
- **Exit gate**: observable evidence required to call the version complete.

All versions inherit these invariants:

1. Every official inventory row records `recognized`, `validated`, `executed`,
   `conditioned`, `recovered`, and `differential` independently.
2. Partial rows never count as complete and cannot be relabeled by prose.
3. Production crates contain no application-specific program, table,
   transaction, dataset, queue, map, or principal dispatch.
4. Application data and resources enter through bounded, signed, versioned
   application packages.
5. Handwritten protocol lists are replaced by reviewed catalogs and generated
   exhaustive code; generated code still requires semantic handlers and tests.
6. Product semantics remain deterministic; infrastructure and licensed oracle
   adapters stay outside the semantic kernels.
7. Every prior profile affected by changed contracts or routes remains green.
   The global CardDemo 20-journey regression runs at scheduled integration,
   0.16/0.17, and release-candidate certification rather than on every PR.
8. No future-version incomplete provider, hidden fallback, or generic-success
   stub ships in the `core-server` closure.
9. A licensed IBM environment is required for a `differential=pass` result.
10. Remote release actions require separate authorization.

All 0.9–0.17 dossiers also inherit the shared
[hardened slice acceptance](../../prompts/coverage-versions/README.md#hardened-slice-acceptance),
[early participant contract](../../prompts/coverage-versions/README.md#early-transaction-participant-contract),
and [licensed-harness preparation](../../prompts/coverage-versions/README.md#licensed-harness-preparation).
Gate applicability is source-backed and obligation-specific. Every mandatory
obligation must pass its applicable gates; unsupported required execution is
not completed by a successful rejection or a reduced denominator.

### Recorded licensed-pending dispositions

Implementation acceptance, licensed differential completion, and release
promotion are separate decisions. The following are already-recorded scoped
handoffs, not new exceptions created by this amendment:

| Implementation | Recorded approval/disposition source | Licensed handoff | Mandatory closure |
|---|---|---|---|
| 0.4 COBOL | [Historical status, explicit 2026-09-02 approval](status/0.4.0.md) | 0/153 pending | CER-1702 and 0.17 release-certify |
| 0.5 RACF/SAF | [Historical status](status/0.5.0.md); user-approved 2026-09-01 scoped policy | 0/48 pending | CER-1702 and 0.17 release-certify |
| 0.6 dataset/VSAM/AMS | [Historical status, approved 2026-09-01 policy](status/0.6.0.md) | 0/36 pending | CER-1702 and 0.17 release-certify |
| 0.8 JES2 | [Historical status, recorded approved completion policy](status/0.8.0.md) | 0/16 pending | CER-1702 and 0.17 release-certify |

The linked records preserve the historical implementation and receipt context;
they are not a fresh attestation of the current tree. At dependency consumption,
record the accepted candidate SHA/tree, receipt and approval references, affected
regressions and still-pending obligations in the target status. A starting branch
or dependency SHA in an old status is not automatically that version's accepted
candidate. Missing identity or approval evidence remains unresolved until verified.

These dispositions never grant IBM credit to local models, GnuCOBOL, Hercules,
CardDemo or historical observations. The licensed numerators remain pending
until real pinned receipts pass. No blanket extension to 0.9–0.15 is authorized:
their current licensed exit requirements remain unchanged. In particular, the
0.9 hardening entry gate is additional to its 0.4/0.5/0.6 dependency acceptance.

## Post-131 amendment tracking

[#132](https://github.com/toreleon/mainframe-env/issues/132) tracks the paired
prompt/dossier update: shared contracts
[#133](https://github.com/toreleon/mainframe-env/issues/133), CICS/z/OSMF
[#134](https://github.com/toreleon/mainframe-env/issues/134), Db2
[#135](https://github.com/toreleon/mainframe-env/issues/135), and IMS/MQ/integration/
certification [#136](https://github.com/toreleon/mainframe-env/issues/136).
The real entry-evidence verification in
[#137](https://github.com/toreleon/mainframe-env/issues/137) is separate and must
not be closed by a documentation-only PR. Existing release epics remain the
implementation trackers; this amendment changes neither coverage nor acceptance.

## Common evidence package

From 0.3 onward, every claimed coverage gate flows through the shared typed
[Conformance IR](../../architecture/CONFORMANCE-IR.md):

```text
official row -> typed row spec -> mandatory obligation -> executable binding
             -> obligation/gate verdict -> ledger
```

The retained package is intentionally small:

- candidate commit SHA;
- official catalog and Conformance IR version/digest;
- canonical obligation-level verdict stream or its artifact digest;
- generated numerator/denominator ledger for affected gates;
- CI verdict/reference and shipped artifact digest;
- migration/restart/rollback receipt only when durable behavior changes; and
- licensed IBM oracle receipt only for a differential claim.

Do not add per-review schemas, free-form command transcripts, test-count claims,
GitHub job archives, dirty-tree/status evidence, or manually maintained coverage
pass counts. Release notes, limitations, and upgrade instructions remain normal
documentation rather than cryptographic conformance evidence.

## Risk-tiered validation floor

Validation cost follows risk and lifecycle stage:

| Tier | Required scope |
|---|---|
| Inner loop | Focused package test and affected schema/inventory/conformance shard |
| Work-package/PR | Formatting, compile/check, focused positive and negative semantics, affected public routes, and `git diff --check` |
| Minor exit | Workspace plus complete affected-subsystem conformance once on one unchanged integrated candidate |
| Nightly/release | Global CardDemo, PostgreSQL, Zowe, load/recovery, dual-target artifacts, cross-subsystem replay, and required licensed IBM differential |

Public or durable contract changes add schema compatibility, migration,
restart, rollback, backup/restore, and security gates as applicable. Tooling,
documentation, CI, and evidence-only changes do not rerun application
environments unless they alter shipped artifacts or the corresponding gate.

Every plan may add focused subsystem gates, but it may not promote partial
recognition to semantic coverage or silently skip malformed, condition/status,
authorization, failure, and forbidden-mutation checks for behavior it changes.
Evidence is generated from the final accepted run; it is not a reason to repeat
an otherwise unchanged expensive gate.
