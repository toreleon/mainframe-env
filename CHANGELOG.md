# Changelog

All notable changes to mainframe-env are documented here.

## [Unreleased]

### Added

- Added verified offline IBM-documentation search/read commands and a cache-first
  source-review workflow for semantic development.
- Added repository contributor guidelines in `AGENTS.md` covering structure,
  development commands, coding conventions, tests, and pull request expectations.
- Added ADR-0011 and the first typed-HIR vertical slices: resolved COBOL
  arithmetic plans, typed CICS file/unit-of-work plans, versioned executable
  dialect identities, and bounded canonical plan codecs.
- Added candidate-bound product-source mutants for typed decimal receiver-local
  updates, operand capture, condition timing, rounding, and qualified
  `ADD CORRESPONDING`; unchanged exact-result tests must kill every
  representative mutant.
- Added a reviewed, pinned-source COBOL arithmetic pilot with independent
  golden bytes and typed-plan observations for `SIZE ERROR`, relative
  qualification, numeric-edited eligibility, and the no-pair no-op.
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
- Added digest-bound, HTML-only IBM source mapping, corpus projection, extraction,
  and implementation-independent automatic verification for all 263 CICS
  application commands in deterministic 88/88/87 batches. The accepted receipts
  contain 18,070 candidates: 16,307 objectively verified and 1,763 retained as
  bounded product ambiguity, with raw IBM publication bodies kept outside Git.
- Added a zero-credit, five-topic IBM source scope for the CICS task-enqueue
  slice. Alongside ENQ and DEQ it pins the command-level parameter page that
  establishes `DFHVALUE(TASK)=233` and `DFHVALUE(UOW)=246`, plus the ENQMODEL
  definition and global-enqueue tuning pages; publication bodies remain in the
  external content-addressed cache.
- Added typed local `EXEC CICS ENQ` and `DEQ` execution over a bounded durable
  lock catalog. The runtime preserves address-versus-content resource identity,
  nested UOW/TASK ownership, FIFO wait promotion, `NOSUSPEND`/active-handler
  `ENQBUSY`, syncpoint/task cleanup, atomic replay, and durable resume across
  memory, SQLite, and PostgreSQL store profiles.
- Added durable installed ENQMODEL definitions with bounded generic matching,
  local APPLID/SYSID isolation, nonblank-scope global serialization, disabled
  model abends, address-enqueue locality, atomic catalog installation, and
  restart validation.
- Added a zero-credit two-topic IBM source scope and typed execution for CICS
  `CHANGE TASK` and `SUSPEND`. Priority omission and `-1` remain no-ops,
  priorities `0..255` update the task and yield once, invalid values return
  `INVREQ` 16/1, and SUSPEND produces a one-shot durable scheduler handoff.
- Expanded the source-bounded CICS `ASSIGN` compatibility route with APPLID,
  SYSID, USERID, TASKPRIORITY, and the exact absent application/channel context
  defaults. It also reports absent CWA/TWA lengths, null OPERKEYS, and a normal
  task start without inventing state. The compiler enforces the 16-option limit
  and halfword/fullword/exact-width receivers, the provider rejects unknown or
  wrongly typed arguments, and local OPSECURITY/TCTUALENG defaults become exact
  DPL `INVREQ` 16/200 failures while unrelated requested outputs still populate.
  With no configured initialization parameter, INITPARM remains unchanged and
  INITPARMLEN returns halfword zero. PROGRAM is derived from the trusted current
  execution frame and follows durable HANDLE ABEND program transfers. Compiled
  local tasks with no pending next transaction receive four blanks from
  NEXTTRANSID, while DPL use returns `INVREQ` 16/200. BRIDGE returns four
  blanks because bridge-started tasks are outside the runtime. Compiled online
  tasks also receive exact zero ABOFFSET, instruction-interrupt, PSW, and
  register diagnostics because no recoverable ASRA-class machine-check handoff
  exists. They observe these values, the durable terminal's
  current/default/alternate screen geometry, and priority changes through the
  selected route. With no transaction-abend-control-block message, ERRORMSG
  and ERRORMSGLEN return 500 null bytes and halfword zero. Screen options fail
  with
  `INVREQ` 16/5 for nonterminal tasks and `INVREQ` 16/200 in DPL; FCI
  distinguishes the supported terminal facility (`X'01'`) from no facility
  (`X'00'`) and is also DPL-prohibited. LINKLEVEL returns one for a top-level
  local program and two for a DPL target behind its level-one mirror; unmodeled
  deeper local stacks fail closed. With no application partition set, PARTNSET
  returns six blanks on a terminal task and follows the local/DPL `INVREQ`
  matrix. The same owned virtual terminal reports a 3270 data stream and no
  basic SCS data stream; its unsupported optional device capabilities return
  false indicators, and its interactive session profile returns the attended
  indicator. CMDSEC and RESSEC return `X` because command admission and
  resource-owning operations use the platform's mandatory authorization routes.
  QNAME fails with exact `INVREQ` 16/4 because no task can be started by an ATI
  trigger, and with 16/200 in DPL. ACTIVITY, ACTIVITYID, PROCESS, and
  PROCESSTYPE fail with exact `INVREQ` 16/6 because no BTS activity path exists.
  DESTID and DESTIDLENG similarly report 16/3 before any BDI command and
  16/200 in DPL.
