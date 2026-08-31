# CardDemo-full 0.1.1 Implementation Plan

Status: **Prepared; implementation not started**
Owner request: fully execute the clean local AWS CardDemo repository
Target: **mainframe-env 0.1.1**

## Outcome contract

The target is not “recognize CardDemo syntax” or “implement its CICS command
names.” The target is a clean install followed by executable user, admin, batch,
Db2, IMS, and MQ journeys through public product entry points, with durable
restart and exact state observations.

The machine-readable scope, corpus, gaps, and workloads are under
`conformance/0.1.1/`. ADR-0006 owns the additive scope decision.

## Frozen application input

The local checkout must be clean and equal to:

```text
repository  https://github.com/aws-samples/aws-mainframe-modernization-carddemo.git
commit      59cc6c2fd7ebd7ef7925cad552a01a4b8b6e4d5e
tree        a1253e31c839f78d1f185b01771ba956da63b005
license     Apache-2.0
```

The corpus includes:

| Surface | Frozen amount |
|---|---:|
| COBOL programs | 44 |
| Base COBOL programs | 31 |
| Application/BMS copybooks | 62 |
| BMS maps | 21 |
| EXEC CICS blocks | 240 |
| Embedded SQL blocks | 32 |
| IMS/DLI markers | 35 |
| MQ calls | 22 |
| CICS transactions | 25 |
| Base CICS file resources | 8 |
| Main/extension JCL files | 46 |
| Runtime-import dataset definitions | 23 |
| Base EBCDIC seed files | 13 |

The runtime archive contributes exact KSDS/AIX keys, six GDG bases, three PDS
catalogs, migrated expected objects, and a TN3270 listener configuration. It is
oracle-only.

## Current baseline failures

These failures were reproduced on `3dd0de6`:

1. The real login source fails at COPY expansion because comment text containing
   “copy of” is treated as a COPY statement.
2. The CLI and server construct one-file source bundles and cannot supply the
   application copybook closure or IBM compatibility copybooks.
3. After isolating the comment bug and stubbing `DFHAID`/`DFHBMSCA`, 30 of 31
   base programs still fail analysis: 27 at global duplicate-name rejection and
   three at restricted HIR forms. The remaining wrapper calls an unavailable
   assembler routine.
4. All 38 base JCL files fail the current parser; fixed-width padded `/*`
   delimiters are one common first failure.
5. No CardDemo program, transaction, map, CICS-file alias, seed dataset, or
   interactive session route is installed in the product.
6. CICS lowering forwards symbolic argument text rather than storage bytes and
   does not apply completed response bytes, RESP/RESP2, or EIB values to COBOL
   storage.
7. CICS `REWRITE`/`DELETE` do not have keyed-record semantics and no alternate
   index authority exists.
8. ASKTIME, FORMATTIME, INQUIRE, and SYNCPOINT are explicit unsupported results.
9. Actual program XCTL/LINK names are absent from the program catalog and the
   generic batch program payload is not a CICS COMMAREA ABI.
10. IDCAMS, IEBGENER, SORT, DD routing, and allocation do not perform the
    repository's dataset transformations.
11. Db2, IMS/DLI, MQ, TN3270, and FTP/JES are not selected product authorities.

## Architecture target

```text
CardDemo application package
  -> generic application installer
     -> source/copybook compiler closure
     -> BMS + CSD resource catalog
     -> dataset/GDG/PDS seed catalog
     -> program + transaction catalog
     -> RACF application profiles

TN3270 / CICS-session API
  -> CICS transaction/session authority
     -> reference machine + program catalog
        -> typed dataset / Db2 / IMS / MQ / clock / JES effects
           -> durable platform store and unit-of-work coordinator
```

Application installation and transaction execution share owned contracts but
not mutable global state. The compiler remains deterministic; providers own I/O
and persistence.

## Work streams and ordering

### CD.A — Corpus and application packaging

- Make clean corpus verification executable from `CARDDEMO_CORPUS_DIR`.
- Add the generic application manifest and transactional installer.
- Parse BMS, CSD, runtime dataset imports, copybook libraries, and seed objects
  into owned versioned types.
- Reject missing, duplicate, orphan, or incompatible resources before install.

Exit: the exact package installs idempotently but is not yet marked runnable.

### CD.B — CardDemo-qualified COBOL compiler

- Repair fixed-column comment and COPY/REPLACING processing with provenance.
- Add compatibility copybooks without copying unlicensed IBM source.
- Implement scoped hierarchical names, legal duplicate qualification, groups,
  levels 66/77/88, REDEFINES, OCCURS/DEPENDING ON, subscripts, reference
  modification, linkage, FD/file control, decimal usages, and reached functions.
