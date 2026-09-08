# Changelog

All notable changes to mainframe-env are documented here.

## [Unreleased]

### Added

- Added persistent bounded COBOL-parser and IR-decoder fuzzing, Loom schedule
  models with a negative concurrency mutant, and instrumented critical-package
  coverage reporting as receipt-backed full CI gates.
- Added a generated documentation manifest and bounded documentation gate via
  `cargo xtask docs --check` for navigation, normative metadata, links, anchors,
  command examples, and public version truth.
- Added ADR-0008 as the current authority for the 26-package workspace
  topology, superseding ADR-0005's historical count.
- Added a docs-driven CICS file/unit-of-work conformance pilot with reviewed
  rules, exact per-obligation observations, memory/SQLite execution, restart
  faults, and a fail-closed licensed-capture adapter.
- Added a reviewed COBOL numeric `MOVE` pilot and corrected floating-insertion,
  capacity, sign, and overflow behavior found by that review.
- Added cost-aware local Jenkins assurance, exact-candidate command receipts,
  PostgreSQL parity helpers, and bounded dataset mutation checks.

### Changed

- Updated the 0.9 CICS implementation plan to require the integrated 28-finding
  hardening baseline, bounded family slices, per-slice security/recovery,
  explicit backend validation, and early licensed-campaign planning.
- Added a blocking `missing_docs` ratchet for every contract crate, reduced the
  initial execution/store debt, and added runnable lifecycle/store examples.
- Split Db2, IMS, and MQ durable state into independently versioned object,
  index, cursor, unit-of-work, and replay rows with atomic legacy migration.
- Assigned post-0.8.2 work the distinct `0.8.3` development identity and made
  released versus development state explicit in every version authority.
- Replaced the live GitHub Actions assurance path with the capped local Jenkins
  workflow; hosted metadata remains historical rather than current evidence.
- Moved official catalog extraction to pinned IBM topic markup and strengthened
  locator, publication-byte, generated-registry, and source-review guards.
- Versioned canonical host-effect digests and tightened installed-call replay,
  live cancellation, deadline, provider-move, and DCOLLECT hardening after the
  0.8.2 tag.

### Fixed

- Persisted bounded typed host audit decisions with versioned canonical resource
  digests; made effect-result, lifecycle, outbox, and audit commits atomic; and
  made ordinary RACF authorization auditing fail closed and survive recovery.
- Derived batch host grants from the validated JCL plan and installed program
  registry, and required typed table/PSB/database/queue SAF authorization
  inside Db2, IMS, and MQ providers before any mutation.
- Routed installed online CICS programs through the durable, resume-aware
  execution journal. Per-session exchange identity now survives restart,
  unresolved effects return the explicit `unknown_outcome` gateway code, and
  file, transient-queue, program-link, and syncpoint replay is provider-ledger
  fenced before a recovered machine can continue. Pseudo-conversation
  checkpoints now close their old execution with an explicit durable handoff,
  and restart cleanup preserves only that handed-off continuation while
  retaining known terminal failure categories.
- Added bounded, transactional retention archives and saturation forecasts for
  lifecycle events, delivered outbox rows, resolved effects, and Db2/IMS/MQ
  replay receipts while protecting checkpoints and unresolved recovery state.
- Resolved PostgreSQL, TLS, bootstrap, and package secrets through one bounded
  reference provider; added named CLI overrides, secure first-administrator
  bootstrap with secret-free restart, and separate writable/auth/artifact/worker
  readiness checks backed by rolled-back provider-state DML proof, retention
  headroom, and per-worker queue-progress freshness.
- Moved synchronous z/OSMF backend calls to a bounded four-worker lane and
  propagated finite HTTP deadlines plus live cancellation into invocations.
- Classified mutating-effect journal failures after dispatch as unknown outcomes
  and added fenced, bounded stale-intent recovery across memory, SQLite, and
  PostgreSQL without redispatching the original mutation.
- Enforced PostgreSQL row/object quotas with transactional reservations, moved
  the PostgreSQL product profile to a shared immutable artifact store, and made
  local artifact publication no-replace and directory-durable.
- Moved JES execution out of HTTP submission into a bounded two-worker pool
  backed by a durable monotonic logical clock, generation-scoped FIFO claims,
  preserved JES priority, owner-bound execution contexts, periodic heartbeats,
  fenced lease recovery, and graceful worker shutdown.
- Filtered dataset catalog listings through a discrete SAF decision per name
  and derived pagination hints only from resources visible to the principal.
- Fenced every work-lease transition by a monotonic epoch and observed clock,
  clamped leases to deadlines, and prevented expired queued work from running.
- Made the offline Cargo archive reproducible twice in one digest-pinned GNU
  tar environment and made local and GitHub release assets immutable by digest.
- Bounded and zeroized transient authentication secrets, randomized and
  unified credential policy, and replaced durable raw bearer tokens with
  hashed, rotating, expiring sessions with a durable cross-server user quota
  and non-reusable principal-authentication-epoch revocation.
