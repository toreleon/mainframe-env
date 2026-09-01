# IBM official coverage and de-hardcoding roadmap

Status: research baseline for planning; not a compatibility claim
Snapshot date: 2026-08-31
Repository baseline: `mainframe-env` 0.1.1 working tree based on commit
`857115b907ce7098c965a51117a079048ea8182e`

## Executive conclusion

The current system is a strong, bounded CardDemo runtime, but it is not close to
full IBM product parity. The most important distinction is between four kinds of
coverage:

1. **Surface coverage**: every documented statement, command, route, call,
   parameter, option, and result is recognized by a typed contract.
2. **Semantic coverage**: every accepted item executes all documented positive,
   negative, boundary, condition-code, and interaction behavior.
3. **Operational coverage**: restart, cancellation, concurrency, locking,
   durability, recovery, security, overload, migration, and version behavior
   match the pinned IBM product.
4. **Ecosystem coverage**: administration, installation, hardware-facing,
   sysplex/distributed, observability, tooling, and product-integration surfaces
   are present.

`100%` is valid only when every mandatory inventory row is complete in all
applicable dimensions. A parser route, enum variant, happy-path test, or
CardDemo journey is not a completed IBM row by itself. Partial rows never round
up.

The practical path is:

- make coverage authority and dispatch data-driven in 0.2.0;
- complete the public programming surfaces subsystem by subsystem through
  0.15.0;
- complete cross-resource transaction/recovery behavior in 0.16.0;
- run licensed IBM differential certification in 0.17.0; and
- promote 1.0.0 only when all pinned programming-surface rows are green.

This program does **not** make the whole IBM product ecosystem 100% equivalent.
Full DFSMS, CICS Liberty/Java, Db2 optimizer and utilities, IMS TM operations,
MQ channels/clustering, sysplex, hardware, and administration require a
separate 2.x portfolio. Claiming whole-product parity before that inventory
exists is prohibited.

The machine-readable companion is
`conformance/roadmap/ibm-official-coverage-roadmap.json`.

The prepared implementation dossiers are indexed at
[`docs/delivery/coverage-versions/README.md`](../delivery/coverage-versions/README.md),
including the dependency graph and safe parallel work lanes.

## Measurement contract

Each official inventory row has these independent gates:

| Gate | Required evidence |
|---|---|
| `recognized` | Valid syntax/request is parsed into a versioned owned type; every invalid form fails with the correct category. |
| `validated` | All operand, option, context, authority, and compatibility rules are enforced. |
| `executed` | All specified state transitions and outputs are implemented without workload-name branches. |
| `conditioned` | Return codes, reason codes, statuses, EIB/SQLCA/PCB/MQ results, and diagnostics match. |
| `recovered` | Applicable mutation, retry, rollback, restart, concurrency, cancellation, and unknown-outcome cases pass. |
| `differential` | Positive, negative, boundary, and interaction cases match a licensed pinned IBM environment. |

For subsystem `S` and gate `g`:

```text
coverage(S, g) = complete mandatory rows for g / total mandatory rows for g
```

There is no weighted shortcut for the 100% claim. Weighted scores can be used
for prioritization only.

Every baseline is immutable and names the product version, document edition or
snapshot date, source URL, source digest, extraction method, and denominator.
When IBM continuous-delivery documentation changes, a new baseline is created;
the old denominator is never silently rewritten.

## Official pinned baselines

The counts below are reference units, not a statement that all units are equal
in implementation cost.

