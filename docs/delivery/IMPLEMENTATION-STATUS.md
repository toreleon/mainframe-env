# mainframe-env 0.1 Implementation Status

Current phase: **ME.V1 — foundation and contracts**
Current work item: the ME.V1 gate passed; create the required phase commit.
Product version: **0.1.0-alpha.0**
Source identity: unborn `main` branch with deterministic initial content digest
`sha256:f821878d7ad92236993e88fa2eab9355f3c493abe31bf2c49e653becc92e25b5`.

## Completed deliverables

- Read the complete mainframe-env normative set and the required OpenMainframe oracle inputs.
- Initialized a local Git repository on `main`; no remote exists.
- Froze the clean OpenMainframe oracle at
  `cee4b1df3fa5e32066b7948d528c163f8636a18e`.
- Reproduced selected COBOL, RACF, z/OSMF dataset/job, CICS, JCL, and JES oracle tests.
- Accepted the owner-authorized mainframe-env identity, greenfield direction, 0.1 scope,
  release line, and local phase-commit protocol.
- Created the contract, inventory, schema, fixture, workload, and entry-evidence pack under
  `conformance/0.1/`.
- Added Rust 2024 workspace/release authorities and deterministic repository checks in `xtask`.
- Passed the full V0 check/test/Clippy/doc/format and xtask gate on Rust 1.98.0.
- Committed ME.V0 as `811aaf1aa84077ca296d83ebca96d17c1dd3843c` with the required trailers.
- Implemented exact bounded source bundles/provenance, stable diagnostics, CP037 and numeric
  byte codecs, typed generic IR with legality and owned codecs, opaque compiler/artifact stages,
  explicit execution identities/outcomes/events, typed host/CICS effects, immutable capability
  snapshots, store contracts, and bounded in-memory stores.
- Passed 42 ME.V1 unit/property tests plus strict Clippy/docs, architecture, inventory, and Rust
  1.95 contract MSRV checks.

Exact machine evidence is in
`conformance/0.1/evidence/program-status.json` and
`conformance/0.1/evidence/entry.json`.

## Accepted decisions

- 0.1 covers only COBOL, CICS, JCL/JES, datasets, RACF/SAF, and 23 selected z/OSMF routes.
- The 43 R0 COBOL statement variants and 22 pinned CardDemo CICS operations form the finite
  language/transaction freeze; already unsupported oracle behavior stays explicitly unsupported.
- The proposed package map is consolidated to 20 packages where a separate package would not
  enforce an independent stable contract, provider selection, or application boundary.
- OpenMainframe remains an out-of-process oracle and is never a production dependency.

## Known gaps and stop-the-line findings

No V0 or V1 stop-the-line finding is open. ME.V2–ME.V7 implementation is pending. Current oracle gaps,
including historical evidence revision differences, partial COBOL/CICS forms, inactive TLS/CSRF,
and the pending PostgreSQL runner, are recorded in
`conformance/0.1/inventory/known-gaps.json` and block only their affected later promotions.

## Commands already run

All commands and exit codes are recorded in the machine status file. The unchanged oracle cohorts
passed: COBOL 37, RACF 8, z/OSMF datasets 9, z/OSMF jobs 9, CICS 317, JCL 164, and JES2 222 tests.

## Next smallest executable step

Record the canonical V1 content digest, review the complete staged diff, and create the dedicated
`Complete ME.V1 foundation contracts` commit. Then begin the COBOL/reference-machine phase.
