# mainframe-env

This repository area contains the architecture and delivery contracts for the
greenfield mainframe-env rewrite. The first release is intentionally
limited to COBOL, CICS, JCL/JES, datasets, RACF/security, z/OSMF, and their
mandatory runtime dependencies. The existing OpenMainframe workspace is treated
as an executable compatibility oracle, not as a source dependency.

The design goal is a stable, robust platform built around a deterministic
compiler and execution kernel, with asynchronous infrastructure and external
frameworks isolated behind owned ports.

The current product version prepared for official release is **0.8.0** on the
0.8 release line. The [0.8 release notes](docs/releases/0.8.md) define its
claims and limitations. No tag, package publication, or deployment is
performed by this release-preparation branch.

Start with [the documentation index](docs/README.md).

To start the complete build program, use the
[long-horizontal implementation prompt](docs/prompts/IMPLEMENT_MAINFRAME_ENV_0_1.md).
Version, phase-commit, and promotion rules are defined in
[Versioning and releases](docs/delivery/VERSIONING-AND-RELEASES.md).
Version progress is coordinated in the
[mainframe-env Roadmap](https://github.com/users/toreleon/projects/3), with its
[repository mapping and operating rules](docs/delivery/coverage-versions/GITHUB-PROJECT.md).

## Current status

- The 0.8 product identity is internally consistent and retains reproducible,
  unpublished release receipts for the advertised macOS ARM64 and Linux x86-64
  targets.
- The 0.3 through 0.7 subsystem work was integrated in parallel before product
  promotion. The 0.7 source baseline truthfully preserves that integrated
  history rather than reconstructing alternate product trees.
- The pinned CardDemo base-batch corpus passes 3 journeys, 12 initialization
  jobs, and 9 operational jobs. This bounded 0.8 evidence does not refresh or
  replace historical full-corpus certification claims.
- Historical accepted 0.1.1/0.2 evidence records CardDemo corpus certification
  at 27/27 issues and 20/20 journeys with memory, SQLite, PostgreSQL 18, and
  live Zowe CLI evidence. That historical result does not establish current
  0.8 full-corpus certification or supersede the bounded base-batch evidence
  above. CardDemo remains a non-production corpus/workload, not a product
  feature.
- Corpus tooling uses `CARDDEMO`; the pinned upstream FTP typo is retained only
  as a bounded compatibility alias.
- Local 0.1.1 release artifacts and provenance are generated and verified.
- Licensed z/OS 3.2/JES2 differential credit remains 0/16 pending and is
  deferred to the 0.17 certification hard gate. Hercules, MVS 3.8J, modeled
  behavior, and local product output receive zero licensed equivalence credit.
- Remote GitHub publication has occurred for 0.2.0 through 0.7.0; this 0.8
  preparation does not itself publish or deploy the product.
- No old implementation may be imported by mainframe-env production crates.
- Existing fixtures, schemas, behavioral tests, and evidence may be reused as
  compatibility inputs.
- Packages and selectors outside the 0.1 scope have no migration obligation and
  are not compiled into the mainframe-env workspace.

## Intended development topology

```text
OpenMainframe current workspace
    executable oracle and frozen behavior
                 |
                 | differential and compatibility tests
                 v
mainframe-env
    independent contracts and implementation
```

The final cutover promotes mainframe-env to the product workspace and archives or
removes the superseded implementation. The final supported product must not
contain two competing default frameworks.