| Subsystem baseline | Official denominator | Current observable footprint |
|---|---:|---:|
| Enterprise COBOL for z/OS 6.5 | 44 PROCEDURE DIVISION statements; 82 intrinsic functions; 15 compiler-directing statements; 5 directive groups; 10 FD clauses; 17 data-description clauses | 42/44 statement families recognized, 37/44 executable; 12/82 intrinsic functions; selected data/source forms |
| CICS TS 6.x docs snapshot | 263 API commands; 269 unique SPI commands; 39 FEPI commands | 25 internal typed operations, of which approximately 24 map to official command forms |
| z/OS 3.2 JCL/JES2 | 20 JCL statements; 13 JES2 JECL statements; 74 DD, 19 EXEC, 35 JOB, and 76 OUTPUT parameters | about 14/20 statement forms recognized; 0/13 JECL; 10/74 DD forms, 4/19 EXEC, 3/35 JOB, and 0/76 OUTPUT semantics |
| z/OS 3.2 DFSMS AMS | 31 functional commands plus modal command language | about 8/31 functional commands and selected IF/SET behavior |
| z/OS 3.2 RACF/SAF | 34 RACF command families; 14 RACROUTE request types | roughly 6 command-equivalent operations and 3/14 request types |
| z/OSMF 3.2 | 27 REST service families; 189 direct guide headings requiring endpoint normalization | 5 official families and 23 declared official routes |
| Db2 13 for z/OS | 158 SQL statement headings plus SQL PL | 14 internal operations; only a small statement-equivalent subset; three production table shapes are hardcoded |
| IMS 15.6 | 25 unique DL/I call families in the IBM call/command comparison | about 9 reached call families over a generic but narrow hierarchy model |
| IBM MQ 9.4 | 26 unique MQI calls (27 documentation rows contain one duplicate) | 7/26 calls |

Corrections to the earlier coarse audit:

- COBOL 6.5 Chapter 28 contains 44, not 46, PROCEDURE DIVISION statement
  headings. Embedded `EXEC CICS`, `EXEC SQL`, and `EXEC DLI` are tracked as
  host-language extensions, not added to that denominator.
- The IBM MQ call-description page has 27 rows but repeats `MQMHBUF`; the unique
  denominator is 26.
- Db2 Chapter 7 has 161 direct headings, of which three are overview headings;
  the SQL-statement denominator is 158 before adding SQL PL.

## Hardcode taxonomy and current audit

Hardcode is not one category. Each class has a different replacement:

| Class | Meaning | Policy |
|---|---|---|
| H1 application identity | Application table, program, transaction, dataset, queue, map, or user name in production logic | Must be zero. Move to signed application packages. |
| H2 protocol catalog | Handwritten statement/command/route/call lists, option names, codes, and layouts | Generate from a versioned reviewed catalog. Generated exhaustive code is allowed. |
| H3 semantic dispatch | `if name == ...` or string inspection selects business/system behavior | Replace with typed AST/IR and registered subsystem handlers. |
| H4 compatibility ABI | Copybooks, control blocks, constants, service names, and status layouts embedded in an unrelated kernel | Move to versioned subsystem ABI packages and source libraries. |
| H5 topology/configuration | Provider, queue, class, resource, or route topology fixed in code | Move to validated configuration/resource catalogs. |
| H6 conformance fixture | Exact workload names and golden values in test/conformance code | Allowed; must never be a production dependency. |

The current production scan finds 32 obvious H1 application-specific hits:

- 29 in `mainframe-env-db2`, all tied to `CARDDEMO.TRANSACTION_TYPE`,
  `CARDDEMO.TRANSACTION_TYPE_CATEGORY`, or `CARDDEMO.AUTHFRDS`;
- 3 in `mainframe-env-batch`, tied to `COBTUPDT` and `CBPAUP0C`.

Dataset, RACF, CICS, IMS, and MQ providers contain CardDemo literals in their
tests but not in their pre-test production sections. They still contain H2/H3
catalog and semantic hardcode that limits official coverage.

The compiler owns nine CICS/Db2/MQ compatibility copybooks. This is H4: the
compiler should consume ordered subsystem source libraries, while each
subsystem package owns and versions its ABI assets.

## Subsystem gap analysis and target design

### COBOL

**Current strengths**

- Fixed/free source, CP037, COPY/REPLACING, qualified layouts, levels
  66/77/78/88, REDEFINES, RENAMES, OCCURS DEPENDING ON, reference modification,
  DISPLAY/binary/packed/edited storage, structured control, file calls, linkage,
  and selected embedded host calls are executable.
- The deterministic IR and interpreter are a suitable semantic reference base.

**Critical gaps**

- `INVOKE`, `MERGE`, `RELEASE`, sort-file `RETURN`, and `SORT` remain explicit
  unsupported; `DELETE` and `START` are absent from the main statement catalog.
- Only 12 of 82 intrinsic functions execute.
- File-description and data-description clauses are partial; NATIONAL, UTF-8,
  DBCS, floating point, object references, typedef/type, synchronized, volatile,
  dynamic-length, external/global, local storage, and full table semantics are
  incomplete.
- Statement option matrices, exception phrases, CORRESPONDING behavior,
  rounding/size-error edge cases, sort/merge procedures, OO/Java interop, and
  Language Environment behavior lack full models.
