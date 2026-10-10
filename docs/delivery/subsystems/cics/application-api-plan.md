# CICS — Application API

Subsystem: **cics**
Phase: **application-api**

Status: **Proposed**
Start gate: cobol.execution COBOL host ABI, racf.security SAF, and dataset.data data authorities frozen;
R-01–R-28 integrated and post-review entry gate accepted
Completion dependencies: cobol.execution, racf.security, dataset.data
Estimate: 18–28 engineer-months

Current readiness: **CIC-901 complete at its non-release implementation
boundary**. The full cics.application-api milestone remains **Proposed**: CIC-902 through
CIC-906, release work, and licensed differentials are not complete. PR
[#131](https://github.com/toreleon/mainframe-env/pull/131) merged the hardening
changes as `8b7459ab9d9c23e3b872314323d3e30020e13f31`. The integrated acceptance
subsequently passed on candidate
`2f3120ae4e294ce01b7058a139554c186b65903e`, tree
`c2961adbcc96060746c87ac1c96f7f11bb8f73f0`; local Jenkins build #8 is
authority only for that historical identity. CIC-901 is implemented by focused
source, contract, compiler, compatibility, and boundary commits culminating in
feature candidate `49ae7c9dbb763c8a88f42a041a58e9e99b59d05c`; its source, workspace,
architecture, documentation, MSRV, dependency-policy, and PostgreSQL parity
gates pass. Preserve the historical
[subsystem review](../../../reviews/SUBSYSTEM-REVIEW.md), including its P2/P3
baseline requirements. Those historical receipts remain separate from the
current CIC-901 validation. Track the boundary in
[CICS application API progress](application-api-status.md) and
[#137](https://github.com/toreleon/mainframe-env/issues/137).

## Post-review entry gate

Before broad CIC-901 work, the status document must identify **Ready for
CIC-901**, the tested hardening SHA/tree, the cobol.execution/racf.security/dataset.data dependency receipts,
and references linking all R-01–R-28 findings to their individual fix commits
and focused regressions. The review's Gates A–D and required entry checks must
pass together on that unchanged candidate, including the accepted CICS pilot,
the affected memory/SQLite/PostgreSQL contracts, discovered tooling suites,
license policy, exact-target launch smoke, and archive reproduction. Reuse the
existing CI receipts and tooling; do not introduce another status-evidence
schema or copy hand-maintained pass counts into conformance evidence.

The review's original no-go remains a historical record. Issue closure, local
commits, or passing tests on different branches cannot replace the integrated
entry result. A passing entry gate permits implementation; it does not grant
new CICS coverage, version promotion, or licensed-equivalence credit.

## CIC-901 non-release implementation boundary

CIC-901 seals the source-derived contract and registry shape for all 263
application-command rows without implementing the 240 unready handlers. The
registry contains three typed runtime handlers, 20 explicit legacy compatibility
handlers, and 240 fail-closed unready handlers. It has no default handler,
`automatic_registration=false`, and logical registry digest
`sha256:26280361a09119b3001cff439a5c10a96845836b85901702f906c3ec74d59346`;
the generated Rust file SHA-256 is
`41ade7b7dfb0d023ab27ef99615027014757e2e141469f01117c136c86cbd03b`.
The generated contract is `frozen-with-bounded-ambiguities` with logical digest
`sha256:0b2bfe153834b053bda9e40155a5024e9c3ec225be575a5476119889c15b83f2`;
its physical file SHA-256 is
`55f2379e10a56e1fe271c21820a9cf6a534a97802e16e85d8d9d9693ee521fd3`.

This is a non-release implementation boundary, not cics.application-api completion. It grants
the 240 unready commands zero execution, coverage, semantic, and differential
credit. CIC-902 through CIC-906 must supply and integrate their family semantics.
Release generation, exact-target launch, archive reproduction, deployment, and
licensed campaigns remain pending; nested environment orchestration is not part of this work.

All three source batches are automatically accepted by a verifier independent
from the extractor: sources A/B/C cover 88/88/87 rows. Official IBM HTML was
fetched through browser control into the external cache; publication bodies are
not committed and no PDF or human approval ledger is used. Objective source
gaps were filled and reprojected, while unresolved product meaning remains
explicitly bounded rather than converted into invented semantics.

Together the batches contain 18,070 candidates: 16,307 verified and 1,763
bounded product ambiguities, with zero mismatch, reprojection, or blocking
finding in the accepted receipts. Their candidate file SHA-256 values are:

- sources A: `sha256:f0918433d7f96d51cc7861c12e5d4434cc5f42c977d3cfd8dd264d5d5c512ed7`
- sources B: `sha256:e83ee1054f46aa872473729e2a10937c2d60ce4429cf606fe9fec4d939f3156b`
- sources C: `sha256:828f942b652262830b9d2ce73604fcd9978e0767e26f021cc74f59f9fed13b33`

The review receipt logical digests are:

- sources A: logical `sha256:03bb2b83212da2ee3bcb6e6c155dae0bf10337ab6f4fea48c2e004b426faa76a`;
  file `sha256:bd8af2a410dce2d5c5748546ec845c92680238971744abddf5526428184a596f`
- sources B: logical `sha256:5ccbf511b13e8af2cba83b95e13a147104cc4bca42095b748a77decc24e7f8fd`;
  file `sha256:3da42fcb80c25c5ce6c3b7c41958b7f428963da90339398179a52be043dffe63`
- sources C: logical `sha256:b62ba10bf18685740cea2aa9e229f4e3f455a113eabb9edbb4ecd58ef112f369`;
  file `sha256:5c8a50eb5c5f822181028a8b0171bb3675bd13ea5fbed63cb2e6d0f457b330b6`

These source receipts authorize contract projection only. The external cache is
a development source store, not a licensed runner, runtime dependency, release
mechanism, or source of execution credit.

The contract carries one EIBRESP authority containing 121 normalized condition
names. Its allowed-name digest is
`sha256:52e48a742cff4299f9580e4d2c082d394849304d3ce589f8ae552d0fad6258cf`,
and its name/code-pair digest is
`sha256:e3eb9481136095de24a161e4e225eea43f3812aefa2222d8a3fdaa18bbbbeb78`.
`INT-1601.cics-participant` records two known mutating rows, 260 rows with
bounded effect classification, one explicit UOW-boundary row, and 261 rows with
bounded UOW participation. These counts describe contract certainty and grant
no execution or conformance credit.

The generated registry is compiler-facing shape, not an implementation claim.
Candidate-aware recognition validates source-derived command heads, COBOL
applicability, flag/value shape, required and forbidden discriminators, and the
known required/alternative/dependency/mutual-exclusion constraints before
handler readiness. Only the three existing typed routes lower through typed HIR;
20 existing handlers remain explicit legacy compatibility routes and all 240
unready rows fail explicitly without advertisement or fallback.

The common `NOHANDLE`/`RESP`/`RESP2` policy remains source-bound: `RESP2`
requires `RESP`, and an explicit `RESP` continues to receive the response even
when `NOHANDLE` is also present. This is implemented consistently on the typed
and legacy paths. The compatibility boundary binds the existing time handler to
the source-truthful `ASKTIME ABSTIME` row and admits exactly `INQUIRE PROGRAM`
through a generated compiler-only SPI descriptor; bare `ASKTIME`, other
`INQUIRE` forms, and `SET FILE` remain fail-closed.

Final validation passed the 574-test tooling discovery suite (one intentional
skip), the complete test suite, Rust 1.95 workspace check,
Clippy with warnings denied, rustfmt, cargo-deny, documentation, architecture,
cache-backed A/B/C projection and independent verification, and all 11 native
PostgreSQL parity selectors. Release and licensed campaigns were not run and
are not implied by this result.

The [shared validation contract](../README.md#shared-validation-contract) and
[hardened slice acceptance](../../../prompts/subsystems/README.md#hardened-slice-acceptance)
apply, including early participant-contract and licensed-harness preparation.
These requirements do not themselves certify implementation or waive an exit gate.

## Outcome

Implement all 263 pinned CICS TS 6.x application programming commands through
generated command identities and resource-semantic handlers.

## Owned scope

- Generate the 263-command API catalog, grammar, option constraints, EIB/RESP
  mappings, condition identities, and exhaustive handler registration.
- Complete program/link/XCTL/return, task/interval/storage, terminal/BMS,
  transient/temporary data, file, spool, journal, and document behavior.
- Complete channels/containers, BTS, web/HTTP, sockets-facing adapters,
  transforms, business transactions, and supported event APIs.
- Implement APPC/MRO conversation state and distributed-program-link behavior
  using transport-neutral contracts.
- Apply transaction, resource, surrogate, and command authorization through SAF.

## Work packages

| ID | Deliverable |
|---|---|
| CIC-901 | Generated API grammar, options, conditions, EIB/RESP, registry, and bindings to hardened execution/security contracts |
| CIC-902 | Program, task, interval, storage, and recovery command families |
| CIC-903 | Terminal/BMS, TSQ/TDQ, file, journal, and spool families |
| CIC-904 | Channels/containers, BTS, documents, web, and transforms |
| CIC-905 | APPC/MRO conversations and distributed program link |
| CIC-906 | Integrated cross-family security, concurrency, recovery, scale, and IBM differential campaigns |

These are parent milestones. Declare bounded slices under each parent in the
status document before starting it, with stable IDs, exact command/options and
row/obligation scope, module ownership, dependencies and acceptance gates.
Examples include separate program-link/return and task-storage slices under
CIC-902, file-update/file-browse/TSQ/TDQ slices under CIC-903, and separate
channels/documents/web slices under CIC-904. Subdivide further as the actual
semantic and review budget requires. Follow the common prompt contract for
slice completion commits and the final parent completion commit.

Each slice includes its applicable SAF/audit, condition, deadline/cancellation,
mutation/replay, recovery and limit tests before integration. CIC-906 extends
this coverage across families and concurrent workloads; those guarantees are
not deferred until that milestone. Newly incomplete behavior remains disabled
in public profiles, and the accepted pilot remains a required regression.

## Hardened runtime baseline

Use the resume-aware durable coordinator, canonical effect identities,
service-specific unknown-outcome reconciliation, typed resource SAF decisions,
and atomic audit/effect/lifecycle journal at shared authority boundaries.
Preserve the finite deadlines, cancellation probes, bounded worker lanes,
durable clock and fencing, session security, readiness and retention contracts
established by the hardening fixes.

Follow [provider object-row persistence](../../../contracts/PROVIDER-ROW-PERSISTENCE-V1.md)
and the [durable storage profile](../../../contracts/DURABLE-STORAGE-PROFILE.md).
PostgreSQL uses the shared immutable artifact adapter. New schemas require
explicit migration/read-version/rollback rules; retention must preserve active
recovery, audit, replay and checkpoint obligations.

Keep shared command descriptors/dispatch separate from family semantic modules
under [ADR-0009](../../../decisions/0009-current-package-topology.md). Preserve
R-26's module boundaries and review budget and R-27's public API documentation
ratchet as new commands and contract items are added.

## Execution-context and selected-route proof

CIC-901 must freeze command/option applicability by execution context, including
local tasks and DPL server programs. CIC-905 binds the pinned restricted DPL
API, syncpoint ownership, `SYNCONRETURN`, return and failure behavior to explicit
obligations; transport success cannot prove transaction semantics.

Each command-family slice requires at least one independent end-to-end proof
through compiled COBOL, the accepted host ABI, durable coordinator and selected
CICS provider route, plus its focused command/condition obligations. A direct
handler call, mocked host result or the pilot's twelve scoped credits cannot
substitute for this route proof or complete a whole family.

## Parallelization

Command families can run in parallel after the generated option, condition,
resource-key, and effect contracts freeze. File and security semantics merge
through the dataset.data and racf.security authorities; host-call changes merge through cobol.execution.

After the hardening entry gate, cics.application-api can run alongside remaining jes.execution work and
db2.core, ims.programming, and mq.programming. SPI/FEPI parser preparation
may begin, but its public surface cannot complete before this API is stable.

## Backend and failure validation

PostgreSQL is an affected cics.application-api environment. Select the following tests according
to each slice's changed boundary and run the complete affected matrix on the
unchanged phase-integration candidate. An ignored or unavailable backend test
remains pending. Use focused tests in the inner loop; the common validation
tiers and CI path/contract selectors determine when broader suites run.

| Boundary | Required backend/environment | Required observations |
|---|---|---|
| Command/options, EIB/RESP and conditions | Deterministic unit fixtures plus the selected CICS product route | Exact success/condition bytes, malformed/boundary input, no silent operand drops |
| Shared journal, SAF/audit and mutation/replay | Memory, SQLite, PostgreSQL affected backend contracts | Deny before mutation; provider success followed by journal failure stays unknown; audit saturation is explicit; reconciliation does not duplicate an effect |
| Online resume, file/queue/link/syncpoint recovery | SQLite and PostgreSQL product compositions; retain the memory pilot for non-restart behavior | Crash-point restart, preserved outcome categories, validated checkpoint/artifact identity, no duplicate committed mutation |
| Worker ownership, cancellation and concurrency | Deterministic memory controls plus SQLite/PostgreSQL durable tests | Stale-owner rejection, lease/deadline expiry, concurrent writers, cancellation and bounded shutdown |
| Retention, capacity and artifacts | Memory/SQLite/PostgreSQL for affected state; multiple PostgreSQL adapter instances for shared objects | Quota enforcement, safe capacity reclamation, retained audit/replay/checkpoint guarantees and shared immutable artifact visibility |
| IBM differential | Protected licensed CICS environment pinned for each applicable family | Independently captured command-level behavior matched to the candidate and reviewed obligations |

Memory tests receive no durable process-restart credit. Family scenarios must
exercise owned product routes and independent expectations; a mocked host
result or the pilot's twelve scoped credits cannot establish whole-command
completion.

Extend the existing fuzz/model/coverage inventory for new CICS parser/decoder
and transaction/recovery boundaries. Keep coverage measurements diagnostic and
retain the discovered Python/PostgreSQL, schema, architecture/docs/API, MSRV,
and supply-chain/license gates. Existing compiler/store fuzz or model receipts
do not establish CICS coverage by themselves.

## Licensed campaign planning

At entry, record runner availability, exact licensed product/environment
versions, family support and missing capture/comparison coverage. The accepted
[licensed pilot adapter](../../../runbooks/cics-licensed-pilot.md) covers one
bounded file/UOW scenario; extend it through owned typed contracts for the
remaining applicable command obligations. Validate that plumbing with local
fixtures without granting licensed credit.

An unavailable licensed runner blocks the final differential gate, not
independent implementation after the hardening entry gate. Keep
`differential=pending`, retain the blocker and next external prerequisite, and
do not report cics.application-api complete until the required licensed campaigns pass.

## Exit gate

- 263/263 commands have explicit semantic handlers and pass every applicable
  gate and mandatory obligation in the reviewed execution-context matrix.
- Every advertised option affects behavior or fails explicitly; no unsupported
  command returns generic success.
- EIB, RESP/RESP2, condition handling, syncpoints, cancellation, distributed
  conversation, malformed input, resource bounds, and recovery matrices pass
  through the declared backend/environment matrix on the exact candidate.
- Licensed CICS TS 6.x application-interface differentials pass for every
  applicable command obligation; a successful pilot alone is insufficient.
- Existing CardDemo CICS transactions, maps, resources, and byte behavior remain
  exact without application identities in production logic.

## Non-goals

- CICS system programming interface and FEPI completion, which belong to cics.system-api.
