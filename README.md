# mainframe-env

This repository area contains the architecture and delivery contracts for the
greenfield mainframe-env rewrite. The first release is intentionally
limited to COBOL, CICS, JCL/JES, datasets, RACF/security, z/OSMF, and their
mandatory runtime dependencies. The existing OpenMainframe workspace is treated
as an executable compatibility oracle, not as a source dependency.

The design goal is a stable, robust platform built around a deterministic
compiler and execution kernel, with asynchronous infrastructure and external
frameworks isolated behind owned ports.

The current product version prepared for official release is **0.3.0** on the
0.3 release line. The [0.3 release notes](docs/releases/0.3.md) define its
claims and limitations. No tag, package publication, or deployment is
performed by this release-preparation branch.

Start with [the documentation index](docs/README.md).

To start the complete build program, use the
[long-horizontal implementation prompt](docs/prompts/IMPLEMENT_MAINFRAME_ENV_0_1.md).
Version, phase-commit, and promotion rules are defined in
[Versioning and releases](docs/delivery/VERSIONING-AND-RELEASES.md).

## Current status

- The 0.3 product identity is internally consistent and retains reproducible,
  unpublished release receipts for the advertised macOS ARM64 and Linux x86-64
  targets.
- The 0.3 through 0.7 subsystem work was integrated in parallel before product
  promotion. The 0.3 source baseline therefore contains later accepted work;
  that does not promote later minor claims into the 0.3 release.
- The generic platform passes CardDemo corpus certification: 27/27 issues and
  20/20 journeys with memory, SQLite, PostgreSQL 18, and live Zowe CLI evidence.
  CardDemo remains a non-production corpus/workload, not a product feature.
- Corpus tooling uses `CARDDEMO`; the pinned upstream FTP typo is retained only
  as a bounded compatibility alias.
- Local 0.1.1 release artifacts and provenance are generated and verified.
- No remote publication or production deployment has occurred.
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