- `TEST-NUMVAL` currently behaves like a boolean validity check rather than
  returning IBM's error position/length result.
- CICS, SQLCA, and MQ compatibility definitions live in the compiler.

**Target architecture**

- Replace keyword-line recognition with a complete lossless grammar and typed
  AST, with recovery nodes prohibited from publication.
- Generate the statement, clause, directive, intrinsic, special-register, file
  status, and compiler-option catalogs from reviewed machine inventory.
- Implement intrinsic functions behind a typed function registry with category,
  arity, determinism, exception, and result-shape metadata.
- Model all data classes/usages in the semantic type system and storage-layout
  engine before adding execution shortcuts.
- Move CICS/Db2/IMS/MQ/LE source assets and ABI lowering to host-extension
  packages registered through owned compiler-extension contracts.
- Differentially compare diagnostics, layouts, final storage, file status,
  return code, and output against Enterprise COBOL 6.5.

### CICS

**Current strengths**

- BMS terminal sessions, pseudo-conversation, file control, program transfer,
  conditions/EIB, transient data, security admission, syncpoint, restart, and
  idempotent unknown-outcome handling are credible foundations.

**Critical gaps**

- The handwritten 25-operation enum covers less than one tenth of the 263 API
  commands and almost none of the 269 SPI or 39 FEPI commands.
- Temporary storage, channels/containers, APPC/MRO, BTS, web/document/transform,
  interval/task control, enqueue/dequeue, storage control, JES spool, journals,
  authentication, and most resource SPI families are absent.
- Command-specific option legality, RESP/RESP2 matrices, EIBFN values, thread
  safety/context restrictions, remote routing, and resource lifecycle are not
  catalog-complete.

**Target architecture**

- Introduce a versioned CICS command specification containing command identity,
  API/SPI/FEPI class, group/function code, options, operand directions, response
  conditions, context restrictions, and handler family.
- Generate translator parsing, typed request DTOs, validators, EIBFN mappings,
  capability requirements, and conformance skeletons from that specification.
- Dispatch by generated command identity into independently owned handler
  families; never select behavior using raw command strings.
- Make CSD/BUNDLE/BMS resources application-package data and keep protocol
  terminals separate from CICS semantic state.

### JCL, JES2, and utilities

**Current strengths**

- Procedures, includes, symbols, conditions, restart/skip, DD concatenation,
  inline data, DISP, GDG references, spool, hold/release, purge, warm start, and
  program routing exist.

**Critical gaps**

- Missing or non-semantic JCL forms include COMMAND, CNTL/ENDCNTL, EXPORT,
  SCHEDULE, XMIT, and OUTPUT processing.
- JOB supports only `CLASS`, `PRTY`, and `RESTART`; EXEC supports `PGM/PROC`,
  `PARM`, and `COND`; DD implements only selected forms of `*`, DATA, DLM,
  DSNAME, SYSOUT, DCB, DSNTYPE, LRECL, RECFM, and DISP.
- No JES2 JECL statement is implemented.
- JES lacks full converter/interpreter diagnostics, OUTPUT routing, NJE,
  multi-access spool, execution selection, checkpoint variants, started-task,
  operator, printer/punch, and complete security semantics.
- `IEBCOPY`, `IEBDG`, `IEBEDIT`, and `IEBUPDTE` return summary output rather
  than implement their utilities.
- The batch service branches directly on IDCAMS, SDSF, IKJEFT01, DFSRRC00,
  COBTUPDT, and CBPAUP0C.

**Target architecture**

- Generate statement and parameter validators from a JCL schema catalog while
  preserving source-level provenance and substitution timing.
- Separate converter, scheduler, allocation, execution, output, and JES2 state
  machines with explicit phase results.
- Register utilities and subsystem controllers as ordinary typed programs.
  Batch execution calls `ProgramService` once; it contains no program-name
  branches.
- Give every utility its own parser, state transformer, return-code model, and
  golden/differential suite.

### Dataset, VSAM, catalog, and AMS

**Current strengths**

- Sequential, partitioned, KSDS, ESDS, RRDS, GDG, AIX/PATH, record formats,
  members, concatenation, browsing, atomic index mutation, seed installation,
  replay, and restart are implemented generically.

**Critical gaps**

