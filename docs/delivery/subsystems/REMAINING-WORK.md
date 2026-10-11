# Remaining roadmap work

Status: **Planning snapshot; full phase acceptance incomplete**
Owner: **maintainers, through each linked subsystem progress record**
Scope: **legacy 0.9–0.17 mapping and all 54 planned work packages**
Applies from: **subsystem delivery organization**

Reviewed on 2026-10-11 against merged source
`3e66917ceea7cce253ce9d4cddda388279e3db6e`. This is a planning summary of the
linked records, not a fresh execution result or a completion certificate.
Update the owning progress record first, then reconcile this summary on the
same source. Newer results from an unmerged branch retain that branch's source
and evidence identity until integration is reviewed.

## Where to find the authority

- [Subsystem index](README.md): the plan, progress record and implementation
  prompt for each phase.
- [Dependencies](DEPENDENCIES.md): completion dependencies; private preparation
  may precede public admission within the plan's declared boundary.
- [Unreleased review](../UNRELEASED-STATUS.md): framework-wide implementation
  and acceptance summary.
- [Historical research roadmap](../../research/IBM-OFFICIAL-COVERAGE-ROADMAP.md)
  and its [machine-readable companion](../../../conformance/roadmap/ibm-official-coverage-roadmap.json):
  frozen scope and estimates, not current implementation counters.

The old release numbers are lookup aliases. Work remains owned by the subsystem
and phase names in the documentation registry. Finishing coverage.foundation
does not finish these nine phases.

## Legacy roadmap crosswalk

| Old milestone | Current phase and plan | Owning progress | Recorded boundary |
|---|---|---|---|
| 0.9 | [cics.application-api](cics/application-api-plan.md) | [Progress](cics/application-api-status.md) | Bounded application runtime exists; complete application behavior and acceptance remain incomplete |
| 0.10 | [cics.system-api](cics/system-api-plan.md) | [Progress](cics/system-api-status.md) | Private SPI/FEPI preparation exists; public execution is not admitted |
| 0.11 | [zosmf.rest](zosmf/rest-plan.md) | [Progress](zosmf/rest-status.md) | Operation normalization is complete; portfolio adapters and backend acceptance remain pending |
| 0.12 | [db2.core](db2/core-plan.md) | [Progress](db2/core-status.md) | Bounded syntax/value surfaces and existing static SQL exist; generic execution integration remains incomplete |
| 0.13 | [db2.programming](db2/programming-plan.md) | [Progress](db2/programming-status.md) | Advanced programming scope remains pending on core acceptance |
| 0.14 | [ims.programming](ims/programming-plan.md) | [Progress](ims/programming-status.md) | Selected DB/TM behavior exists; full context and recovery behavior remain incomplete |
| 0.15 | [mq.programming](mq/programming-plan.md) | [Progress](mq/programming-status.md) | Selected MQI flows exist; all 26 complete-call gates remain pending |
| 0.16 | [integration.transactions](integration/transactions-plan.md) | [Progress](integration/transactions-status.md) | Early participant contracts exist; mixed-resource transactions and recovery remain incomplete |
| 0.17 | [certification.licensed](certification/licensed-plan.md) | [Progress](certification/licensed-status.md) | Shared harness preparation exists; authentic environments and campaigns remain pending |

## Read implementation and acceptance separately

`Preparation implemented` means catalogs, schemas, parsers, private kernels or
harness plumbing exist. `Bounded implementation` means executable behavior is
recorded within stated limits. Neither completes its parent work package.
`Scope complete` applies only to the named local deliverable, such as ZMF-1101
normalization; it does not admit new REST routes or complete the portfolio.

`Pending` describes an unmet obligation. It does not by itself mean there is no
code. `Not separately recorded` identifies a reporting gap: the owning record
does not give that package an individual disposition. Inspect its source and
record a bounded slice before estimating or marking it unstarted. Existing
shared provider behavior must not be implemented again merely because a later
package lacks a separate progress row.

Plan headers identify planning specifications; historical `Proposed` wording
does not describe current implementation. Current implementation belongs to
the linked progress record. Historical counts and
results retain their original source; typed registration, helper tests and
missing licensed runs cannot be converted into whole-row completion.

## 0.9 — CICS application API

