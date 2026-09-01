# IBM coverage release plans

Status: **Proposed implementation plans**
Applies after: `mainframe-env 0.1.1`
Planning authority:
[`IBM-OFFICIAL-COVERAGE-ROADMAP.md`](../../research/IBM-OFFICIAL-COVERAGE-ROADMAP.md)
Machine roadmap:
[`ibm-official-coverage-roadmap.json`](../../../conformance/roadmap/ibm-official-coverage-roadmap.json)

These documents prepare the implementation and certification work for each
minor version. They do not authorize a tag, publication, deployment, remote
push, or a compatibility claim.

## Version plans

| Version | Primary result | Plan |
|---|---|---|
| 0.2.0 | Coverage authority and zero application hardcode | [0.2.0](0.2.0.md) |
| 0.3.0 | Complete COBOL grammar, directives, clauses, types, and layouts | [0.3.0](0.3.0.md) |
| 0.4.0 | Complete COBOL execution and differential semantics | [0.4.0](0.4.0.md) |
| 0.5.0 | Complete RACF command language and SAF | [0.5.0](0.5.0.md) |
| 0.6.0 | Complete dataset, VSAM, catalog, locking, and AMS surface | [0.6.0](0.6.0.md) |
| 0.7.0 | Complete JCL converter and planner | [0.7.0](0.7.0.md) |
| 0.8.0 | Complete JES2 execution and real utility semantics | [0.8.0](0.8.0.md) |
| 0.9.0 | Complete CICS application API | [0.9.0](0.9.0.md) |
| 0.10.0 | Complete CICS SPI and FEPI | [0.10.0](0.10.0.md) |
| 0.11.0 | Complete z/OSMF 3.2 REST portfolio | [0.11.0](0.11.0.md) |
| 0.12.0 | Generic Db2 parser, catalog, executor, and common SQL | [0.12.0](0.12.0.md) |
| 0.13.0 | Complete Db2 13 programming surface | [0.13.0](0.13.0.md) |
| 0.14.0 | Complete IMS 15.6 programming surface | [0.14.0](0.14.0.md) |
| 0.15.0 | Complete IBM MQ 9.4 programming surface | [0.15.0](0.15.0.md) |
| 0.16.0 | Complete cross-resource transaction and failure semantics | [0.16.0](0.16.0.md) |
| 0.17.0 | Licensed IBM differential certification and 1.0 rehearsal | [0.17.0](0.17.0.md) |
| 1.0.0 | Stable pinned programming-surface release | [1.0.0](1.0.0.md) |

See [Parallel implementation plan](PARALLEL-IMPLEMENTATION.md) for the work DAG,
safe concurrency lanes, merge discipline, and critical path.

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

## Common evidence package

From 0.3 onward, every claimed coverage gate flows through the shared typed
[Conformance IR](../../architecture/CONFORMANCE-IR.md):

```text
official row -> formal case -> executable test -> row/gate verdict -> ledger
```

The retained package is intentionally small:

- candidate commit SHA;
- official catalog and Conformance IR version/digest;
- canonical row-level verdict stream or its artifact digest;
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
