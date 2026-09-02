# mainframe-env

This repository area contains the architecture and delivery contracts for the
greenfield mainframe-env rewrite. The first release is intentionally
limited to COBOL, CICS, JCL/JES, datasets, RACF/security, z/OSMF, and their
mandatory runtime dependencies. The existing OpenMainframe workspace is treated
as an executable compatibility oracle, not as a source dependency.

The design goal is a stable, robust platform built around a deterministic
compiler and execution kernel, with asynchronous infrastructure and external
frameworks isolated behind owned ports.

The latest official GitHub Release is
[**0.7.0**](https://github.com/toreleon/mainframe-env/releases/tag/mainframe-env-v0.7.0)
on the 0.7 release line. The [0.7 release notes](docs/releases/0.7.md) define
its claims and limitations. Releases 0.2.0 through 0.7.0 are published with
their target-specific evidence archives; no production deployment is implied.

Start with [the documentation index](docs/README.md).

To start the complete build program, use the
[long-horizontal implementation prompt](docs/prompts/IMPLEMENT_MAINFRAME_ENV_0_1.md).
Version, phase-commit, and promotion rules are defined in
[Versioning and releases](docs/delivery/VERSIONING-AND-RELEASES.md).
Version progress is coordinated in the
[mainframe-env Roadmap](https://github.com/users/toreleon/projects/3), with its
[repository mapping and operating rules](docs/delivery/coverage-versions/GITHUB-PROJECT.md).

## Current status

- Official GitHub Releases 0.2.0 through 0.7.0 retain reproducible evidence
  archives for the advertised macOS ARM64 and Linux x86-64 targets. The
  checked-in candidate manifests remain immutable and retain their original
  `published=false` field.
- The 0.3 through 0.7 subsystem work was integrated in parallel before product
  promotion. The 0.7 source baseline truthfully preserves that integrated
  history rather than reconstructing alternate product trees.
- CardDemo online and batch execution currently fails for the documented 0.7
  compiler/IDCAMS limitations; stale historical CD-006/CD-013 receipts are not
  refreshed or treated as current passes. The proposed 0.7.1 patch was
  intentionally cancelled and is not part of the roadmap.
- Historical accepted 0.1.1/0.2 evidence records CardDemo corpus certification
  at 27/27 issues and 20/20 journeys with memory, SQLite, PostgreSQL 18, and
  live Zowe CLI evidence. That historical result is not a current 0.7 pass and
  does not supersede the current failure and stale-evidence disclosure above.
  CardDemo remains a non-production corpus/workload, not a product feature.
- Corpus tooling uses `CARDDEMO`; the pinned upstream FTP typo is retained only
  as a bounded compatibility alias.
- Local 0.1.1 release artifacts and provenance are generated and verified.
- Remote GitHub publication has occurred for 0.2.0 through 0.7.0; no production
  deployment has occurred.
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
