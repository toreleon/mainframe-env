# Execution Prompt — Implement mainframe-env 0.1 End to End

Copy the master prompt below into a coding-agent task opened at the
`mainframe-env` repository root.

This is a long-horizontal implementation program. It is intended to keep one
agent working across architecture, compiler, runtime, providers, persistence,
gateway, compatibility, and final cutover without stopping after planning,
scaffolding, one vertical slice, or a partially passing phase.

The current OpenMainframe repository is an executable compatibility oracle. It
is not a source dependency and is not the implementation base for
`mainframe-env`.

---

You are implementing **mainframe-env 0.1** as a greenfield Rust workspace. Work
from the `mainframe-env` repository root and continue until the complete 0.1
product contract passes or a genuine stop-the-line condition prevents further
safe progress.

The first release supports only:

- COBOL;
- CICS;
- JCL/JES;
- datasets;
- RACF/SAF security;
- z/OSMF route families backed by those domains; and
- compiler, execution, host-service, store, configuration, observability,
  conformance, and application infrastructure strictly required by that
  surface.

Do not migrate or implement unrelated current-workspace crates merely because
they exist. PL/I, REXX, CLIST, HLASM, Easytrieve, Natural, FOCUS, IMS, IDMS,
ADABAS, MQ, USS, ISPF, TSO product behavior, MVS experiments, DRDA, TN3270,
TUI, Wiki, symbolic execution, native/JIT backends, LLVM, Wasm/process plugins,
deployment generators, WLM product policy, and multi-node distribution are out
of scope for 0.1 unless an already accepted 0.1 selector has a small unavoidable
runtime dependency that cannot be satisfied by an owned in-scope contract.

Deliver working product behavior. Do not stop after creating architecture
documents, Cargo manifests, empty traits, DTO containers, generated schemas,
mock providers, `todo!()`, generic-success handlers, hard-coded fixture output,
or tests that prove only the implementation they contain.

Never fabricate source coverage, semantic parity, protocol compatibility,
security decisions, durability, resource bounds, production selection,
rollback, evidence, approval, or completion.

## 0. Long-horizontal execution contract

This task is intentionally larger than one context window. Operate as a
persistent implementation controller.

1. Read the normative documents completely before changing source.
2. Create and maintain both:
   - `docs/delivery/IMPLEMENTATION-STATUS.md` for human-readable progress; and
   - `conformance/0.1/evidence/program-status.json` for machine-readable phase,
     gate, command, and evidence status.
3. Record, at minimum:
   - current phase and work item;
   - completed deliverables and exact evidence paths;
   - accepted design decisions and unresolved decisions;
   - commands already run on the unchanged tree and their exit codes;
   - known gaps and stop-the-line findings;
   - next smallest executable step; and
   - source revision or deterministic dirty-tree identity.
4. At the start of every continuation or after context compaction, read the
   status files and current tree before planning. Continue from the recorded
   next step. Do not restart repository discovery or redo completed work.
5. Keep at most one phase-level implementation step in progress. Within that
   phase, batch closely related edits before validation.
6. Do not ask the user to approve ordinary in-scope implementation details.
   Request input only when a missing decision would materially change the
   public 0.1 behavior, compatibility promise, destructive migration, or
   security authority and cannot be derived from accepted documents or the
   frozen oracle.
7. Do not stop merely because a phase is difficult, the tree is large, a test
   fails, or a context window ends. Diagnose, repair the narrow failure, update
   the ledger, and continue.
8. Do not claim overall completion while any required 0.1 phase, profile gate,
   selector, fixture, schema, failure control, or cutover obligation remains
   incomplete.
9. Every completed phase `ME.V0` through `ME.V7` must end in the dedicated
   phase-completion commit defined by
   `docs/delivery/VERSIONING-AND-RELEASES.md`. The user's instruction explicitly
   authorizes these local commits. If the repository is not under Git, initialize
   a local repository during V0. Do not invent a remote, push, publish, create a
   release tag, or open a pull request without the corresponding authorization.
10. Use the product release line `0.1`; start at `0.1.0-alpha.0` and advance
    only through the version and release gates defined by ADR-0004.
11. Preserve user-owned changes. Never include unrelated user changes in a
    phase commit. Never delete or overwrite the current
    OpenMainframe oracle repository.

When the environment forces a yield before completion, persist the ledger
first. The next agent turn must be able to resume from files alone.

## 1. Meaning of “full 0.1”

“Full 0.1” is bounded by the accepted 0.1 profile, not by every feature IBM has
ever shipped and not by the full current OpenMainframe portfolio.

At entry, mechanically freeze:

- every current public selector advertised as supported, beta,
  compatibility-capable, or production-capable within COBOL, CICS, JCL/JES,
  dataset, RACF/security, and the selected z/OSMF route families;
- every COBOL construct, CICS command/subform, JCL/JES behavior, dataset form,
  RACF/SAF operation, and z/OSMF field/status/error contract reached by those
  selectors and accepted fixtures;
- every configuration key, source/precompiler input, utility program, protocol
  route, artifact, store, and provider dependency reached by that surface;
- every current default, fallback, bridge, mock/stub, analyzer, registry,
  mutable state authority, and direct subsystem access reachable from the
  frozen selectors; and
- every exact fixture/corpus, oracle command, source revision, external tool,
  environment input, redaction rule, and known legacy gap used to characterize
  the surface.

Freeze only the in-scope surface. Record every other current workspace package
once in `conformance/0.1/inventory/excluded-components.json` with reason
`excluded_from_v1`. Do not inventory out-of-scope packages selector by selector
or allow them into the 0.1 dependency graph.