- Added online continuation format `MEOM4`, which retains the current task
  priority and an optional staged program-transfer handoff. Readers preserve
  `MEOM3` priority rows and historical `MEOM2` rows without that field.
- Added a zero-credit IBM source scope and typed CICS `SET ASSOCIATION
  USERCORRDATA`. The task-owned value overwrites with IBM's silent 64-byte
  truncation, enforces originating-task and command-security checks, and uses
  replay-bound `MECS5` session rows with `MECS1`–`MECS4` read compatibility.
- Added typed CICS `ADDRESS SET` for both documented COBOL directions. The
  compiler distinguishes pointer references from `ADDRESS OF` data areas, the
  provider validates only opaque storage identities, and the interpreter
  applies checked virtual aliases after successful audited dispatch.
- Added source-backed CICS `ABEND NODUMP` admission and explicit terminal dump
  disposition. Valid nonreserved ABCODE values request a dump, omitted or
  invalid codes and NODUMP suppress it, and retained older outcomes remain
  distinguishable as having no recorded dump decision.
- Added single-level CICS `HANDLE ABEND RESET` lifecycle semantics. Dispatching
  an active label automatically deactivates it, RESET reactivates the canceled
  label, bare HANDLE ABEND defaults to CANCEL, and conflicting action forms
  fail before execution.
- Added typed CICS `PUSH HANDLE` and `POP HANDLE` over a bounded 64-frame task
  stack. Nested frames suspend and restore condition/ABEND specifications,
  unmatched POP returns exact INVREQ behavior, and a compiled online route
  proves inner-to-outer exit restoration.
- Added typed CICS `IGNORE CONDITION` for 1–16 unique generated EIBRESP names.
  Ignored failures continue with the EIB set, HANDLE CONDITION overrides the
  matching ignore, PUSH/POP preserves it, and hostile lists fail before task
  state changes.
- Promoted CICS `HANDLE CONDITION` to typed execution for 1–16 generated
  EIBRESP names. One command atomically installs or deactivates every selected
  handler, specific actions precede generalized `ERROR`, and canonical or
  legacy duplicates and malformed labels fail before task state changes.
- Pinned a zero-credit CICS `HANDLE AID` source scope containing the exact
  command page and its linked BMS/DFHAID constant authority, reproduced through
  the existing Chrome session. This source receipt grants no execution or
  licensed differential credit.
- Added typed CICS `HANDLE AID` for the 34 source-named terminal AIDs, including
  optional-label deactivation, exact-over-`ANYKEY` precedence, the complete
  reached DFHAID byte set, PUSH/POP participation, and DPL `INVREQ` 16/200.
- Added durable `MECS7` CICS HANDLE state. Condition, AID, IGNORE, typed
  LABEL/PROGRAM ABEND exits, and nested PUSH/POP specifications now use session
  CAS, roll back on failed persistence, survive a terminal-input handoff and
  SQLite reopen, and clear when the task completes or recovery discards a
  non-handoff terminal task. `MECS6` label-only state remains readable.