- LDS, full fixed/variable RRDS distinctions, RBA access, spanned records,
  control interval/control area behavior, alternate-index edge cases, record
  management macros, SHAREOPTIONS, RLS/TVS, and locking are incomplete.
- PDS and PDSE versions, program objects, directory behavior, generations,
  aliases, user/master catalogs, volume/VTOC semantics, multivolume/tape,
  SMS classes/ACS, encryption, compression, striping, extended addressability,
  buffering, migration, and recall are absent or abstracted away.
- Only about 8 of 31 AMS functional commands are reached.

**Target architecture**

- Split logical catalog, allocation, access-method, record-locking, storage
  placement, and lifecycle authorities behind owned transactions.
- Add schema-driven DCB/SMS/VSAM attributes with explicit unsupported hardware
  adapters until implemented; no attribute may be silently ignored.
- Implement all AMS functional and modal commands over the same authorities used
  by JCL, CICS, COBOL, and z/OSMF.
- Use an abstract deterministic volume model for conformance and separate real
  storage adapters for operational deployments.

### RACF and SAF

**Current strengths**

- User/group/profile persistence, UACC, six access levels, generic matching,
  authentication, authorization, default deny, audit redaction, and application
  manifests are generic.

**Critical gaps**

- Most of the 34 command families are absent, including complete alter/delete/
  list/search, REMOVE, SETROPTS, certificate/keyring, distributed identity,
  RRSF, database variation, subsystem, and system-option behavior.
- Only AUTH-, VERIFY-, and AUDIT-like paths exist from the 14 RACROUTE request
  types. ACEE creation/deletion/nesting, FASTAUTH/RACLIST, DEFINE, EXTRACT,
  SIGNON, STAT, token operations, DIRAUTH, VERIFYX, and return/reason matrices
  are missing.
- Group hierarchy/authority, profile segments, conditional access, class
  activation/RACLIST caching, password/passphrase policy, MFA, security labels,
  PROGRAM control, auditing/SMF, and delegation are partial or absent.

**Target architecture**

- Define profile templates and segments as versioned schemas; the database
  stores schema-tagged profiles rather than one reduced profile struct.
- Implement command processors over a common profile transaction service.
- Model ACEE and security tokens explicitly; SAF requests become typed state
  machines with exact SAF/RACF return and reason codes.
- Generate supplied class metadata and keep installation-defined CDT classes as
  validated configuration.

### z/OSMF

**Current strengths**

- Information, authentication, dataset, jobs, and console routes share the
  product authorities and enforce bounds, authorization, CSRF, and stable error
  mapping.

**Critical gaps**

- Only 5 of 27 service families are present. Dataset lacks UNIX file/mount and
  several copy/rename/attribute operations; jobs lacks hold, release, class
  change, correlator/JESB variants, and full headers/query behavior.
- Cloud provisioning, software/storage/sysplex management, workflows,
  topology, TSO, WLM, RMF, parmlib, variables, compliance, notifications,
  persistence, routing, settings, and catalog services are absent.
- Routes are handwritten and the custom CICS session namespace must never count
  as IBM z/OSMF coverage.

**Target architecture**

- Maintain a reviewed route/operation catalog derived from the pinned
  Programming Guide and available OpenAPI documents.
- Generate router registration, request/response schemas, authorization needs,
  headers/query parsing, and negative tests; handlers remain thin translations
  to subsystem ports.
- Count official and product-specific namespaces separately.

### Db2 for z/OS

**Current strengths**

- Host variables/indicators, SQLCA-shaped results, CRUD, cursors, commit,
  rollback, conflict, extraction, replay, and restart work for CardDemo.

**Critical gaps**

- Production code recognizes three CardDemo tables and their columns by name.
- There is no general Db2 SQL parser, binder, catalog, relational expression
  engine, type system, optimizer/executor boundary, constraint/index model,
  privilege model, package/plan/bind model, or full locking/isolation system.
- The small operation enum is not comparable with the 158 SQL statements and
  SQL PL surface. SELECT, DDL, DML, expressions, functions, nulls, joins,
  subqueries, temporal/XML/LOB/array types, triggers, routines, dynamic SQL,
  distributed SQL, utilities, and diagnostics are incomplete.

**Target architecture**

- Parse Db2 dialect into an owned SQL AST; bind against a generic versioned
  catalog; lower queries to relational IR; execute through typed expression,
  row, index, constraint, and transaction engines.