A 0.1 selector is complete only when it has:

- one implemented deterministic default authority;
- stable typed input, result, diagnostic, failure, and resource contracts;
- required compiler/execution/host/store routes;
- exact normal, condition, failure, cancellation, timeout, malformed-input,
  authorization, overload, and provider-failure tests as applicable;
- bounded queues, state, output, payloads, recursion, frames, effects, and
  retention;
- compatibility or an explicit accepted correction against the frozen oracle;
- actual selected-route evidence rather than direct implementation tests only;
- restart/recovery behavior when it owns durable state;
- explicit unsupported behavior with stable diagnostics; and
- a cutover/removal decision for the old authority.

Do not reduce current in-scope support merely to make 0.1 smaller. A support
reduction requires an explicit compatibility decision in the 0.1 freeze. The
user's scope reduction excludes other components; it does not silently erase
supported behavior inside the selected components.

## 2. Authoritative inputs and precedence

Read these files completely before editing source:

1. `AGENTS.md` and every more-local instruction in `mainframe-env`.
2. `docs/CHARTER.md`.
3. Every document under `docs/architecture/`.
4. Every ADR under `docs/decisions/`.
5. Every delivery contract under `docs/delivery/`.
6. This prompt.
7. The current OpenMainframe repository's `AGENTS.md`, relevant crate READMEs,
   public entry points, tests, fixtures, accepted R0/R1/R2 decisions and
   evidence, and in-scope source implementations.
8. The complete current prompt
   `docs/prompts/IMPLEMENT_FULL_AUTHORITY_CONVERGENCE.md` only as a reference
   for evidence discipline, fail-closed gates, and long-program mechanics. Its
   R2A scope, phases, statuses, adoption categories, and portfolio obligations
   are not mainframe-env requirements.
9. External specifications and official framework documentation needed to
   resolve an in-scope technical question.

Use this precedence:

```text
explicit repository-owner mainframe-env 0.1 scope and naming decisions
-> accepted mainframe-env charter and ADRs
-> accepted mainframe-env architecture and delivery contracts
-> frozen 0.1 selector/fixture/profile inventory
-> accepted external compatibility specifications for those selectors
-> frozen current-workspace observations and historical evidence
-> current implementation details
-> this execution prompt
```

The user's decisions authorize greenfield implementation of mainframe-env 0.1,
the narrow 0.1 scope, and the `mainframe-env` identity. Record those decisions
truthfully. Do not fabricate independent architecture, security, operations, or
compatibility reviewers. A missing independent reviewer does not block ordinary
implementation unless an accepted gate explicitly requires that reviewer for a
production claim.

Historical OpenMainframe R0–R2 evidence remains historical. Do not rewrite,
relabel, or mutate it to make mainframe-env pass. mainframe-env has a separate
`conformance/0.1/` contract and evidence identity.

## 3. Entry gate and repository foundation

Before broad implementation, create a deterministic 0.1 entry pack:

```text
conformance/0.1/
  contract/
  inventory/
  schemas/
  fixtures/
  workloads/
  evidence/
```

The entry pack must contain:

- versioned 0.1 objective and scope contract;
- owner authorization and exact scope decision;
- `core-server` and `conformance` profile manifests;
- in-scope package/selector/construct/operation/route inventory;
- one compact out-of-scope component exclusion manifest;
- current OpenMainframe oracle revision, dirty-input identity, commands, and
  environment requirements;
- accepted external fixture/corpus content and license identities;
- known legacy gaps and disputed behavior;
- target package and dependency graph;
- architecture, compatibility, security, durability, and removal gates;
- deterministic generation commands; and
- a fail-closed entry result.

The entry gate passes only when:

- the 0.1 scope is finite and every in-scope selector is assigned exactly once;
- every target package is justified by an in-scope dependency or product
  boundary;
- no out-of-scope current package appears in a target production closure;
- the oracle can be reproduced or its unavailability is explicitly recorded
  without fabricating parity;
- contract and evidence schemas validate;
- required owners and decisions are recorded truthfully; and
- the next phase has exact deliverables and tests.

If the oracle is temporarily unavailable, continue implementing from accepted
specifications and fixtures where safe, mark parity pending, and do not promote
the affected selector or claim overall pass.

## 4. Architecture invariants

Implement and mechanically enforce these rules.

### 4.1 Deterministic core, asynchronous shell

Compiler passes, verifiers, codecs, semantic operations, and executable
machine transitions are synchronous deterministic functions over explicit
input/state. They do not depend on Tokio, Axum, SQLx, OpenTelemetry, filesystem
globals, wall clock, ambient randomness, or mutable application state.

The Tokio/Tower/Axum/SQLx shell owns networking, scheduling, backpressure,
cancellation, provider I/O, persistence, readiness, shutdown, and export.

The reference machine executes a bounded quantum and returns exactly one
logical drive action such as continue, host call, child invocation, transfer,
suspension, completion, or typed failure. It never awaits a provider or hides
normal control flow in an error.

### 4.2 Owned contracts

Stable and durable types are owned by mainframe-env. Axum, Tokio, Tower, SQLx,
Miette, Rowan, Rustls, tracing, Serde implementation details, and database row
types do not cross stable boundaries.

Use typed IDs and validated constructors. Deserialization alone must not create
an executable artifact, authorized principal, legal IR module, valid
checkpoint, or ready provider.

### 4.3 Dependency direction

Mechanically enforce:

```text
applications -> kernel + adapters
kernel       -> contracts + deterministic core
adapters     -> contracts
COBOL        -> source + diagnostics + IR/compiler contracts
interpreter  -> IR + semantic/CICS + execution/host contracts
JCL/JES      -> workflow/program/host contracts
providers    -> host contracts
contracts    -> foundation only
foundation   -> standard library and narrowly approved utilities
```

Prohibit frontend-to-backend, core-to-infrastructure, gateway-to-AST/provider
internals, engine-to-concrete-store, production-to-conformance, and any 0.1
dependency on excluded current packages.

### 4.4 Boundedness

Every queue, semaphore, mailbox, stream, parser recovery path, source bundle,
IR arena, diagnostic set, machine quantum, storage region, frame stack, effect
trace, output, dataset record, spool, job, session, cache, SQL pool, artifact,
checkpoint, event store, and retention policy has an enforced bound and a typed
exhaustion result.

Unbounded collections hidden behind an async channel, `Arc`, lock, iterator,
stream, or external library are prohibited.

### 4.5 State and durability

Every state type declares scope, owner, mutability, bound, durability,
checkpoint/safe point, schema version, security classification, and retry
behavior.

Delivery is at least once. Lease expiry does not prove an external effect did
not occur. Mutating effects require idempotency, a transaction, or an explicit
non-retryable/unknown-outcome policy.

### 4.6 Readability

- `lib.rs` contains crate documentation, module declarations, and re-exports.
- `pub(crate)` is the default visibility.
- Modules have one primary reason to change.
- Generated code is isolated and has a readable normative source.
- Scenario suites live outside production implementation modules.
- Every crate README states ownership, non-goals, invariants, allowed
  dependencies, public surface, and verification commands.
- [ADR-0010](../decisions/0010-rust-module-review-budgets.md) sets a hard
  1,200-production-line maximum for new/non-exempt Rust modules. Existing
  exceptions use exact non-growing ceilings and split only at stable reasons to
  change recorded in the machine budget inventory.
- Production code contains no `r0`, `r1`, `r2`, `r2a`, `wave`, `carddemo`, or
  other migration-program names as architectural boundaries.

## 5. Required program sequence

Execute phases in dependency order. A later phase may begin only when its
required earlier contracts are implemented and its entry tests pass. Complete
each phase's production path and evidence; do not accumulate a workspace of
empty future crates.

### ME.V0 — Scope, oracle, architecture, and workspace foundation

Deliver:

- accepted mainframe-env program identity and 0.1 scope in documents and
  machine contracts;
- owner-approved name, 0.1 scope, greenfield direction, `0.1` release line, and
  mandatory phase-commit decisions recorded as accepted without fabricating
  independent reviewer identities;
- complete entry pack from section 3;
- Rust 2024 workspace with pinned Rust 1.98.0 toolchain, declared
  `rust-version = "1.95"`, resolver 3, rustfmt, Clippy, and strict docs;
- local Git initialization when absent, without creating a remote, and the
  repository's active implementation branch according to local instructions;
- `VERSION` initialized to `0.1.0-alpha.0`, matching workspace package
  versions, plus `release.toml`, `CHANGELOG.md`, and `docs/releases/0.1.md`;
- product, crate, contract, schema, migration, and evidence version inventory;
- `cargo xtask versions --check` and machine-readable release-policy checks;
- only the package directories justified by the first executable phases;
- workspace dependency policy with minimal features;
- architecture/package/profile checks in `xtask`;
- deterministic code-generation and schema-check entry points;
- progress ledger and evidence status generator;
- CI skeleton that runs only real implemented gates; and
- no path or source dependency on current OpenMainframe crates.

The planned package map is defined in
`docs/architecture/PACKAGE-MAP.md`. If implementation evidence shows two
packages do not enforce a real boundary, consolidate them and update the ADR.
Do not preserve a proposed crate count for appearance.

ME.V0 passes when the new workspace builds its real foundation targets, the entry
gate derives pass, dependency/profile checks reject injected forbidden edges,
and no placeholder claims implementation completion.

### ME.V1 — Foundation, IR, compiler/execution/host/store contracts

Implement:

- exact source bytes, `SourceId`, logical paths, source formats, encoding, and
  expansion/provenance graph;
- stable diagnostics and execution problems with code, severity, phase,
  source span/provenance, category, completeness, help, and redaction metadata;
- CCSID/EBCDIC conversion and collation primitives required by 0.1 fixtures;
- deterministic typed-ID IR arenas, modules, regions, blocks, operations,
  values, storage references, locations, attributes, effects, and limits;
- dialect/type/operation identity and readable 0.1 operation catalogs;
- IR verification, canonical text form, versioned envelope, and owned binary
  codec with hostile-input limits;
- opaque compiler stages from source through verified HIR, legalized MIR, and
  publishable artifact;
- artifact semantic fingerprint and independent payload digest;
- invocation/context/limits/outcomes/events/run-unit/frame/suspension contracts;
- typed host requests/results for dataset, program, JES/spool, terminal,
  security/SAF, clock, audit, state, and 0.1 CICS effects;
- store interfaces for execution, events, work, checkpoints, sessions,
  artifacts, provider generations, and idempotency/effect records;
- bounded deterministic in-memory store implementations; and
- registry snapshots and capability selection needed by statically linked 0.1
  providers without implementing an external plugin ecosystem.

Require property and hostile-input tests for IDs, bounds, codecs, schema
versions, canonicalization, verifier rejection, store saturation, and immutable
snapshot publication.

ME.V1 passes only when contract/foundation crates have no infrastructure,
frontend, provider, application, current-workspace, or excluded-component
dependencies and cannot fabricate later-stage/executable state through public
constructors.

### ME.V2 — COBOL frontend, semantic model, HIR/MIR, and reference machine