- Extended the session state to `MECS8` with the latest explicit EXEC CICS
  ABEND code, dump request, and failing program. ASSIGN ABCODE, ABDUMP, and
  ABPROGRAM now survive handler and program-transfer handoffs and SQLite reopen;
  `MECS7` remains readable with no abend history.
- Added current-level CICS `HANDLE ABEND PROGRAM(name)` with exact local-program
  SAF and PGMIDERR checks, issuing-program COMMAREA transfer, CANCEL/RESET and
  PUSH/POP participation, a compiled two-program selected route, and recoverable
  artifact-bound execution handoff. Outward LINK-level search remains pending.
- Added the frozen-with-bounded-ambiguities CIC-901 command contract and generated
  263-row compiler registry. The registry explicitly separates three typed runtime
  handlers, 20 legacy compatibility handlers, and 240 unready handlers; automatic
  registration remains disabled and no default handler exists. The contract
  binds one 121-name EIBRESP condition authority and truthfully classifies the
  participant boundary as two known mutating rows, 260 bounded-effect rows, one
  explicit UOW boundary, and 261 bounded-UOW rows.
- Added candidate-aware EXEC CICS compiler recognition and fail-closed validation
  for source-reviewed command heads, COBOL applicability, option value shapes,
  discriminators, dependencies, alternatives, exclusions, and known source bounds.
  This seals only the non-release CIC-901 implementation boundary; it grants no new
  execution, coverage, or differential credit to unready commands.
- Added the first CIC-902 recovery guard: an owned execution-context binding
  rejects DPL `SYNCPOINT` without `SYNCONRETURN` or under `DPLSUBSET` with exact
  `INVREQ` RESP/RESP2 before unit-of-work mutation.
- Added a bounded remote-syncpoint outcome binding: a `SYNCONRETURN` DPL commit
  that the remote system cannot commit now rolls back local recoverable work,
  durably finalizes the rolled-back UOW, and returns exact `ROLLEDBACK` RESP 82
  with replay-safe behavior. A zero-credit selected-route regression drives the
  condition through typed COBOL, Conformance IR, the coordinator, and product
  providers.
- Implemented the source-defined `ABEND CANCEL` behavior on the existing task
  path: it cancels the active HANDLE ABEND exit before abnormal termination,
  persists no stale target, and is now admitted by the generated legacy option
  catalog. Dump disposition and typed task-control migration remain pending.
- Added a reviewed COBOL numeric `MOVE` pilot and corrected floating-insertion,
  capacity, sign, and overflow behavior found by that review.
- Added cost-aware local Jenkins assurance, exact-candidate command receipts,
  PostgreSQL parity helpers, and bounded dataset mutation checks.

### Changed

- Scoped local CI to the last successful ancestor, preserving policy/docs
  checks while skipping runtime rebuild/deployment for prose-only changes. Added
  per-command timings, bounded stage timeouts, and focused agent verification rules.
- Organized shared Git ignore rules for generated output, runtime state,
  environment secrets, and local tooling while retaining templates and evidence.
- Advanced decimal assignment to a policy-bearing `@2` contract with explicit
  arithmetic context, COBOL numeric-storage ABI, receiver-update, condition,
  and rounding behavior. The exact `@1` compatibility route remains readable,
  while an independent `ledger.formula@1` adapter proves bounded reuse without
  importing COBOL HIR.
- Made `mainframe.core.cobol@1.define` a dialect-owned semantic contract so
  legalization, artifact admission, and defensive VM admission reject malformed
  static layout ABI metadata before runtime construction.
- Corrected omitted-minimum ODO parsing, clause boundaries and phrase ordering,
  qualified ODO execution, and bounded key/index admission against the pinned
  grammar authority. Unsupported DYNAMIC table/alias combinations and TYPEDEF
  ODO objects now fail before publication; CONDITION and RENAMES associations
  no longer inherit REDEFINES-only constraints. Executable admission also checks
  nonnumeric category shapes, physical alias topology, RENAMES endpoints, and
  the bounded level-88 value subset shared with the frontend.