- Compile static SQL to immutable packages identified by normalized statement
  and schema fingerprints; model plans, collections, compatibility levels, and
  authorization separately.
- Move all CardDemo DDL, rows, constraints, extraction formats, and statements
  into the application package. Production Db2 code must contain zero
  application identifiers.
- Keep optimization optional for correctness; the reference executor is the
  semantic authority.

### IMS

**Current strengths**

- The provider accepts external definitions and supports a durable root/child
  hierarchy, PSB/PCB selection, qualifiers, navigation, mutation, checkpoint,
  load/unload, commit/rollback, and restart without CardDemo production names.

**Critical gaps**

- Hold calls, DEQ, INIT variants, GSCD, LOG, POS, XRST, ROLL/ROLB/ROLS,
  SETS/SETU, STAT, and detailed status/command-code matrices are missing.
- DBD/PSB grammar, complete SSA semantics, multiple hierarchy depths, logical
  relationships, secondary indexes, HIDAM/HDAM/HISAM/SHISAM/GSAM/DEDB/MSDB,
  message processing, scheduling, logging, recovery, and utilities are partial
  or absent.

**Target architecture**

- Parse DBD/PSB definitions into a generic catalog and compile them to an
  immutable database-access plan.
- Generate the DL/I call/status/command-code catalog; execute calls through
  organization strategies over a common segment transaction engine.
- Keep application definitions and data entirely in installable packages.

### IBM MQ

**Current strengths**

- Queue definitions are external. Handles, put/get, message/correlation IDs,
  wait/no-message, triggers, syncpoint, replay, unknown outcome, and restart are
  generic.

**Critical gaps**

- Missing connection/disconnection, MQBEGIN, inquire/set, subscriptions,
  message handles/properties, callbacks/control, async status, and full option,
  descriptor, completion/reason-code behavior.
- Queue/topic/subscription/process/namelist models, alias/model/remote queues,
  browse cursors, context authority, dead-letter handling, expiry/priority,
  persistence, conversion, queue-manager policy, channels, clustering,
  security, and administration are incomplete.

**Target architecture**

- Generate MQI call, structure, option, object, completion, and reason catalogs
  from a pinned MQ 9.4 specification inventory.
- Model queue-manager connection, object handle, message handle, subscription,
  callback, and unit-of-work lifecycles explicitly.
- Put all queue/application topology in installed MQ resource manifests; keep
  broker/channel implementations behind separate providers.

## Cross-cutting target architecture

```text
official source receipts
        |
        v
reviewed machine catalogs -----> generated parsers/types/validators/tests
        |                                      |
        v                                      v
versioned semantic IR ----------------> registered generic handlers
        |                                      |
        +--------------> differential oracle <-+
                              |
                              v
                 immutable coverage evidence

application package
  sources + schemas + resources + DDL/DBD/PSB/MQ/CSD/JCL + seed data
        |
        v
generic compiler/providers/runtime (zero application identities)
```

Required cross-cutting components:

1. `OfficialCatalog` contract with immutable product/version/source identity.
2. Generated typed catalogs for syntax, options, results, conditions, and
   capability requirements.
3. `SubsystemHandlerRegistry` keyed by generated semantic identity, not raw
   application or command strings.
4. Versioned application package sections for subsystem resources and ABI
   source libraries.
5. Coverage store that records every row and all six gates independently.
6. IBM oracle adapters that run out of process and record product levels,
   compiler options, environment, input identity, output observations, and
   redaction-safe evidence.
7. Static gates:
   - no application identity in production crates;
   - no direct program/table/transaction dispatch by string;
   - every official catalog row has exactly one owner and handler;
   - every route/operation advertises only executable behavior;
   - no test/conformance crate is a production dependency.

## Minor-version roadmap

The sequencing minimizes rework. A later subsystem may prototype earlier, but
it cannot claim completion before its dependencies and exit gates pass.