Freeze every COBOL construct advertised by the accepted in-scope inventory. Do
not attempt the full IBM language outside that frozen surface.

Implement, wherever the frozen inventory requires it:

- fixed and free source formats, continuation, comments, UTF-8 and declared
  EBCDIC input, compiler directives, COPY/REPLACING, include search order, and
  exact expansion maps;
- handwritten bounded lexer, lossless CST, typed AST wrappers, parser recovery,
  stable diagnostics, and analysis/executable mode separation;
- divisions, sections, paragraphs, entry points, declaratives, nested source
  structure, and exact provenance;
- DATA DIVISION storage classes, groups/elements, PIC/USAGE, VALUE,
  levels 66/77/78/88, REDEFINES/RENAMES, OCCURS/DEPENDING ON, qualification,
  subscripts, reference modification, pointers where supported, file
  declarations, aliases, and layout bounds;
- DISPLAY, binary/COMP forms, COMP-3/COMP-5, decimal and integer conversion,
  signs, truncation, rounding, overflow/size error, figurative constants,
  encoding, and collation required by fixtures;
- MOVE, INITIALIZE, DISPLAY, ACCEPT, arithmetic/COMPUTE, conditions, IF,
  EVALUATE, SEARCH, PERFORM, GO TO/ALTER where supported, STRING/UNSTRING,
  INSPECT, file verbs, CALL/CANCEL/GOBACK/STOP RUN, return code, intrinsics,
  JSON/XML generation, and exception branches reached by the freeze;
- typed COBOL HIR with layouts, symbols, CFG, effects, diagnostics,
  completeness, and executable-support metadata;
- independent HIR verifier and schemas derived from one operation catalog;
- deterministic HIR-to-Core-MIR lowering organized by domain modules and one
  owned `LoweringContext`;
- full legalization preventing unknown/recovery/analysis-only operations from
  publication;
- deterministic reference machine for storage/aliases, values, decimal and
  byte operations, frames/calls, branches, conditions, file/program/terminal
  effect requests, output, limits, cancellation, and explicit terminal
  outcomes; and
- compile, emit/explain, execute, and inspect application services through the
  common compiler/execution contracts.

The machine is the only 0.1 backend. Do not add symbolic, Cranelift, LLVM, Wasm,
JIT, or silent legacy fallback.

For each COBOL cohort:

1. freeze exact source/dependency/options/encoding inputs;
2. compile through the public mainframe-env compiler service;
3. run the current oracle independently where available;
4. compare diagnostics, layouts, storage, aliases, source control flow,
   effects, output, return/condition/control outcomes, and resource counters;
5. test malformed, unsupported, overflow, bound, cancellation, and provider
   failure behavior;
6. record intentional legacy corrections separately; and
7. prove the selected route rather than only invoking private functions.

ME.V2 passes when every frozen non-CICS COBOL cohort compiles and executes through
the new reference path, executable legality is fail-closed, exact fixtures pass
or carry accepted correction decisions, and no old implementation code or
dependency is linked.

### ME.V3 — Dataset and RACF/SAF security authorities

Implement only the dataset and security surface required by frozen COBOL,
CICS, JCL/JES, and z/OSMF selectors, but implement that surface completely.

Dataset deliverables:

- canonical dataset, member, DD, volume/catalog, and record identities;
- declared organization, record format, logical record length, block/extent,
  key, encoding, disposition, access, and lock semantics required by 0.1;
- sequential, partitioned/member, and selected VSAM-style behavior reached by
  accepted fixtures;
- catalog/list/attribute/read/create/write/rename/delete/member operations
  reached by accepted z/OSMF and program paths;
- exact record/key bytes, conditions/status, browse/update guards, transaction
  ownership, idempotency, cancellation, deadlines, quotas, and audit;
- DD binding resolution for JCL/program execution;
- provider-private local storage layout with no physical path in public
  contracts; and
- deterministic in-memory plus durable local SQL/filesystem adapter behavior.

RACF/SAF deliverables:

- principal, user, group, credential-reference, session, and security-context
  contracts;
- authentication success and stable invalid/expired/revoked/locked/failure
  categories required by 0.1;
- dataset and general-resource profiles, ownership, groups, permissions,
  generic matching, and SAF request/decision semantics reached by fixtures;
- authorization at admission and again at sensitive dataset, JES, CICS, and
  administrative host boundaries;
- administrative mutations admitted by 0.1 with transaction/idempotency,
  audit, rollback, redaction, bounds, and failure behavior;
- versioned provider persistence and configuration; and
- default deny for missing grants, profiles, provider, or policy failure.

Host-service middleware must enforce principal, grant/interface version,
execution/run-unit/session/transaction identity, deadline, cancellation,
request/result size, idempotency/effect sequence, tracing, and audit before
provider invocation.

No caller receives dataset catalog internals, RACF database handles, unrelated
locks, raw credentials, or broad application state.

ME.V3 passes when every frozen dataset/security selector uses typed host services,
normal/condition/denial/failure/cancellation/idempotency/restart tests pass,
provider state is isolated, and no authorization failure can become success.

### ME.V4 — Typed CICS runtime and interactive execution

Mechanically freeze every CICS command and subform reached by supported 0.1
COBOL/CICS selectors. Do not limit coverage to the historical seven-operation
canary when the accepted 0.1 inventory advertises more.

Implement typed contracts and behavior, wherever reached, for:

- BMS map/mapset definitions, fields, positions, attributes, colors,
  highlights, cursor, erase/data/map/free-keyboard options, AID, modified data,
  input length, secret classification, SEND MAP, SEND TEXT, and RECEIVE MAP;
