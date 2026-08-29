# Execution Prompt — Implement CardDemo-full for mainframe-env 0.1.1

Use this prompt from the `mainframe-env` repository root with the clean local
CardDemo checkout available separately.

---

You are implementing the additive **mainframe-env 0.1.1 CardDemo-full profile**.
Continue until every issue and journey in the 0.1.1 preparation pack passes, or
a genuine recorded stop-the-line condition prevents safe progress.

The current product version remains `0.1.0-alpha.0`. Do not bump versions, tag,
push, publish, deploy, or claim a 0.1.1 release merely because implementation
work starts. The release-order decision is a final gate.

## Authoritative preparation inputs

Read completely before editing:

1. `docs/decisions/0006-carddemo-0.1.1-profile.md`;
2. `docs/delivery/CARDEMO-0.1.1-IMPLEMENTATION-PLAN.md`;
3. `conformance/0.1.1/contract/carddemo-profile.json`;
4. `conformance/0.1.1/inventory/carddemo-corpus.json`;
5. `conformance/0.1.1/inventory/carddemo-gap-matrix.json`;
6. `conformance/0.1.1/workloads/carddemo-journeys.json`;
7. existing 0.1 architecture, ADRs, compatibility, security, durability, and
   release contracts; and
8. the complete relevant source, resources, data, READMEs, and runtime-oracle
   metadata in the clean CardDemo checkout.

Use `CARDEMO_CORPUS_DIR` as the explicit corpus location. It must resolve to the
clean commit and tree in the corpus inventory. Never store its absolute value in
checked-in evidence.

## Scope

The cumulative target includes:

- the 31-program base online and batch corpus;
- 17 base BMS maps and base CSD resources;
- exact VSAM KSDS/AIX data, GDGs, PDS, ESDS/RRDS, JCL and procedures;
- base user and administrator online journeys;
- the declared base initialization and operational batch cycle;
- transaction-type Db2 online and batch extensions;
- VSAM/MQ date and account request-response extensions;
- IMS/Db2/MQ authorization online and batch extensions;
- compatible reached assembler/Language Environment services;
- protocol-neutral CICS sessions and selected TN3270 compatibility;
- owned operator commands and the bounded FTP/JES compatibility needed by the
  accepted repository workflows; and
- full restart, security, failure, resource, and overload evidence.

Do not interpret “full” as permission to implement unrelated general Db2, IMS,
MQ, HLASM, TN3270, FTP, or z/OS features. Implement the exact reached surface
and fail explicitly outside it.

## Persistent program control

Maintain:

- `docs/delivery/CARDEMO-0.1.1-STATUS.md`; and
- `conformance/0.1.1/evidence/program-status.json`.

At every continuation, read them before planning. Record the current issue,
completed issue commits, commands and exit codes, exact evidence paths, corpus
identity, dirty-tree identity, open decisions, blockers, and next smallest
executable step.

Work on exactly one gap-matrix issue at a time. Preserve user changes. Do not
rewrite 0.1 historical evidence to make 0.1.1 pass.

## Issue and commit protocol

For each `CD-NNN` issue:

1. Add a focused failing regression derived from the real pinned corpus.
2. Implement only the bounded issue and unavoidable contract support.
3. Run narrow package tests and the issue acceptance command.
4. Run architecture/profile/schema checks affected by the change.
5. Generate issue evidence from commands and observations.
6. Update both status ledgers.
7. Review the complete diff and stage only the issue.
8. Commit locally with the exact `commit_subject` in the gap matrix and body:

   ```text
   CardDemo-Issue: CD-NNN=pass
   Evidence-Digest: sha256:<canonical issue digest>
   Target-Product: 0.1.1
   ```

One commit must not mark multiple open issues pass. Focused follow-up repair
commits are allowed but do not replace the issue completion commit.

## Implementation invariants

- Production code is generic and contains no fixture-output shortcuts.
- CardDemo sources and native runtime objects are not compiled dependencies.
- Runtime archives are oracle-only; there is no native or legacy fallback.
- Compiler stages remain deterministic and synchronous over explicit source
  closures.
- Syntax recovery, unresolved names, unsupported constructs, and incomplete
  host lowering cannot publish executable artifacts.