Authority: [plan](cics/application-api-plan.md),
[progress](cics/application-api-status.md). Completion dependencies:
cobol.execution, racf.security, dataset.data. Current admission is 260 typed,
zero legacy and three unready rows, not 260 accepted command families.
CICSMESSAGE, GETNEXT TIMER and ISSUE COPY remain unready. The plan's older
three-typed/20-legacy/240-unready figures describe its historical CIC-901
boundary.

| ID | Planned deliverable | Recorded implementation | Remaining acceptance / next step |
|---|---|---|---|
| CIC-901 | Generated API grammar, options, conditions, EIB/RESP, registry, and bindings to hardened execution/security contracts | Preparation implemented; current typed admission recorded | Preserve three unready dispositions; close applicable forms and obligations through the owning families |
| CIC-902 | Program, task, interval, storage, and recovery command families | Bounded frame/call/storage preparation and runtime slices | Complete installed replacement handoff, frame/storage ownership and lifecycle/recovery matrices |
| CIC-903 | Terminal/BMS, TSQ/TDQ, file, journal, and spool families | Bounded terminal/file behavior; READ/REWRITE/SYNCPOINT pilot | Complete family-wide selected-route, terminal lifetime, durability and recovery obligations |
| CIC-904 | Channels/containers, BTS, documents, web, and transforms | Bounded container/BTS and related runtime slices | Close task-wide storage lifetime and complete family behavior, security and recovery |
| CIC-905 | APPC/MRO conversations and distributed program link | Selected local/owned-DPL compatibility behavior | Complete supported conversation/distributed contexts and failure/recovery matrices |
| CIC-906 | Integrated cross-family security, concurrency, recovery, scale, and IBM differential campaigns | Selected context, retention and backend controls | Run full family integration/backend matrices; retain pending licensed differential separately |

## 0.10 — CICS SPI and FEPI

Authority: [plan](cics/system-api-plan.md),
[progress](cics/system-api-status.md). Completion dependency:
cics.application-api. Private grammar covers 266 SPI and 39 FEPI identities;
accepted public coverage remains 0/269 SPI and 0/39 FEPI. Three exact SPI
source/form joins remain unresolved.

| ID | Planned deliverable | Recorded implementation | Remaining acceptance / next step |
|---|---|---|---|
| SPI-1001 | Generated SPI/FEPI grammars, options, conditions, and registry | Private preparation implemented; package incomplete | Resolve PERFORM SECURITY/SSL/STATISTICS joins and freeze executable command contracts |
| SPI-1002 | Resource inquiry, lifecycle, set, and administrative families | Private PROGRAM observation/security preparation; full package not separately recorded | Declare public resource slices and implement lifecycle/authorization/restart behavior after admission prerequisites |
| SPI-1003 | Monitoring, statistics, trace, dump, and system-control families | Not separately recorded; public execution pending | Declare family owners, state transitions and independent selected-route checks |
| SPI-1004 | CSD, bundles, topology, quiesce, and recovery operations | Not separately recorded; public execution pending | Implement resource publication, drain/quiesce and recovery through existing authorities |
| SPI-1005 | Complete FEPI pools, sessions, conversations, and targets | Private grammar preparation; runtime package not separately recorded | Implement pool/target/session and conversation/data-flow slices with failure/restart checks |
| SPI-1006 | Authorization, concurrency, failure, scale, and IBM differentials | Selected private security/SQLite helper controls | Complete public family/backend matrices and licensed campaigns; private checks do not admit public routes |

## 0.11 — z/OSMF REST portfolio

Authority: [plan](zosmf/rest-plan.md), [progress](zosmf/rest-status.md).
Completion dependencies: racf.security, dataset.data, jes.execution,
cics.system-api. Existing 23 official routes and seven custom CICS bindings
remain distinct. Most portfolio families lack an accepted backend capability;
catalog normalization does not implement those backends.