- file READ/READ UPDATE, WRITE, REWRITE, DELETE, STARTBR, READNEXT/READPREV,
  ENDBR, exact dataset/key/record bytes, guards, conditions, and effects;
- CALL/LINK/XCTL/RETURN, COMMAREA/channel, replacement versus child frame,
  transfer target, next TRANSID, suspension/resume, and run-unit behavior;
- HANDLE/IGNORE CONDITION, RESP/RESP2/NOHANDLE, HANDLE ABEND, ABEND, route
  activation/reset/cancel, EIBRESP/EIBRESP2, APPLID/SYSID, and supported ASSIGN;
- supported temporary/transient data or other CICS-local services reached by
  frozen fixtures without importing out-of-scope MQ or subsystem crates;
- transaction ownership, sync/rollback behavior required by 0.1 mutations;
- RACF/SAF principal, capability, resource, and audit integration;
- typed ordered effect traces with exact request/result/storage/condition
  observations and configured secret redaction; and
- limits, cancellation, timeout, missing provider/grant, resource exhaustion,
  provider panic/failure, and infrastructure failure.

COBOL `EXEC CICS` parsing, typed HIR, verification, lowering, runtime imports,
and machine dispatch must use one operation catalog and complete provenance.
Raw command strings, global bridges, renderer/TUI state, and direct dataset or
RACF internals are prohibited production authorities.

Suspended terminal sessions persist bounded protocol-neutral state and consume
no dedicated idle CPU worker or OS thread.

Differential tests run current and mainframe-env routes from independent clones
of the same frozen dataset/map/security/session state. Compare exact internal
bytes before evidence redaction. Shadow comparison cannot duplicate an external
mutation.

ME.V4 passes when every frozen CICS form has one typed provider route, exact
normal/condition/failure observations, authorization and transaction behavior,
bounded suspend/resume, and selected-route compatibility evidence without a
string or legacy fallback.

### ME.V5 — JCL workflow and JES job authority

Freeze every JCL/JES construct, utility selector, status, and spool behavior
advertised by the accepted 0.1 inventory.

JCL deliverables:

- bounded source/lexer/parser and diagnostics;
- JOB, EXEC, DD, PROC/PEND, INCLUDE where supported, cataloged/in-stream
  procedures, symbols and override/substitution semantics;
- step order, program/procedure selection, parameters, DD bindings,
  disposition, temporary datasets, continuation, comments, and source
  provenance;
- COND, return-code tests, abnormal termination, restart/skip semantics reached
  by fixtures;
- typed workflow/step plan with explicit dataset, program, spool, security, and
  lifecycle effects; and
- no direct instantiation of COBOL, utility, dataset, or JES implementations.

JES deliverables:

- job, step, execution, owner, class, priority, status, queue, and spool
  identities;
- submit/admit/queue/run/complete/fail/cancel/purge lifecycle;
- JOBLOG, JESMSGLG, JESJCL, SYSOUT/SYSPRINT and accepted spool metadata/content;
- all `EXEC PGM=` resolution through `ProgramService` and the common execution
  coordinator;
- COBOL batch execution and only those utilities directly reached by accepted
  0.1 fixtures, implemented as in-scope program capabilities rather than
  migrating an unrelated utility portfolio wholesale;
- dataset DD allocation/disposition and RACF/SAF checks through host services;
- bounded job queues, active steps, spool bytes/files, output, events,
  retention, and purge;
- cancellation, timeout, missing program/provider, condition, ABEND, overload,
  provider/store failure, and restart/recovery; and
- one durable authoritative job state rather than handler-local maps.

ME.V5 passes when accepted JCL/JES fixtures use one typed workflow and program
path, job/spool/dataset/security outcomes match or have accepted corrections,
restart does not lose authoritative queued/terminal work, and no generic-success
utility or alternate job authority is reachable.

### ME.V6 — z/OSMF gateway and durable single-node product

The 0.1 z/OSMF surface includes only route families backed by the accepted 0.1
domains. Freeze exact supported routes, methods, paths, query/body formats,
authentication, fields, pagination, status codes, headers, and error bodies for:

- server/system information;
- jobs, status, spool, submit, cancel, and purge;
- datasets and members;
- accepted RACF/security or console operations that can be implemented entirely
  by 0.1 authorities; and
- health/readiness and version/capability information.

DB2, DRDA, TSO product, IMS, MQ, deployment, Wiki, TUI, and other out-of-scope
route families are not advertised or started in the 0.1 profile. Their absence
must be explicit, not generic success.

Implement:

- Axum handlers as thin protocol translators over application services;
- request/body/output limits before expensive work;
- authentication and stable principal propagation;
- route-specific compatibility DTOs isolated from domain DTOs;
- Tower middleware for tracing, timeout, concurrency, overload, and response
  mapping in correct order;
- one versioned product configuration schema with explicit file/environment/
  CLI precedence and provider-private namespaces;
- readiness derived from required store/provider/kernel health;
- graceful shutdown with admission stop, cancellation, drain, checkpoint, and
  bounded deadline;
- tracing spans/events and stable operational metrics without exposing
  OpenTelemetry types to contracts;
- SQLx SQLite local and PostgreSQL 18 production store adapters;
- embedded, versioned, expand/contract SQL migrations;
- execution/event/work/checkpoint/session/generation/idempotency durable state;
- immutable local/object-store artifacts with integrity, conditional creation,
  retention, and garbage-collection policy;
- append-only execution/audit events plus transactional current projections;
- transactional outbox and bounded reconstructible local wakeups;
- lease, heartbeat, attempt, expiry, dead-letter, and recovery behavior for one
  active worker pool;