- CICS, SQL, DLI, MQ, dataset, JES, security, clock, and audit access use typed
  scoped host services.
- Program transfer and calls preserve principal, capabilities, generation pins,
  trace, limits, COMMAREA/linkage bytes, and normal control-flow identity.
- Every mutating base record/index, Db2, IMS, MQ, job, application install, and
  syncpoint path has intent/result, idempotency, retry classification, unknown
  outcome reconciliation, and restart behavior.
- No provider returns success for an unimplemented command, utility, SQL/DLI/MQ
  operation, or missing resource.
- Every queue, cursor, session, map, screen, record, source expansion, compiler
  structure, job, spool, database result, and retained state has a configured
  bound.
- Security defaults deny at application install, transaction admission,
  program transfer, and every sensitive provider call.
- Passwords, card data classified secret, BMS protected fields, MQ secrets, SQL
  credentials, and local paths are redacted from evidence and logs.

## Compatibility copybooks and external services

Do not copy unlicensed IBM product source into the repository. Represent the
reached DFHAID, DFHBMSCA, SQLCA, and MQ constants/layouts as owned compatibility
contracts backed by published behavior and pinned CardDemo observations.

COBDATFT, MVSWAIT, CEEDAYS, CEE3ABD, CBLTDLI, and MQ entry points receive one
explicit disposition: compiled owned source, compatible ProgramService/host
service, or accepted unsupported correction. A handwritten routine must match
reached input/output, condition, mutation, and failure behavior and cannot be a
generic success stub.

## Application package and install gate

The generic package must validate all references before committing:

```text
sources -> copybook libraries -> published program artifacts
BMS source -> maps and fields
CSD -> programs, transactions, mapsets, files, queues, Db2/IMS/MQ resources
dataset catalog -> seed objects, keys, indexes, GDGs, PDS members
security manifest -> principals, classes, resources, access
```

Install through durable migrations. Crash before the commit leaves no selected
partial generation. Crash after the commit can resume idempotently. Upgrade and
rollback retain version compatibility.

## Cumulative gates

Run a cumulative profile gate only after all of its dependencies pass:

1. `carddemo-base-online`: CD.J01–CD.J09.
2. `carddemo-base`: CD.J01–CD.J12.
3. `carddemo-db2`: CD.J13–CD.J14 plus base.
4. `carddemo-mq`: CD.J15 plus base.
5. `carddemo-authorization`: CD.J16–CD.J18 plus Db2/MQ/base.
6. `carddemo-full`: CD.J01–CD.J20 and all 27 issues.

Every journey runs through the public application install, terminal/session,
job, or operator entry point. Direct provider tests supplement but do not
replace selected-route evidence.

At minimum compare:

```text
compile diagnostics and source provenance
program artifacts and catalog generations
terminal screens, fields, AID, cursor and protected values
ordered host effects, CICS EIB/conditions and transfers
dataset records, keys, AIX order, GDG/PDS catalog state
SQL rows/cursors/SQLCA and commit/rollback
IMS hierarchy/PCB/status/checkpoints
MQ messages/correlation/queue state
JES jobs, steps, return codes and spool
restart, reconciliation, authorization and resource counters
```

## Stop-the-line rules

Stop and record a blocker when:

- the corpus is dirty, missing, or differs from the pin;
- a required upstream input is absent and behavior cannot be derived safely;
- an issue would require breaking an existing supported 0.1 contract without a
  negotiated version/migration plan;
- a mutation cannot be made retry-safe or explicitly non-retryable;
- security denial or provider failure could become success;
- native oracle code would enter production closure;
- evidence cannot be derived from an actual application route; or
- the `CDV1 -> COCRDSEC` decision remains unresolved at the full-profile gate.

Do not stop merely because the compiler or application work is large. Persist
the ledger, narrow the next failing corpus case, and continue.

## Final handoff

When all 27 issues and 20 journeys pass:

- produce a clean derived CardDemo-full exit report and evidence digest;
- run full workspace format, check, test, Clippy, doc, architecture, supply
  chain, SQLite/PostgreSQL, restart, backup/restore, and overload gates;
- confirm no native oracle/fallback or conformance dependency enters production;
- leave the version and release-order decision explicit; and
- do not tag, push, publish, or deploy without separate authorization.