| ID | Planned deliverable | Recorded implementation | Remaining acceptance / next step |
|---|---|---|---|
| ZMF-1101 | Normalize 27 families/189 source headings; freeze operation/schema/error and backend-owner/obligation mapping | Scope complete: source normalization and generated contracts only | Consume its frozen mapping; keep unowned operations pending and existing routes unchanged |
| ZMF-1102 | Jobs, spool, console, workflow, and asynchronous-operation adapters | New portfolio adapters pending; existing routes retained | Implement each operation against an accepted backend and prove async/failure/cancellation behavior |
| ZMF-1103 | Dataset, file, software, configuration, and systems adapters | New portfolio adapters pending; existing routes retained | Resolve missing backend prerequisite slices before public integration |
| ZMF-1104 | Authentication, SAF, CSRF, negotiation, pagination, and audit | Full package acceptance not separately recorded | Extend and verify existing middleware across the complete normalized portfolio |
| ZMF-1105 | Custom-route separation, capability discovery, and compatibility | Existing namespace separation preserved; full package not separately recorded | Close truthful discovery, compatibility and official/custom separation for new adapters |
| ZMF-1106 | Protocol, failure, scale, cancellation, and IBM differentials | Portfolio acceptance pending | Execute all required normalized operations across 27 families, including licensed protocol comparisons |

## 0.12 — Db2 engine and common SQL

Authority: [plan](db2/core-plan.md), [progress](db2/core-status.md).
Completion dependencies: coverage.foundation, cobol.execution, racf.security.
The catalog has 158 SQL headings plus 16 SQL PL rows. Freeze the common/deferred
obligation map before broad execution; existing generic catalog/static-SQL
authorities are dependencies, not replacements to build again.

| ID | Planned deliverable | Recorded implementation | Remaining acceptance / next step |
|---|---|---|---|
| DB2-1201 | SQL lexer/parser, typed AST, official statement catalog, diagnostics | Bounded syntax implemented; in progress | Close full pinned recognition and common/deferred diagnostics/obligation mapping |
| DB2-1202 | Schema catalog, binder, names, types, functions, and expressions | Bounded qualification, type/value and default surfaces; in progress | Connect complete catalog/privilege binding and runtime value/default producers |
| DB2-1203 | Relational IR, query/DML executor, constraints, indexes, and views | Full generic execution scope pending | Integrate the frozen common subset with existing provider/catalog authorities |
| DB2-1204 | Transactions, cursors, dynamic SQL, host ABI, SQLCA, packages/plans | Existing signed static-SQL and bounded syntax behavior | Close general host/runtime integration, SQLCA and transaction/backend matrices |
| DB2-1205 | Common DDL and bounded SQL PL routine/control-flow core | Bounded DDL syntax; full execution scope pending | Implement applicable DDL/SQL PL execution and recovery from the frozen obligation map |
| DB2-1206 | De-hardcoding, compatibility, property, failure, and differential suites | Existing baseline controls; full package acceptance pending | Run candidate-bound zero-hardcode, failure/backend and common-subset differential checks |

## 0.13 — Complete Db2 programming surface

Authority: [plan](db2/programming-plan.md),
[progress](db2/programming-status.md). Completion dependency: db2.core.
All six full-scope packages are recorded Pending; advanced programming is
blocked on core acceptance. Core parser/value APIs do not complete them.

| ID | Planned deliverable | Recorded implementation | Remaining acceptance / next step |
|---|---|---|---|
| DB2-1301 | Remaining SQL/SQL PL syntax, binder, diagnostics, and conditions | Pending on accepted core | Consume the common/deferred map and close every remaining obligation |
| DB2-1302 | Advanced query, temporal, XML/LOB/array, and analytic semantics | Pending on accepted core | Implement advanced values, binding, execution and bounded failure behavior |
| DB2-1303 | Routines, triggers, variables, and dependency lifecycle | Pending on accepted core | Implement invocation, dependencies, invalidation and recovery |
| DB2-1304 | Packages/plans, bind lifecycle, privileges, and invalidation | Pending on accepted core | Complete bind and privilege lifecycle through existing catalog/package owners |
| DB2-1305 | Isolation, locks, deadlocks, savepoints, logging, and recovery | Pending on accepted core | Close full concurrency/transaction/recovery matrices |
| DB2-1306 | Distributed behavior, scale, compatibility, and IBM differentials | Pending on accepted core | Implement remaining distributed behavior and run backend/scale/licensed comparisons |

## 0.14 — IMS programming surface

Authority: [plan](ims/programming-plan.md),
[progress](ims/programming-status.md). Completion dependencies:
cobol.execution, racf.security, dataset.data. Selected behavior is implemented;
all 25 complete-family official gates remain unaccepted.