- effect intent/result and unknown-outcome reconciliation; and
- backup/restore, store saturation, restart, and recovery runbooks/tests.

The 0.1 product remains single-node/one-active-worker-pool. Do not add NATS,
Kubernetes, remote workers, or multi-node placement. Durable interfaces must be
capable of later adapters without claiming later distribution support.

ME.V6 passes when `core-server` contains only 0.1 packages, all accepted z/OSMF
routes use the common authorities, gateway/coordinator restart does not lose
authoritative queued/suspended/terminal work, overload remains bounded, and
SQLite/PostgreSQL profile behavior passes its declared compatibility and
recovery gates.

### ME.V7 — Certification, default authority, and cutover

Complete the full 0.1 verification and cutover contract.

Require:

- complete in-scope inventory with zero unassigned selector/construct/route;
- exact package/profile dependency closure with zero excluded component;
- one default authority for compile, execute, CICS effects, JCL/JES jobs,
  datasets, security, configuration, stores, artifacts, and z/OSMF routes;
- full selected-route normal/condition/failure/cancellation/timeout/overload/
  malformed/authorization/provider/store/restart evidence;
- COBOL, CICS, JCL/JES, dataset, RACF, and z/OSMF differential reports;
- accepted correction records for intentional legacy fixes;
- property, fuzz, model, concurrency, migration, and schema compatibility
  evidence required by `docs/delivery/VERIFICATION-STRATEGY.md`;
- release-mode mixed interactive/batch/API load at 1x and 2x offered load;
- long-run memory/task/thread/session/permit/handle/artifact/spool/checkpoint
  leak evidence;
- backup/restore and crash-point/idempotency exercises;
- TLS/security/supply-chain/advisory/license review;
- reproducible configuration, API, crate, operation, and support
  documentation;
- explicit canary and 0.1 default selection evidence;
- rollback rehearsal where retained oracle/previous authority and state
  compatibility make rollback truthful; and
- final archive/removal of the old implementation from the mainframe-env
  production dependency closure.

There is no requirement to migrate out-of-scope packages before 0.1 pass. They
must remain absent and unadvertised.

ME.V7 passes only when the generated final gate proves the complete accepted 0.1
surface and mainframe-env is the sole default 0.1 authority with no hidden old
fallback.

## 6. Phase commits, versioning, and release gates

Follow ADR-0004 and `docs/delivery/VERSIONING-AND-RELEASES.md` exactly.

### 6.1 Mandatory phase commits

After each phase gate derives pass, update the ledgers/evidence, review the
staged phase diff, and create exactly one dedicated completion commit with the
required subject:

| Phase | Commit subject |
|---|---|
| ME.V0 | `Complete ME.V0 workspace foundation` |
| ME.V1 | `Complete ME.V1 foundation contracts` |
| ME.V2 | `Complete ME.V2 COBOL execution path` |
| ME.V3 | `Complete ME.V3 dataset and security authorities` |
| ME.V4 | `Complete ME.V4 typed CICS runtime` |
| ME.V5 | `Complete ME.V5 JCL and JES batch path` |
| ME.V6 | `Complete ME.V6 durable z/OSMF product` |
| ME.V7 | `Complete ME.V7 certification and cutover` |

Each commit body includes:

```text
Phase-Gate: ME.Vn=pass
Evidence-Digest: sha256:<canonical phase content digest>
Product-Version: <VERSION>
```

Do not create a completion commit for a failing phase. Focused intermediate
commits are allowed but do not replace the phase commit. Do not stage unrelated
user changes. If user changes overlap required phase files and cannot be
preserved safely, stop and request direction rather than absorbing or
overwriting them.

Use a canonical phase input/content digest that excludes Git metadata and
self-referential receipt fields. The phase commit supplies Git identity. Do not
edit committed evidence only to insert the hash of the commit containing it.

### 6.2 Version authorities

Maintain and mechanically cross-check:

```text
VERSION
[workspace.package].version
release.toml
CHANGELOG.md
docs/releases/0.1.md
contract/interface/dialect version inventory
configuration/artifact/checkpoint/session versions
SQL migration head
conformance/evidence schema versions
```

Shipped 0.1 crates use the product version in lockstep. Public/durable contract
versions remain independent from the product version and cannot be inferred
from `0.1.x`.

Once `0.1.0` is final, patch releases `0.1.x` must remain compatible with every
surface classified supported in 0.1. Breaking supported behavior requires
`0.2.0` or an independently versioned major contract with an explicit upgrade
and compatibility plan.

### 6.3 Release promotion

Do not bump a channel or create a release tag merely because implementation
reached a phase number.

- `0.1.0-alpha.N` requires ME.V0–ME.V4 plus the working public COBOL/CICS,
  dataset, and security vertical path.
- `0.1.0-beta.N` requires ME.V0–ME.V6 plus the complete frozen 0.1 functional
  surface, clean profile closure, durable store/restart behavior, and public
  compatibility documentation.
- `0.1.0-rc.N` requires ME.V0–ME.V7 functional gates, a clean candidate,
  complete release validation, long-run/overload/leak/recovery/rollback
  evidence, reproducible packages, SBOM, checksums, provenance, migrations,
  and release notes.
- final `0.1.0` requires clean-checkout reproduction of the RC, matching release
  manifest/artifact digests, final security/license/advisory/profile gates,
  truthful owner/cutover decisions, and generated `MAINFRAME-ENV 0.1 = PASS`.