- Made schema discovery cover every versioned conformance directory and made
  CICS oracle imports validate the 0.9 schema before a closed typed origin is
  parsed or credited.
- Sealed the compiler's executable type-state chain and separated semantic
  artifact identity from the exact payload SHA-256 used by runtime references;
  the artifact contract is now `mainframe-env.artifact@2` and old compiler
  outputs must be rebuilt before execution.
- Replaced diagnostic provider replay identities and lifecycle outbox payloads
  with versioned canonical encodings, including fail-closed legacy
  reconciliation and credential-redacted RACF command digests.
- Unified execution-journal, effect, checkpoint, and artifact invariants across
  memory, SQLite, and PostgreSQL stores, with hostile-record rollback contracts.
- Replaced release-wide Cargo inventory with official-schema-validated
  CycloneDX 1.6 SBOMs for each exact target production closure, including the
  dependency graph; replaced unauthenticated local provenance with a signed
  DSSE in-toto Statement, SLSA Provenance v1 fields, a reviewed Jenkins builder
  identity, unique invocation identity, and tamper-failing verification.
- Expanded the Rust 1.95.0 gate to the complete workspace, all targets, and all
  features; locked Jenkins and external CI inputs immutably; and embedded exact
  supply-chain input identities in offline Cargo bundles.
- Added the complete Apache-2.0 project license and ICU attribution, generated
  deterministic full notices from each target production dependency closure,
  and made dependency-license policy a blocking CI and release gate.
- Made CI discover every shipped Python and shell tooling test, and expanded
  isolated PostgreSQL parity to cover durable migration and CardDemo restart
  suites.
- Made the macOS release build retain its required `LC_UUID` and required the
  exact target CLI and server binaries to pass launch, help, version, and
  readiness probes before release receipts can be written.
- Corrected RACF flat/nested syntax value handling and added generated-path
  regressions.
- Corrected Jenkins checkout/temp storage, tool selection, parameter handling,
  shell portability, and release-target selection.

### Known issues

- These `0.8.3` development changes are not part of the published 0.8.2 tag. The
  [pre-0.9 deep review](docs/reviews/PRE-0.9.0-DEEP-REVIEW.md) records the
  release-truth, durability, security, CI, and documentation blockers that must
  be resolved before 0.9.0 implementation and publication.

## [0.8.2] - 2026-09-06

### Fixed

- Retained installed COBOL program state within a run unit, observed live
  cancellation and deadlines, preserved unknown outcomes, and made installed
  child replay durable and stable without redispatching completed calls.
- Required verified HIR before executable lowering.
- Rejected incomplete, non-progressing, or changing DCOLLECT catalog traversals
  instead of publishing partial output.
- Applied one provider-state Move validation contract across memory, SQLite,
  and PostgreSQL.

### Evidence and distribution

- Separated observation perturbations from behavioral mutants in dataset
  certification schema `@2` and added an opt-in memory-store scaling benchmark.
- Published a locked offline Cargo vendor bundle and SHA-256 checksum after an
  offline workspace build. No 0.8.2 native binaries or binary receipts were
  published.

### Compatibility

- This patch changes runtime and contract behavior. Custom `WorkStore`
  implementations must add `get_work`; legacy MIR must be recompiled; and
  protocol-1 or counter-era in-flight installed calls require draining or
  explicit reconciliation before protocol 2.
- DCOLLECT can now fail where 0.8.1 returned partial output. Provider Move maps
  missing memory sources to `Conflict` and oversized SQL payloads to
  `PayloadTooLarge`. Dataset certification consumers must accept schema `@2`.

## [0.8.1] - 2026-09-05

### Fixed

- Preserved durable abend state and stable per-step effect identities across
  warm restart, preventing inverted `COND` handling and duplicate `DISP=MOD`
  appends in multi-step jobs.
- Preserved exact utility record boundaries, including empty records and data
  containing `0x0A`, without delimiter-based reconstruction.
- Serialized absent-dataset probe/create under a durable name reservation and
  retained the original abend when terminal DD cleanup also fails.
- Corrected omitted abnormal `DISP` defaults, CCSID-aware fixed-record padding,
  negative zoned edit signs, content-addressed spool rollback, JES queue use,
  de-hardcoding scan scope, and local artifact hygiene.

### Compatibility

- The program-input wire contract gains only an optional typed record map.
- 0.8.1 accepts 0.8.0 checkpoint identities. Queued and terminal durable jobs
  migrate through defaults; an in-flight legacy step without a replay base
  fails closed and can be resubmitted instead of risking a duplicate effect.

### Known limitations

- The licensed z/OS 3.2/JES2 differential remains exactly 0/16 pending under
  the approved `pass-with-licensed-differential-pending` policy. Hercules,
  MVS 3.8J, modeled behavior, local output, and generated or historical
  evidence receive zero licensed equivalence credit.
- The authentic licensed campaign remains a hard gate for 0.17 release
  certification and 1.0, while the standalone receipt adapter remains
  fail-closed.

## [0.8.0] - 2026-09-04