| ID | Planned deliverable | Recorded implementation | Remaining acceptance / next step |
|---|---|---|---|
| IMS-1401 | Generated call catalog, SSA grammar, PCB status, and diagnostics | Bounded call/SSA/PCB behavior; in progress | Complete raw CBLTDLI framing, feedback and remaining SSA/context classes |
| IMS-1402 | DBD/PSB/PCB metadata, validation, and application packages | Bounded package publication implemented | Complete metadata and call-context acceptance |
| IMS-1403 | Database organizations, navigation, mutation, indexes, and paths | Bounded database behavior implemented | Close complete organization, positioning and logical/secondary-path matrices |
| IMS-1404 | TM queues, scheduling, conversations, and alternate PCBs | Bounded TM dispatch implemented | Implement shared TM settlement/recovery and broader application behavior |
| IMS-1405 | Checkpoint/restart, logging, recovery, and utilities | Selected DB-batch recovery implemented | Complete TM/remaining-context recovery and ownership/lease guarantees |
| IMS-1406 | SAF, concurrency, scale, compatibility, and IBM differentials | Local assurance/harness and participant preparation | Close participant admission, shared-IR/backend/failure matrices and licensed differential |

## 0.15 — MQ programming surface

Authority: [plan](mq/programming-plan.md),
[progress](mq/programming-status.md). Completion dependencies:
cobol.execution, racf.security. Selected/private and installed finite flows
are implemented. All 26 complete-call profiles remain Pending; MQINQ is
unavailable. Native pass-through and owned simulator claims remain separate.

| ID | Planned deliverable | Recorded implementation | Remaining acceptance / next step |
|---|---|---|---|
| MQ-1501 | Generated MQI catalog, structures, options, reason codes, and registry | Identity/structure/status preparation implemented | Close all applicable structures, versions, options and host-context contracts |
| MQ-1502 | Queue-manager and object lifecycle plus names/resolution | Bounded object kernels and installed CONN/DISC/OPEN/CLOSE flows | Complete lifecycle/resolution, MQINQ and applicable context/backend behavior |
| MQ-1503 | Put/get/browse, properties, selectors, groups, and distribution lists | Bounded delivery/property and installed forwarding; Batch prerequisites | Complete installed/JES composition and all delivery/options/concurrency obligations |
| MQ-1504 | Pub/sub, callbacks, asynchronous consumption, and triggering | Private bounded pub/sub and related kernels | Integrate complete installed callback/async/pub-sub/triggering behavior |
| MQ-1505 | Syncpoint, persistence, backout, dead-letter, restart, and recovery | Selected publication/replay and recovery prerequisites | Close lifecycle/recovery and participant admission; reconcile cold-connect results by source |
| MQ-1506 | SAF, concurrency, overload, compatibility, and IBM differentials | Selected authorization/helper controls | Complete shared-IR/backend/concurrency matrices; licensed oracle remains separately pending |

## 0.16 — Cross-resource transactions and recovery

Authority: [plan](integration/transactions-plan.md),
[progress](integration/transactions-status.md). Completion dependencies:
jes.execution, cics.system-api, db2.programming, ims.programming,
mq.programming. Early participant compatibility does not establish aggregate
atomicity or universal exactly-once behavior.

| ID | Planned deliverable | Recorded implementation | Remaining acceptance / next step |
|---|---|---|---|
| INT-1601 | Common UOW/effect protocol and transaction coordinator | Early participant schema, coordinator binding and compatibility slices | Complete provider admissions and prepare/commit/backout/compensation/outcome closure |
| INT-1602 | Principal, delegation, correlation, deadline, and audit propagation | Full package not separately recorded | Inspect existing propagation, declare missing mixed-provider slices and prove complete context preservation |
| INT-1603 | Cancellation, retry, idempotency, overload, and backpressure semantics | Full package not separately recorded | Inspect existing coordinator controls and close mixed-provider failure/overload obligations |
| INT-1604 | Crash, in-doubt, heuristic, unknown-outcome, and operator recovery | Selected early CICS unknown-outcome/replay compatibility | Complete provider-loss/crash/operator recovery across accepted participant combinations |
| INT-1605 | Mixed-provider backup/restore, migration, replay, and compatibility | Full package not separately recorded | Implement and prove a coherent cross-provider restore boundary, not independent provider restores |
| INT-1606 | Cross-resource matrix, soak, chaos, scale, and differential suites | Early compatibility fixtures only | Execute complete mixed-resource, application, fault/soak/backend and differential matrices |