| Version | Deliverable | Mandatory exit result | Estimate |
|---|---|---|---:|
| 0.2.0 | Official coverage authority and de-hardcoding foundation | Pinned catalogs and coverage schema exist; generated dispatch works; 32 H1 production hits become zero; CardDemo remains 20/20 | 6–9 engineer-months |
| 0.3.0 | COBOL complete grammar, directives, clauses, and semantic type/layout system | 44/44 statements, 82/82 functions, and all pinned clauses/directives recognized and validated; no compiler-owned host ABI assets | 10–15 |
| 0.4.0 | COBOL execution and LE/host-extension completeness | All COBOL rows execute and differential gates pass, including unsupported families from 0.1 | 12–18 |
| 0.5.0 | RACF command language and SAF | 34/34 commands, 14/14 RACROUTE types, profile segments, ACEE/token, cache, policy, certificate, audit, and recovery gates pass | 16–24 |
| 0.6.0 | Dataset/VSAM/catalog/AMS programming surface | All pinned organizations, record access, catalog forms, 31 AMS commands, locking, RLS/TVS, and recovery rows pass | 18–30 |
| 0.7.0 | Complete JCL converter and planner | 20/20 JCL statements; 74/74 DD, 19/19 EXEC, 35/35 JOB, 76/76 OUTPUT parameters; 13/13 JECL recognized/validated | 10–16 |
| 0.8.0 | JES2 execution, spool, NJE/MAS, and real utilities | All JCL/JES operational rows pass; no program-name branch in batch; summary utilities replaced | 14–22 |
| 0.9.0 | CICS application API | 263/263 API commands generated, implemented, conditioned, recovered, and differentially verified | 18–28 |
| 0.10.0 | CICS SPI and FEPI | 269/269 unique SPI and 39/39 FEPI commands pass; resource and distributed families complete | 18–30 |
| 0.11.0 | z/OSMF 3.2 REST portfolio | 27/27 service families and normalized route catalog pass exact protocol/error/security gates | 12–20 |
| 0.12.0 | Generic Db2 compiler/catalog/executor | Zero Db2 application names; 158 statement and SQL PL forms recognized; common DDL/DML/query/transaction semantics pass | 24–36 |
| 0.13.0 | Complete Db2 programming surface | Remaining SQL, routines, temporal/XML/LOB, packages/plans, privileges, isolation, diagnostics, and differential rows pass | 24–36 |
| 0.14.0 | Complete IMS programming surface | 25/25 call families, DBD/PSB/SSA, database organizations, TM interaction, checkpoint/restart, and utilities pass | 20–32 |
| 0.15.0 | Complete MQ programming surface | 26/26 MQI calls, objects, pub/sub, properties, callbacks, transactions, trigger/dead-letter, security, and recovery pass | 16–26 |
| 0.16.0 | Cross-resource transaction and failure semantics | CICS/Db2/IMS/MQ/dataset/JES/RACF mixed commit, rollback, heuristic/unknown outcome, restart, and overload matrices pass | 14–22 |
| 0.17.0 | Licensed IBM differential certification and 1.0 rehearsal | Every mandatory programming-surface row has all applicable gates green; no accepted unsupported row; reproducible certification pack | 12–20 |
| 1.0.0 | Stable pinned programming-surface release | Exact 0.17 evidence promoted without source drift; cutover, rollback, support, and upgrade contracts complete | release gate |

Total order-of-magnitude estimate: **244–384 engineer-months** for the pinned
programming surfaces, assuming access to IBM environments and subject-matter
experts. With 10–12 effective engineers, parallel provider work, and stable
oracles, this is roughly a two-to-four-year program. Whole-product ecosystem
parity is materially larger and is not included in this estimate.

## Dependency and parallelism plan

- 0.2.0 blocks all coverage claims and all production de-hardcoding.
- COBOL 0.3/0.4 is required before broad embedded CICS/SQL/DLI/MQ program
  differentials.
- Dataset/RACF can proceed in parallel after 0.2 and must stabilize before JES
  and CICS completion.
- JCL/JES and CICS can then proceed in parallel.
- z/OSMF can add families as subsystem ports become stable but cannot claim a
  route complete before its backend is complete.
- Db2, IMS, and MQ engines can proceed in parallel after 0.2; their embedded
  language and CICS integration gates wait for COBOL/CICS.
- 0.16 and 0.17 are integration/certification phases, not feature catch-up
  phases.

## 0.2.0 executable backlog

The first minor release should contain these concrete work items:

1. Add versioned official-source receipts and the six-gate coverage schema.
2. Normalize the COBOL 6.5, CICS 6.x, JCL/JES2 3.2, AMS 3.2, RACF/SAF 3.2,
   z/OSMF 3.2, Db2 13, IMS 15.6, and MQ 9.4 inventories.
3. Add generated semantic IDs and registries without changing current public
   results.