Version changes occur in dedicated release-preparation commits. Annotated local
tags use `mainframe-env-v0.1.0-<channel>.N` and
`mainframe-env-v0.1.0`, but create a tag only after its gate and explicit
release action are authorized. Never push or publish without authorization.

### 6.4 Release artifacts

Prepare release manifests and, where applicable, server/CLI binaries, profile
manifest, configuration/schema, SQL migration bundle, compatibility catalogs,
API/crate docs, SBOM, license notices, checksums, provenance, evidence summary,
known limitations, and upgrade/backup/restore/rollback/capacity runbooks.

Each artifact maps to source commit, canonical content digest, toolchain,
target, profile/features, dependency lock, contract inventory, and SHA-256.
Release packages must not contain current OpenMainframe implementation code,
oracle binaries, test harnesses/datasets, raw credentials, local absolute paths,
or uncontrolled raw evidence.

## 7. Evidence and derived gates

Keep evidence compact, deterministic, reviewable, and phase-specific.

At minimum create:

```text
conformance/0.1/contract/
  release-contract.json
  owner-authorization.json

conformance/0.1/inventory/
  profiles.json
  packages.json
  selectors.json
  cobol-constructs.json
  cics-operations.json
  jcl-jes-coverage.json
  dataset-coverage.json
  racf-saf-coverage.json
  zosmf-routes.json
  dependency-graph.json
  authority-graph.json
  state-catalog.json
  excluded-components.json

conformance/0.1/schemas/
  evidence and result schemas

conformance/0.1/fixtures/
  manifests and immutable fixture identities

conformance/0.1/workloads/
  correctness, failure, resource, restart, and recovery workloads

conformance/0.1/evidence/
  entry.json
  phase-v0.json ... phase-v7.json
  differential/
  resource-summary.json
  security-summary.json
  recovery-summary.json
  profile-closure.json
  program-status.json
  release-exit-gates.json
  MAINFRAME_ENV_0_1_EXIT_REPORT.md
```

Large raw resource, fuzz, and long-run artifacts belong in bounded CI/release
artifacts or a documented content-addressed location. Checked-in evidence keeps
the exact identity, digest, command, runner, limits, and derived summary without
committing uncontrolled generated noise.

Every generated result carries:

- schema and contract versions;
- mainframe-env source revision or dirty-input identity;
- oracle revision/dirty identity where used;
- generation command and exit code;
- profile, selector/cohort, source/fixture/config identity;
- compiler/interpreter/provider/store generations;
- canonical options and limits;
- contributing paths and hashes;
- support, completeness, authority, failure, and rollback state; and
- known gaps and accepted corrections.

Derive phase and final status from contributing evidence. Do not hard-code pass.

At minimum:

```text
me_v0_pass = scope_finite
       && in_scope_selectors_assigned_once
       && excluded_components_absent
       && entry_contract_valid

me_v1_pass = foundation_contracts_complete
       && dependency_direction_clean
       && codecs_verifiers_bounds_pass

me_v2_pass = all_frozen_cobol_constructs_implemented_or_explicitly_unsupported
       && all_executable_cobol_cohorts_use_verified_hir_legalized_mir
       && reference_machine_selected

me_v3_pass = dataset_authority_typed_complete
       && racf_saf_authority_typed_complete
       && authorization_failure_closed

me_v4_pass = all_frozen_cics_forms_typed
       && cics_provider_is_default
       && no_string_or_global_bridge_authority

me_v5_pass = all_frozen_jcl_jes_forms_implemented
       && every_exec_pgm_uses_program_service
       && one_job_spool_authority

me_v6_pass = all_frozen_zosmf_routes_use_release_0_1_services
       && durable_single_node_recovery_passes
       && core_profile_has_no_excluded_dependency

me_v7_pass = all_phase_gates_pass
       && compatibility_security_resource_recovery_pass
       && authority_unique
       && cutover_complete

mainframe_env_0_1_pass = me_v0_pass && me_v1_pass && me_v2_pass && me_v3_pass
                     && me_v4_pass && me_v5_pass && me_v6_pass && me_v7_pass
```

The generator fails when an in-scope row lacks implementation, fixture,
selected route, evidence, owner, or disposition; when an excluded dependency is
reachable; when two default authorities exist; when mock/generic success is
reachable; when evidence is stale; or when a required bound/failure/recovery
result is absent.

Evidence prose cannot override machine gate data.

## 8. Validation and CI discipline

Keep validation proportional during implementation and broad at milestones.

During a phase:

- run the narrowest named test or module that proves the edit;
- batch related edits before rerunning;
- when a command fails, repair and rerun the smallest affected command first;
- do not repeat a successful broad command on an unchanged tree; and
- record successful commands in the progress ledger.

Before each phase gate, run the affected package/profile suites and generation
checks once on the stable candidate.

Before final 0.1 completion, run at minimum, adjusted only for actual workspace
targets:

```text
cargo fmt --all -- --check
cargo check --workspace --all-targets --all-features --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo build --workspace --all-features --locked
cargo test --workspace --all-features --locked --no-fail-fast
RUSTDOCFLAGS='-D warnings' cargo doc --workspace --all-features --no-deps --locked
cargo xtask architecture --check
cargo xtask profiles --check
cargo xtask versions --check
cargo xtask schemas --check
cargo xtask inventory --check
cargo xtask conformance
cargo xtask evidence --check
<all 0.1 differential commands>
<all 0.1 security/resource/restart/recovery commands>
<SQLite and PostgreSQL migration/compatibility checks>
<core-server package/closure check>
git diff --check                  # when the repository is under Git
```

Also verify:

- Rust MSRV 1.95 and pinned Rust 1.98 across the full workspace, all targets,
  and all features;
- core-server and conformance closures separately;
- release builds on advertised operating systems;
- SQLite local and PostgreSQL production profiles;
- deterministic regeneration from a clean or equivalent source snapshot; and
- bounded raw evidence artifact publication;
- phase completion commit and trailers for every passed phase; and
- release manifest/SBOM/checksum/provenance/package checks at release gates.

Do not add CI jobs for out-of-scope frameworks or components.

## 9. Stop-the-line conditions

Stop the affected selector or phase, retain the last safe non-production state,
and report the exact blocker when:

- the finite 0.1 scope or owner decision is missing for an in-scope behavior;
- an in-scope supported selector disappears without an explicit compatibility
  decision;
- passing depends on importing or linking current OpenMainframe implementation
  crates;
- a phase is only schemas, empty traits, Nops, mocks, fixture-specific output,
  generic success, or automatic fallback;
- a frontend AST becomes a backend, provider, gateway, store, or stable plugin
  contract;
- analysis/recovery/unknown/illegal IR can publish or execute;
- lowering loses storage, aliases, layout, decimal/encoding, conditions,
  effects, source/provenance, output, or control meaning;
- a CICS form still reaches raw string/global bridge authority;
- a JCL step bypasses ProgramService or JES job authority;
- a z/OSMF handler owns job, dataset, security, or compiler business state;
- a dataset or RACF caller receives broad provider internals or raw credentials;
- authorization, missing provider, policy error, or mock behavior can become
  success;
- two automatic default authorities exist for an in-scope selector;
- a mismatch is normalized, redacted, hashed, omitted, or reclassified before
  exact internal comparison;
- shadow/differential execution can duplicate an external mutation;
- effect retry can silently duplicate a business mutation or discard an
  unknown outcome;
- checkpoint/artifact/config/store migration silently accepts incompatible
  state;
- a queue, cache, stream, mailbox, session, output, spool, record, artifact,
  event, checkpoint, task, thread, or connection pool is unbounded;
- cancellation, timeout, panic, provider/store failure, overload, restart, or
  rollback leaks a permit, handle, principal, state, or effect;
- an out-of-scope package enters the 0.1 production or conformance closure
  without an accepted ADR;
- a passed phase lacks its required completion commit, evidence digest trailer,
  or product version trailer;
- `VERSION`, Cargo workspace/crate versions, `release.toml`, release notes,
  contract inventory, or migration head disagree;
- a release channel/tag is prepared without its derived gate, clean source,
  reproducible artifacts, or authorization;
- a patch in the 0.1 line silently breaks a supported public/durable contract;
- a gate, approval, measurement, compatibility result, source identity, or
  zero-result report is hard-coded or fabricated; or
- completion requires a destructive migration or public compatibility decision
  that the accepted documents do not authorize.

A stop-the-line finding blocks only the affected promotion where meaningful
independent work remains. Continue safe foundation, tests, documentation, or
other in-scope cohorts that do not depend on the blocked decision. Overall pass
remains impossible until the blocker is resolved.

## 10. Scope exclusions are not blockers

Do not report 0.1 blocked because an excluded component is unimplemented,
unowned, incompatible, untested, or absent. Excluded components are outside the
program.

Do report a blocker when an accepted 0.1 selector secretly depends on an
excluded component and no in-scope contract/provider can satisfy that behavior.
In that case, identify the exact selector and smallest dependency decision; do
not migrate the entire excluded crate by default.

## 11. Final response contract

The final response must begin with exactly one truthful outcome:

```text
MAINFRAME-ENV 0.1 = PASS
MAINFRAME-ENV 0.1 = FAIL
MAINFRAME-ENV 0.1 = BLOCKED
```

Use `PASS` only when the generated `mainframe_env_0_1_pass` gate is true on the
reported source identity and no required work remains.

Then report:

- mainframe-env product version/channel, contract version inventory, source
  revision or dirty-input identity, and owner authorization;
- frozen 0.1 profiles, packages, selectors, COBOL constructs, CICS forms,
  JCL/JES coverage, dataset/security operations, and z/OSMF routes;
- excluded-component count and proof they are absent from 0.1 closures;
- V0–V7 result with exact gate derivation;
- ME.V0–ME.V7 completion commit hashes, required trailers, and phase evidence
  digests;
- compiler/IR/reference-machine coverage and explicit unsupported surface;
- dataset and RACF/SAF authority, durability, authorization, and failure
  results;
- typed CICS coverage, terminal/file/program/condition/transaction behavior,
  and confirmation that no string/global bridge authority remains;
- JCL/JES workflow, program dispatch, job/spool lifecycle, restart, and utility
  coverage;
- z/OSMF route compatibility and gateway-thinness result;
- SQLite/PostgreSQL/artifact/checkpoint/event/idempotency/recovery results;
- semantic differential, accepted corrections, malformed-input, property,
  fuzz, model, concurrency, security, resource, overload, leak, restart, and
  backup/restore evidence;
- exact validation commands and exit codes, reusing recorded results rather
  than rerunning unchanged gates;
- canary/default/cutover/rollback/archive status;
- release-gate status, `VERSION`/workspace/release-manifest consistency,
  candidate/final tag state, SBOM/checksum/provenance/package identities, and
  confirmation that no remote publish occurred without authorization;
- known gaps and blockers with affected selectors and safe current state; and
- exact evidence/report paths and hashes.

If some phases pass and others do not, report them individually, persist the
progress ledger, keep overall status non-pass, and identify the next smallest
safe executable step. Do not describe a partial implementation as complete 0.1.

---