## 0.17 — Licensed certification

Authority: [plan](certification/licensed-plan.md),
[progress](certification/licensed-status.md). Completion dependencies:
zosmf.rest, integration.transactions. Required licensed environments are
missing. Local or synthetic harness validation supplies no licensed credit.

| ID | Planned deliverable | Recorded implementation | Remaining acceptance / next step |
|---|---|---|---|
| CER-1701 | Licensed environment manifests and reproducible oracle harnesses | Early environment-schema, binding and synthetic validation slices | Populate authentic environments, complete missing readers/adapters and bind an exact final candidate |
| CER-1702 | Per-subsystem differential campaigns and mismatch triage | Pending | Run authorized pinned-environment campaigns and retain reviewed mismatches |
| CER-1703 | Cross-subsystem replay, fault, scale, and long-run certification | Pending | Run integrated campaigns after accepted provider/backend closure |
| CER-1704 | Migration, backup/restore, upgrade, rollback, and compatibility rehearsal | Pending | Rehearse the exact candidate against supported state generations and failure points |
| CER-1705 | Canonical coverage/evidence digest, provenance, SBOM, and security closure | Pending | Close all mandatory obligations and exact artifact/environment provenance |
| CER-1706 | Unchanged-source 1.0 release rehearsal and independent review | Pending | Complete rehearsal and independent review; publication remains a separate release decision |

## Missing tracking information and reconciliation

The 54 deliverables above remain present in their plans. Several progress
records do not give every package an individual state, assigned person, linked
active issue, reviewed source/date or next bounded slice. Those facts are
unrecorded, not inferred as no implementation. No package-level finish date can
be derived from the old engineer-month estimates.

Each new bounded slice should record its parent ID, actual owner, source,
implementation boundary, checks still unrun, actual failures, specific missing
inputs, consumed dependencies and next action in the owning progress document.
Keep failed checks distinct from implementation TODOs and from external
licensed/source blockers. Unavailable inputs block only dependent work.

The research roadmap's observable footprint and hardcode audit are the frozen
2026-08-31 analysis. They must not be read as current counters. CICS historical
admission counts, earlier Db2 paused-wave descriptions and earlier MQ recovery
failures also retain their original producing scope.

[Foundation follow-up PR #393](https://github.com/toreleon/mainframe-env/pull/393)
has a newer reported snapshot at
`a1696e3b31850f1d4626fb69f8b2b222cda89c03`, reviewed on 2026-10-11. Its PR
description records the following changes in disposition. These are unmerged
branch reports, not checks rerun by this documentation review:

| Topic | Follow-up reported by #393 | Integration action |
|---|---|---|
| Original 2 MiB memory input | Scoped probe PASS; earlier refusal retained | Add the new source-qualified result beside the original refusal |
| PostgreSQL backend profile | All 12 original registered gates PASS | Preserve actual backend/source scope; do not promote to complete roadmap acceptance |
| Targets, rustdoc, fuzz and coverage | Scoped stages PASS; whole-workspace command still FAILED | Keep scoped checks separate from the missing complete workspace result |
| Default online and batch | Online 403 and batch layout/CCSID failures | Retain failures and review actual fixes before fresh acceptance |
| Live SDK | Inclusive ten-second allowance exceeded; failing subphase unknown | Keep failed run/source; investigate phase timing without changing the allowance |
| Remaining local implementation | Durable READ journal, typed package composition and IMS storage bridge TODOs | Keep these local work items separate from vendor/reference and Docker blockers |

Its results are not merged-main passes. During integration, reconcile
the original failure and later scoped result with both sources retained;
update the owning progress record and this summary instead of erasing the
historical result or promoting a local pass to full phase acceptance.

## Merge order and maintenance

Merge this documentation consolidation before the active Foundation follow-up
PR #393 and other dependent implementation PRs. It is based directly on main
and changes no runtime, catalog denominator, source pin or acceptance gate.
After this PR merges, those branches should incorporate updated main, preserve
their newer owning progress records and regenerate documentation through the
existing generator. This sequencing requests review priority; it does not
merge or publish an implementation branch automatically.

When updating a phase, maintain its plan/progress authority first and review the
matching six rows here. Regenerate navigation and the documentation manifest
normally. Preserve the distinction between planned scope, implemented behavior,
current-candidate validation and licensed certification.
