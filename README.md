# mainframe-env

This repository area contains the architecture and delivery contracts for the
greenfield mainframe-env rewrite. The first release is intentionally
limited to COBOL, CICS, JCL/JES, datasets, RACF/security, z/OSMF, and their
mandatory runtime dependencies. The existing OpenMainframe workspace is treated
as an executable compatibility oracle, not as a source dependency.

The design goal is a stable, robust platform built around a deterministic
compiler and execution kernel, with asynchronous infrastructure and external
frameworks isolated behind owned ports.

The initial product release line is **0.1**, beginning at
`0.1.0-alpha.0` and reaching `0.1.0` only through the documented release gates.

Start with [the documentation index](docs/README.md).

To start the complete build program, use the
[long-horizontal implementation prompt](docs/prompts/IMPLEMENT_MAINFRAME_ENV_0_1.md).
Version, phase-commit, and promotion rules are defined in
[Versioning and releases](docs/delivery/VERSIONING-AND-RELEASES.md).

## Current status

- Product roadmap work, including R2A, is paused.
- The ME.V0 architecture, compatibility freeze, and workspace-foundation gate passes locally.
- Foundation and product implementation phases ME.V1 through ME.V7 remain incomplete.
- No production authority has moved.
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