### Added

- Added deterministic JES2 scheduling, DD allocation and DISP processing,
  artifact-backed spool, output routing, started tasks, internal readers,
  bounded NJE/MAS topology, operator controls, and durable recovery.
- Added admission-pinned typed program registrations and real bounded semantics
  for all nine required utility families without program-name dispatch or
  generic-success fallback.
- Certified the pinned CardDemo base-batch corpus across 3 journeys, 12
  initialization jobs, and 9 operational jobs.

### Known limitations

- The licensed z/OS 3.2/JES2 differential remains exactly 0/16 pending under
  the approved `pass-with-licensed-differential-pending` policy. Hercules,
  MVS 3.8J, modeled behavior, local output, and generated or historical
  evidence receive zero licensed equivalence credit.
- The authentic licensed campaign is a hard gate for 0.17 release
  certification and 1.0, while the standalone receipt adapter remains
  fail-closed.

## [0.7.0] - 2026-09-02

### Added

- Added the lossless bounded JCL frontend, catalog-driven validation, procedure
  and symbol expansion, immutable typed planner, and JES2 JECL annotations.
- Added exact recognition and validation for all 237 pinned JCL/JES2 rows with
  deterministic malformed, recovery, scale, compatibility, and CardDemo plan
  matrices.

### Known limitations

- JCL row-wide execution, condition, recovery, and licensed differential gates
  remain pending; 0.7 is a converter/planner release, not the 0.8 JES runtime.
- CD-006 and CD-013 receipts are stale. Fixed-format comment text leaks into a
  multi-receiver `MOVE` during CardDemo online execution. IDCAMS rejects
  CardDemo's `DELETE ... CLUSTER` and `DATA/INDEX(NAME(...))` component forms,
  so the reproduced online and batch journeys fail. These defects are not
  hidden by weakened gates; 0.7.1 is the planned patch.

## [0.6.0] - 2026-09-02

### Added

- Added typed dataset, VSAM organization, catalog, GDG, allocation, locking,
  RLS/TVS, migration, backup/restore, and recovery semantics.
- Added generated execution for the 31-command AMS surface and local
  independent reference-model assurance across all 36 official rows.

### Known limitations

- The licensed z/OS 3.2 dataset/VSAM/AMS differential remains exactly 0/36 and
  is deferred to release certification.

## [0.5.0] - 2026-09-02

### Added

- Added the complete pinned RACF command and RACROUTE/SAF surface through one
  typed, durable, authorization-aware authority.
- Added audit redaction, migration/recovery, replay, restart, credential/MFA,
  certificate/keyring, and independent reference-model assurance.

### Known limitations

- The licensed z/OS 3.2 RACF/SAF differential remains exactly 0/48 and is
  deferred to release certification.

## [0.4.0] - 2026-09-02

### Added

- Added deterministic execution for the pinned COBOL statement, intrinsic,
  data, file, JSON/XML, condition, and recovery surfaces.
- Added checkpoint schema 10 and the bounded 16-case GnuCOBOL reference
  campaign as local assurance with zero licensed credit.

### Known limitations

- The licensed Enterprise COBOL 6.5 differential remains exactly 0/153 and is
  deferred to release certification.

## [0.3.0] - 2026-09-02

### Added

- Added the shared typed Conformance IR, deterministic shard/cache identities,
  replayable verdicts, and derived ledgers.
- Added complete recognition and validation coverage for the pinned 173-row
  COBOL structure and type-system inventory.

### Changed

- Generalized stable `0.x.y` release preparation and bound post-0.2 receipts to
  the clean live source tree while retaining immutable 0.2 evidence.

## [0.2.0] - 2026-09-02

### Added

- Added reviewed official coverage catalogs, generated registries, application
  packages, subsystem ABI libraries, and six independent evidence gates.

### Changed

- Authorized external promotion of the immutable accepted 0.2 candidate and
  its two retained target receipts; this branch does not create the tag or
  publish artifacts.

## [0.1.1] - 2026-08-31

### Added

- Accepted the bounded mainframe-env 0.1 greenfield product contract.
- Added the deterministic ME.V0 scope, oracle, profile, package, and evidence entry pack.
- Added Rust 2024 workspace, release-version authorities, and architecture/profile checks.
- Certified the generic COBOL, CICS, Db2, IMS, MQ, dataset, JES, RACF, restart,
  backup/restore, security, and overload capabilities with the complete
  CardDemo corpus. CardDemo remains conformance data and tooling, not a shipped
  application feature.
- Added owned, hash-pinned `COCRDSEC` demo source for the upstream `CDV1`
  orphan with an explicit no-card-data correction contract.

### Changed

- Promoted the product and all workspace crates to 0.1.1.
- Corrected corpus-tooling `CARDEMO` spellings to `CARDDEMO`. The typo in the
  pinned upstream FTP JCL remains only as an explicit compatibility alias.

## [0.1.0-alpha.0] - Unreleased

Initial development identity. This version is not published and makes no production-readiness claim.
