# 0.1 Greenfield Implementation Roadmap

Status: **Accepted by repository owner**

This roadmap replaces R2A as the active engineering sequence. It does not
reinterpret historical R0–R2 evidence.

The roadmap delivers the `0.1` release line. Every completed phase ends in the
required Git commit from
[Versioning, Phase Commits, and Release Gates](VERSIONING-AND-RELEASES.md).

## ME.V0 — Scope, oracle, and architecture freeze

Deliver:

- accept this charter and ADR set;
- freeze 0.1 selectors and fixtures only;
- create the out-of-scope exclusion manifest;
- create current-workspace oracle commands;
- approve package boundaries and dependency rules;
- scaffold the independent Rust 2024 workspace; and
- add architecture/profile checks before product implementation.

Exit: every in-scope selector is assigned; no out-of-scope crate appears in the
0.1 dependency plan.

## ME.V1 — Foundation and contracts

Deliver:

- source bytes, encoding, identities, and provenance;
- diagnostic/problem model and rendering adapter;
- IR object model, verifier primitives, text/binary envelopes;
- compiler stages and artifact contract;
- execution context, outcomes, limits, events, and machine contract;
- typed host services for dataset, terminal, program, spool/JES, security,
  clock, audit, and CICS; and
- store contracts plus bounded in-memory implementations.

Exit: contract crates have no infrastructure or current-workspace dependencies;
property/roundtrip/limit tests pass.

## ME.V2 — COBOL compiler and reference interpreter

Deliver:

- fixed/free source, COPY expansion, lossless syntax, and typed AST;
- semantic names, layouts, aliases, decimals, encoding, and control flow;
- verified COBOL HIR and fully legalized Core MIR;
- reference machine with storage, calls/frames, conditions, output,
  cancellation, and bounds; and
- accepted non-CICS COBOL fixtures through compile and execute.

Exit: accepted COBOL fixtures pass differential semantics without using old
implementation code.

## ME.V3 — Dataset and RACF/security providers

Deliver:

- dataset/catalog model, DD bindings, record operations, statuses, and
  transactional mutation needed by 0.1;
- RACF identity, authentication, profile matching, SAF authorization, and
  audit;
- scoped host-service clients and centralized middleware;
- SQLite persistence for local/restart tests; and
- hostile, authorization, idempotency, cancellation, and failure suites.

Exit: provider state is reachable only through typed services; restart and
negative security fixtures pass.

## ME.V4 — CICS runtime

Deliver:

- exact 0.1 CICS operation inventory;
- typed terminal, file, program-control, condition, EIB, transaction, and
  security contracts;
- protocol-neutral session and suspension/resume state;
- COBOL CICS lowering and interpreter dispatch;
- isolated effect comparison and mutation safety; and
- CardDemo/accepted CICS fixture parity.

Exit: every accepted CICS operation has one typed route, provider, limits,
conditions, audit, and exact differential evidence.

## ME.V5 — JCL/JES batch

Deliver:

- JCL syntax, procedures, symbols, DD, conditions, and workflow plan;
- JES job/step lifecycle, spool, cancellation, purge, and status;
- all `EXEC PGM=` dispatch through `ProgramService`;
- COBOL batch plus required 0.1 utilities implemented through the same execution
  spine; and
- job/dataset/security failure and restart behavior.

Exit: accepted JCL/JES fixtures use one program path and one durable job
authority.

## ME.V6 — z/OSMF and durable single-node product

Deliver:

- Axum gateway and typed route adapters;
- information, jobs, datasets, and accepted security/console routes;
- central versioned configuration and readiness;
- SQLx SQLite/PostgreSQL store adapters and object artifact storage;
- append-only events, materialized state, effect intent/result, checkpoints,
  backup/restore, and restart recovery;
- bounded workload lanes, graceful shutdown, telemetry, and capacity runbook;
  and
- core-server profile closure.

Exit: a single active worker pool runs the full 0.1 profile through durable
interfaces and survives declared restart/failure scenarios.

## ME.V7 — Certification and cutover

Deliver:

- full 0.1 differential and protocol compatibility pack;
- fuzz/property/model/concurrency/security/resource evidence;
- long-running mixed COBOL/CICS/JCL/z/OSMF load;
- canary, default promotion, and rollback rehearsal;
- public configuration and API documentation;
- package ownership and release policy; and
- removal/archive of the old implementation from production closure.

Exit: mainframe-env is the sole default 0.1 authority, with no hidden fallback and no
out-of-scope dependency in the shipped profile.

## Work sequencing rule

Do not implement later-phase abstractions speculatively. A phase may prepare
only contracts required by its accepted 0.1 consumers. New scope requires an ADR
and an updated profile manifest before implementation.