- Advanced artifact publication to `mainframe-env.artifact@3`; manifests now
  carry the exact dialect namespace/major set derived from their executable
  payload and bind COBOL arithmetic/display-sign/LP options to payload config,
  while historical `@2` artifacts retain their original bytes and contract.
- Made the release-smoke rejection test cover development hosts that are not
  advertised release targets, including native Linux ARM environments.
- Updated the 0.9 CICS implementation plan to require the integrated 28-finding
  hardening baseline, bounded family slices, per-slice security/recovery,
  explicit backend validation, and early licensed-campaign planning.
- Moved the 20 raw CICS compatibility routes' executable option subsets into a
  versioned runtime-admission catalog and the generated application registry,
  removing a handwritten compiler allowlist and rejecting catalog/runtime
  drift during generation. The move also removed the unreachable `ASSIGN
  TRANSID` entry, which is absent from the pinned application-command syntax.
- Normalized the verified ENQ/DEQ syntax so direct `UOW` and `TASK` lifetime
  forms remain distinct flags from `MAXLIFETIME(cvda)`. The generated registry
  now requires `RESOURCE`, resolves RESOURCE/MAXLIFETIME as inputs, and enforces
  the three lifetime spellings as mutually exclusive without advertising the
  still-unimplemented commands.
- CICS result handling now distinguishes a source-defined ignored condition
  from normal completion, updates `EIBFN` from the generated application row,
  and retains an ENQ suspension as the same durable online task until dequeue,
  timeout, or cancellation cleanup.
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

- Made legacy EXEC CICS compatibility routes reject source-valid options that
  their pre-typed runtime handlers do not implement, preventing silent operand
  drops while retaining the documented `DATASET` file-name compatibility alias.
- Moved the authentication wall-clock fixture wholly behind the server test
  boundary and tightened the reviewed product-module production-line ceiling.
- Anchored authentication-session expiry and rotation to the shared durable
  clock so wall-clock regressions cannot revoke valid sessions or bypass the
  cross-node per-user quota fence.
- Applied the idempotent PostgreSQL executable-artifact migration from both
  store entry points so fresh shared stores can persist schema-v2 metadata.
- Corrected typed ADD/COMPUTE receiver-local `SIZE ERROR` commits, resolved
  `ADD CORRESPONDING` by relative qualifiers with bilateral uniqueness and
  subordinate-item exclusions, and made selected-table compatibility preserve
  the requested occurrence while invalid or unrepresentable forms fail before
  publication.
- Moved decimal/CICS plan decoding, slot binding, and condition-topology checks
  into dialect-aware HIR/MIR/artifact verification, while retaining defensive
  machine admission.
- Persisted versioned executable manifests with installed artifacts and required
  manifest-aware admission before installed batch, online, nested-call, reload,
  or continuation execution.
- Routed the CICS file/UOW pilot through the durable execution coordinator and
  bounded arithmetic-expression grammar descent before typed HIR construction.
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
- Added the PostgreSQL writable-readiness rollback contract to the blocking
  parity stage so every environment-gated PostgreSQL correctness test is run.
- Kept full development certification runnable while preserving stable
  release-artifact checks behind an explicit release-candidate promotion.
- Required the archive CLI to verify its locked offline runtime before granting
  reproducibility credit.
- Kept Jenkins test temporaries below the excluded Cargo target tree so
  parallel fixtures cannot be mistaken for candidate repository contents.
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
- Corrected EXEC CICS condition-policy precedence so `RESP` continues to update
  its response area when combined with `NOHANDLE`, while `RESP2` still requires
  `RESP`, on both typed and legacy interpreter paths.
- Bound the existing time handler to `ASKTIME ABSTIME`, whose packed-decimal
  output it implements, and kept bare `ASKTIME` unready until EIBDATE/EIBTIME
  updates exist. Preserved only exact `INQUIRE PROGRAM` legacy SPI compatibility
  through a generated compiler descriptor without exposing other SPI commands.
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
