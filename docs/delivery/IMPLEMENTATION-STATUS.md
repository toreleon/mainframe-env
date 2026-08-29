# mainframe-env 0.1 Implementation Status

Current phase: **ME.V4 — typed CICS runtime**
Current work item: the ME.V4 candidate is complete; run the full gate and create the required phase commit.
Product version: **0.1.0-alpha.0**
Source identity: ME.V3 commit `2f8f332` plus the current ME.V4 candidate.

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
- Committed ME.V1 as `4bd5c983938354be621dcffd0cf3b6b9c5640ca5` with the required trailers.
- Implemented bounded fixed/free and CP037 COBOL input, lossless Rowan syntax, COPY/REPLACING
  with expansion identity, data layouts, typed HIR, Core-MIR lowering, fail-closed legality,
  artifact publication, a deterministic bounded machine, and shared CLI compile/run/inspect paths.
- Reproduced the frozen OpenMainframe HELLO output exactly (`sha256:9ca8206d...a637c25b`),
  proved 37 frozen supported statement-family routes, retained six oracle-matching unsupported
  families as compile-time diagnostics, and passed cancellation/timeout/resource/provider controls.
- Committed ME.V2 as `a11034a382ddcb8ccd378926f1f3bb91f0b1bcc2` with the required trailers.
- Implemented typed dataset and RACF/SAF providers over isolated provider-state contracts, including
  exact record/key bytes, member and browse behavior, optimistic mutation guards, atomic rename,
  durable replay/unknown-outcome protection, Argon2 credential verification through secret references,
  generic profiles, the RACF access hierarchy, default deny, bounded audit, and stable auth outcomes.
- Added a durable SQLx SQLite state adapter and passed close/reopen recovery for datasets, identities,
  groups, and profiles. Host middleware now fails closed on principal/grant, identity, cancellation,
  deadline, request/result bound, missing provider, provider panic, and malformed-result controls.
- Committed ME.V3 as `2f8f332` with the required trailers.
- Implemented the sole typed CICS provider for all 22 frozen operation families: 18 executable
  families and four explicit unsupported oracle-matching families. The shared contract parser
  distinguishes multiword forms, and interpreter lowering emits named schema-tagged arguments.
- Added bounded BMS maps, durable protocol-neutral terminal suspension/resume, file and program
  calls through scoped host services, typed CICS condition/EIB outcomes, transaction identity,
  transient data bounds, RACF default-deny admission, and SQLite suspended-session recovery.

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

No V0–V4 stop-the-line finding is open. ME.V5–ME.V7 implementation is pending. Current oracle gaps,
including historical evidence revision differences, partial COBOL/CICS forms, inactive TLS/CSRF,
and the pending PostgreSQL runner, are recorded in
`conformance/0.1/inventory/known-gaps.json` and block only their affected later promotions.

## Commands already run

All commands and exit codes are recorded in the machine status file. The unchanged oracle cohorts
passed: COBOL 37, RACF 8, z/OSMF datasets 9, z/OSMF jobs 9, CICS 317, JCL 164, and JES2 222 tests.

## Next smallest executable step

Run the complete locked ME.V4 gate, record its canonical content digest, review the staged diff, and
create the dedicated `Complete ME.V4 typed CICS runtime` commit.