4. Add `no-application-hardcode` and `no-string-dispatch` architecture gates.
5. Define application-package sections for SQL schema/data/extract formats,
   IMS definitions/data, MQ definitions, subsystem ABI libraries, batch
   controllers, and security resources.
6. Move the three CardDemo Db2 table definitions and special CRUD/extract logic
   out of `mainframe-env-db2` into package data interpreted by a generic minimal
   relational engine.
7. Move `COBTUPDT` and `CBPAUP0C` behavior out of JES into installed typed
   programs/controllers.
8. Move DFHAID, DFHBMSCA, SQLCA, and MQ copybooks out of the compiler into
   versioned subsystem ABI libraries.
9. Replace built-in utility name matching with a program registry; preserve
   unsupported dispositions as catalog data until their real implementations
   land.
10. Generate z/OSMF router registration from the owned route catalog while
    keeping custom `/mainframe-env/*` routes in a separate inventory.
11. Make the workload ledger and program-status ledger agree, and add a gate
    that prevents future contradictory state.
12. Re-run all 260 workspace tests, explicit PostgreSQL controls, the full
    CardDemo gate, and live Zowe compatibility after de-hardcoding.

0.2.0 must not increase a numerator merely because a catalog row exists. Its
primary success is trustworthy measurement and zero application-specific
production dispatch.

## Certification prerequisites and risks

### Required external authority

Documentation alone cannot prove full semantics. Differential certification
requires licensed, version-pinned environments for:

- z/OS 3.2 and JES2;
- Enterprise COBOL 6.5 with explicit compiler/runtime options;
- CICS TS 6.3 or the exact product level represented by the 6.x snapshot;
- Db2 13 for z/OS;
- IMS 15.6; and
- IBM MQ 9.4.

Every oracle result must record APAR/PTF/product levels and configuration that
can affect behavior. Without these environments, surface recognition can reach
100%, but semantic and operational coverage must remain `unverified`, never
`pass`.

### Documentation licensing

The repository should store source identifiers, URLs, edition dates, hashes,
and reviewed derived facts. It should not redistribute IBM manuals or
proprietary binaries. Any generated inventory must be reviewed for licensing
and provenance before publication.

### Scope growth

IBM continuous delivery changes denominators. Freeze each baseline and add new
rows through a versioned compatibility decision. Never rewrite a passing old
baseline to make a new release appear complete.

### Performance versus correctness

Reference interpreters and deterministic storage models establish correctness
first. Native/JIT execution, physical VSAM layout, Db2 optimization, distributed
MQ, and sysplex scale are separate backends and cannot redefine the semantic
oracle.

## Official sources used for this baseline

- [Enterprise COBOL for z/OS documentation library](https://www.ibm.com/support/pages/enterprise-cobol-zos-documentation-library), Language Reference SC27-8713-04, 2026-05-31 edition.
- [CICS TS EXEC CICS function codes](https://www.ibm.com/docs/en/cics-ts/6.x?topic=codes-function-exec-cics-commands), snapshot 2026-08-31.
- [z/OS 3.2 MVS JCL Reference](https://www.ibm.com/docs/en/SSLTBW_3.2.0/pdf/ieab600_v3r2.pdf).
- [z/OS 3.2 DFSMS Access Method Services Commands](https://www.ibm.com/docs/en/SSLTBW_3.2.0/pdf/idai200_v3r2.pdf).
- [z/OS 3.2 RACF Command Language Reference](https://www.ibm.com/docs/en/SSLTBW_3.2.0/pdf/icha400_v3r2.pdf) and [RACROUTE request cross-reference](https://www.ibm.com/docs/en/zos/3.2.0?topic=macros-racroute-router-interface).
- [z/OSMF 3.2 Programming Guide](https://www.ibm.com/docs/en/SSLTBW_3.2.0/pdf/izua700_v3r2.pdf).
- [Db2 13 for z/OS SQL Reference](https://www.ibm.com/docs/en/SSEPEK_13.0.0/pdf/db2z_13_sqlrefbook.pdf).
- [IMS 15.6 EXEC DLI and DL/I call comparison](https://www.ibm.com/docs/en/ims/15.6.0?topic=programs-comparing-exec-dli-commands-dli-calls).
- [IBM MQ 9.4 MQI call descriptions](https://www.ibm.com/docs/en/ibm-mq/9.4.x?topic=calls-call-descriptions).
