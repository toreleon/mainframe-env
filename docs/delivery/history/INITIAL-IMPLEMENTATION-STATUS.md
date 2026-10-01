# mainframe-env 0.1 Implementation Status

Current phase: **CARDDEMO 0.1.1 — released locally**
Current work item: none; all 27 implementation issues and all 20 application journeys pass.
Product version: **0.1.1**
Source baseline: CardDemo-full completion commit
`857115b907ce7098c965a51117a079048ea8182e`; ADR-0007 owns release promotion.

The released 0.1.1 profile is under `conformance/0.1.1/` and
`docs/delivery/CARDDEMO-0.1.1-STATUS.md`. Product-controlled identifiers use
`CARDDEMO`; the misspelled upstream FTP dataset remains only as a bounded alias.

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
- Committed ME.V4 as `9cb93a0` with the required trailers.
- Implemented bounded JCL planning with JOB/EXEC/DD, procedures, includes, symbols, conditions,
  restart/skip, inline data, DD/DISP, and source provenance. JES is the sole durable job/spool
  authority for submit, hold/release, queue/run, completion/failure, cancellation, purge, and warm start.
- All executable steps dispatch through `host.program.invoke`; nine accepted utilities have explicit
  routes and COBOL is injected only through the same ProgramService. Selected IEBGENER preserves exact
  input record bytes, unknown utilities fail explicitly, and SQLite recovers running work to its queue.
- Committed ME.V5 as `4a752fa` with the required trailers.
- Implemented the thin 23-route z/OSMF gateway and single-node product composition over the common
  dataset, RACF, CICS, compiler/interpreter, program, and JES authorities. Added stable auth/CSRF/error
  mapping, versioned config precedence, readiness, bounded shutdown, metrics, and Rustls construction.
- Added embedded SQLite/PostgreSQL migration `0001-durable-state`, full durable store contracts,
  immutable local artifacts, SQLite integrity/backup/restore, and PostgreSQL 18 execution/provider
  compatibility. The temporary PostgreSQL test container was stopped and auto-removed after the pass.
- Committed ME.V6 as `c7f0c6d555709ec0bfb7f64866e29c7d2d04dece` with the required trailers.
- Completed six-domain differential summaries, accepted corrections, exact authority/profile closure,
  release-mode 1x/2x/long-run gates, supply-chain/license/advisory checks, cutover/archive decisions,
  and reproducible alpha manifest/SBOM/checksum/license/provenance artifacts.
- Committed ME.V7 as `6c946753cf1d9ba7d4123e96511bd089a0b30423` with the required trailers.
- Completed the post-certification architecture audit: isolated synchronous SQL adapters from Tokio
  workers, composed the common durable coordinator, added atomic projection/event/effect/checkpoint/
  outbox journaling and leased work recovery, propagated scoped principals/capabilities through COBOL
  and CICS, recorded ADR 0005, and made architecture/runtime certification executable.
- Recorded focused issue commits `e860bdb`, `86f7199`, `93cf4f9`, `7ca6c79`, `a85171d`, and
  `7e3842a`; the post-audit workspace now passes 127 tests plus PostgreSQL 18 explicitly.

Exact machine evidence is in
`conformance/0.1/evidence/program-status.json` and
`conformance/0.1/evidence/entry.json`.

## Accepted decisions

- 0.1 covers only COBOL, CICS, JCL/JES, datasets, RACF/SAF, and 23 selected z/OSMF routes.
- The 43 R0 COBOL statement variants and 22 pinned CardDemo CICS operations form the finite
  language/transaction freeze; already unsupported oracle behavior stays explicitly unsupported.
- The proposed package map is consolidated to 20 packages where a separate package would not
  enforce an independent stable contract, provider selection, or application boundary; ADR 0005
  records the exact physical-to-logical mapping and future split triggers.
- OpenMainframe remains an out-of-process oracle and is never a production dependency.

## Known gaps and stop-the-line findings

No V0–V7 functional stop-the-line finding is open. Historical oracle attribution and explicit
unsupported surface are retained in
`conformance/0.1/inventory/known-gaps.json`; they do not create an alternate production route.

The 0.1.1 CardDemo-full target has no open implementation issue. All 20
journeys execute through public application, terminal, job, and operator
routes. Its entry and gap inventory are under `conformance/0.1.1/`; historical
0.1 evidence remains identifiable.

## Commands already run

All commands and exit codes are recorded in the machine status file. The unchanged oracle cohorts
passed: COBOL 37, RACF 8, z/OSMF datasets 9, z/OSMF jobs 9, CICS 317, JCL 164, and JES2 222 tests.

## Next smallest executable step

No implementation step remains. The 0.1.1 commit, artifacts, and annotated tag
are local. Publication, deployment, and remote push require separate owner
authorization.