- Replace period-leading statement classification with typed structured syntax
  and CFG for nested IF/EVALUATE/SEARCH/PERFORM/GO TO and exception clauses.
- Implement reached file, CALL/USING, CICS, SQL, DLI, and MQ lowering.

Exit: all 44 source closures have an explicit published or accepted-correction
result; no incomplete program publishes.

### CD.C — Base CICS and terminal runtime

- Resolve literals and storage references into typed CICS operands.
- Apply INTO/FROM, RESP/RESP2, EIB, AID, map fields, cursors, COMMAREA, and
  linkage results back to exact storage.
- Install transaction/program/map/file catalogs and implement XCTL, LINK,
  RETURN TRANSID, pseudo-conversational resume, conditions, handlers, ABEND,
  ASKTIME, FORMATTIME, INQUIRE, and SYNCPOINT.
- Add BMS screen composition/input parsing and protocol-neutral session APIs.
- Add a bounded TN3270 adapter only over the same session authority.

Exit: base login, user menu, admin menu, every base screen, mutation, and restart
journey passes.

### CD.D — VSAM and application data

- Add keyed insert/read/rewrite/delete and stable browse ordering.
- Add alternate indexes with atomic base/index mutation.
- Import exact EBCDIC/ASCII records, key offsets, record lengths, GDGs, PDS
  members, ESDS/RRDS, and catalog metadata from the pinned package.
- Map CICS FILE resources to dataset authorities and enforce application RACF
  profiles.

Exit: clean install produces exact record counts/digests and all base online
file traces match.

### CD.E — JCL/JES and base batch

- Parse all base JCL and procedures with fixed records, continuations, symbols,
  instream data, DD concatenation, GDGs, conditions, and control statements.
- Connect DDs to real datasets and disposition lifecycle.
- Implement the reached IDCAMS, IEBGENER, SORT, IEFBR14, SDSF/CICS-command,
  internal-reader, FTP, and report utility behavior without generic success.
- Route named COBOL programs and compatible COBDATFT, MVSWAIT, CEEDAYS, and
  CEE3ABD services through ProgramService.

Exit: initialization, posting, interest, backup/combine/index, statement, and
report cycles pass with exact dataset and spool observations.

### CD.F — Db2, IMS, MQ, and cross-resource transactions

- Add static embedded-SQL precompile/lowering and the reached DDL, DML, cursor,
  SQLCA, commit, and rollback semantics.
- Add the reached IMS HIDAM/PCB/DLI operations, scheduling, checkpoint, load,
  unload, browse, insert, replace, and delete behavior.
- Add MQ open/get/put/put1/close, queue configuration, trigger, correlation,
  wait, and restart behavior.
- Coordinate VSAM, IMS, Db2, MQ, and CICS syncpoint intent/result records so
  unknown outcomes are reconciled and rollback is observable.

Exit: transaction-type management, MQ date/account queries, authorization
approval/decline, fraud marking, and purge journeys pass.

### CD.G — Operator compatibility and certification

- Provide owned install, compile, submit, inspect, and reset commands.
- Provide the selected FTP/JES subset needed by repository operator workflows,
  or record exact owned-command substitutions.
- Run base and full profiles on memory, SQLite, and PostgreSQL where applicable.
- Add restart, authorization, malformed input, cancellation, timeout, overload,
  resource, and provider-failure cohorts.
- Replace inventory-only CardDemo evidence with derived application-run
  receipts.

Exit: `carddemo-full` is derived pass and the 0.1.1 release candidate gate may
begin. No version bump, tag, push, publication, or deployment is implied.

## Commit and evidence rule

The exact issue sequence is in
`conformance/0.1.1/inventory/carddemo-gap-matrix.json`. Complete one issue at a
time, add its focused regression evidence, and make one local commit with the
declared subject. A later issue may not mark an earlier issue pass by prose.

Every cumulative gate records:

```text
corpus commit/tree and clean state
mainframe-env source commit or deterministic dirty-tree digest
profile and issue set
commands and exit codes
application journey observations
dataset/database/queue state digests
terminal/spool/output observations
restart/security/resource results
```

## Release-order stop condition

The repository currently identifies itself as `0.1.0-alpha.0`. Implementation
may proceed against the 0.1.1 target, but an actual `0.1.1-*` release cannot be
cut until the 0.1.0 baseline is finalized or the owner records a separate
version-line correction. The preparation artifacts do not make that decision.
