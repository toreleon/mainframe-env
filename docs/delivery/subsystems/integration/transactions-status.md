# Cross-resource integration — Transactions and recovery progress

Subsystem: **integration**
Phase: **transactions**

Status: **Early INT-1601 participant boundary sealed; integration.transactions is not complete**
Candidate base: `782c65830845ae2c0ddc7289977521fcaf3fa3fa` (`origin/main`)
Branch: `codex/parallel-integration.transactions-int1601`

## Bounded objective

This lane owns only the additive early participant contract required before
CICS, Db2, IMS, and MQ mutating adapters integrate. It preserves the accepted
execution coordinator, canonical effect journal, provider row stores, security
authority, recovery worker, and retention lifecycle. It adds no public route,
profile capability, mixed-provider behavior, universal prepare/2PC behavior,
exactly-once claim, or final integration.transactions coverage result.

The contract has no official coverage denominator of its own. Its accepted CICS
mapping is an executable compatibility boundary for application row `0218`
(`SYNCPOINT`) and the existing coordinator/CICS route; it grants no new row or
gate credit. Db2, IMS, and MQ remain pending participant bindings until their
owned releases supply mutation, failure, replay, and restart evidence.

## Declared INT-1601 slices

| Slice | Parent | State | Semantic scope | Dependencies | Acceptance gates |
|---|---|---|---|---|---|
| `INT-1601.participant-schema` | `INT-1601` | pass | Freeze `mainframe-env.transaction-participant@1`, its Draft 2020-12 schema, readable authority, generated execution-contract projection, strict validator, fixture shape, read-version rule, shared effect/lock order, and explicit CICS/pending-provider declarations. | Accepted coordinator/store/effect/security/retention contracts and CICS row `0218` source review | Schema compilation and instance validation; generator freshness; malformed/order/version/pending-binding rejection; execution-contract unit tests; docs and diff checks |
| `INT-1601.coordinator-bindings` | `INT-1601` | pass | Bind the accepted `ExecutionCoordinator` and CICS recovery handler to the shared descriptor without changing dispatch, routes, profiles, or CICS results. | `INT-1601.participant-schema` | Coordinator and CICS focused tests; typed-boundary and architecture guards; memory behavior unchanged |
| `INT-1601.compatibility-tests` | `INT-1601` | pass | Execute contract fixtures through compiled CICS, the durable coordinator, canonical effects, and the selected CICS provider; prove read-version and fail-closed context compatibility. | Prior two slices | Local commit/rollback, owned-DPL rollback condition, forbidden DPL contexts, journal ordering, replay identity, unknown-outcome non-redispatch, schema/fixture tests, SQLite compatibility where applicable |

`INT-1601` remains in progress after these early slices. Full prepare/commit/
rollback/compensation/heuristic/in-doubt/unknown mixed-resource closure,
operator resolution, coherent backup/restore, and the final provider matrix stay
owned by INT-1601 and INT-1604–INT-1606 on an accepted dependency candidate.

## Contract ownership and applicability

- Contract owner: `mainframe-env-execution-api`, projected from
  `conformance/subsystems/integration/contracts/transaction-participant.json`.
- Runtime authority: the existing `ExecutionCoordinator`; no participant may
  add another effect journal, recovery ledger, security evaluator, or retry
  loop.
- CICS participant authority: the existing CICS task/UOW handler and its
  `cics-uow`, `cics-uow-undo`, and `cics-effect-replay-v1` rows.
- Public routes and profiles affected: none.
- Backends affected by implementation code: none; compatibility exercises the
  accepted Memory and SQLite boundaries. Existing PostgreSQL behavior is not
  relabeled as new candidate evidence by this slice.
- Exact mandatory obligations:
  `INT-1601.owner`, `INT-1601.modes`, `INT-1601.prepare`,
  `INT-1601.completion`, `INT-1601.compensation`, `INT-1601.outcomes`,
  `INT-1601.idempotency`, `INT-1601.ordering`, `INT-1601.fencing`,
  `INT-1601.deadline-cancellation`, `INT-1601.security-audit`,
  `INT-1601.recovery-schema`, `INT-1601.retention`, and
  `INT-1601.compatibility`.

## Source-backed CICS decision

Offline review used baseline `ibm-cics-ts-6x-file-uow-pilot-2026-09-08`
and the retained, manifest-matching topics below. These sources have zero
semantic or licensed-execution coverage credit.

- `SSJL4D_6.x/reference-applications/commands-api/dfhp4_syncpoint.html`,
  `sha256:2e1bebaa9ac35c7444eeb63d2e15d1773a5d39e06f0e65f00970e96f411f9b34`:
  commit covers recoverable changes since the prior syncpoint; DPL without
  `SYNCONRETURN` and local `EXECUTIONSET=DPLSUBSET` reject with 16/200; a remote
  inability to commit reports `ROLLEDBACK`.
- `SSJL4D_6.x/reference-applications/commands-api/dfhp4_syncpointrollback.html`,
  `sha256:566d8661a0af02559d8679234e959d2e2aa2577dcf14c55211ec07c7c03e2954`:
  rollback backs out recoverable changes since the prior syncpoint and is
  propagated only where that execution context supports it.
- `SSJL4D_6.x/administering/recovery/dfht2c0034.html`,
  `sha256:9faa985f20ae267453f4fdfecbc945a963c311eecbf1c72595dc580866442dfb`:
  UOWs end at explicit or owned implicit syncpoints and cannot span CICS tasks.

Consequently CICS v1 reports committed, rolled-back, failed, and unknown
outcomes. It does not claim provider prepare, automatic post-commit
compensation, heuristic classification, in-doubt classification, universal
atomicity, or exactly-once execution.

## Pending provider bindings and final dependencies

| Provider | Early binding | Required before integration |
|---|---|---|
| CICS | accepted mapping to the existing local and owned-DPL syncpoint boundary | The bounded compatibility suite in this lane; later public DPL and full mixed-resource obligations remain pending |
| Db2 | pending | Accepted db2.programming-owned participant capabilities plus mutation/failure/replay/restart evidence |
| IMS | pending | Accepted ims.programming-owned participant capabilities plus mutation/failure/replay/restart evidence |
| MQ | pending | Accepted mq.programming-owned participant capabilities plus mutation/failure/replay/restart evidence |

Final integration.transactions still depends on accepted jes.execution, cics.system-api, db2.programming, ims.programming, and mq.programming
candidates and completion of INT-1601 through INT-1606 on one unchanged
candidate. This status document must not be used as a release, licensed oracle,
provider-pass, or full-integration.transactions receipt.

## Validation disposition

The participant generator/freshness check, six malformed-contract tests, three
execution-contract unit tests, direct Draft 2020-12 instance validation, docs,
formatting, and diff checks pass for this slice. Current main already locks
`rustls` 0.23.45 for `RUSTSEC-2026-0285`, so this rebased lane carries no
`Cargo.lock` delta and does not require a separate dependency-policy rerun.

Two unrelated accepted-base failures were diagnosed once and left outside this
slice: the global schema pass reaches an overlong historical jes.execution CardDemo note,
and the full architecture-fast pass reaches an existing `CARDDEMO` word in a
dataset replay-index performance comment. The integration.transactions contract schemas compile,
their instances validate, and the new participant freshness guard passes before
that unchanged architecture ratchet failure.

The coordinator exposes the one validated contract and resolves descriptors
without selecting or dispatching a provider. The CICS recovery handler now
derives owned versus subordinate syncpoint applicability and exact rejection
from that shared descriptor. Focused coordinator, unowned-DPL no-mutation, and
remote-refusal rollback/replay tests pass; the static guard rejects a private
CICS rejection table or any early Db2/IMS/MQ binding.

The six CICS fixture cases execute through compiled typed CICS, the durable
coordinator, canonical effect journal, and selected CICS provider on Memory and
SQLite. They cover local commit/rollback, owned-DPL commit/forced rollback, and
both subordinate-DPL rejections with no UOW row. Every outer effect remains
canonical and identity-bound. A separate full-boundary failure injects a CICS
UOW clock failure, observes coordinator `UnknownOutcome` plus `MECU2` pending
state, and proves repeated admission does not redispatch or alter that row.
The accepted SQLite remote-refusal restart/replay regression also passes.

These executable checks are compatibility proof for the early boundary only.
They emit no official row/gate verdict, no licensed differential credit, and no
Db2/IMS/MQ provider pass. PostgreSQL was not rerun because this slice changes no
store or durable participant implementation; its existing accepted evidence is
not relabeled to this candidate.

## IMS-1406.participant-binding — prepared ims.programming slice; admission pending

Parent: `IMS-1406`; target: `ims.programming`. This provider-owned slice consumes the
early `INT-1601.participant-schema` prerequisite and prepares the IMS binding
required before INT-1601 adapter integration. It does not close INT-1601 or
integration.transactions mixed-resource integration. Candidate base is
`213ed878ec138bdb2914330db6613559bffc5a86`; branch is
`codex/v014-ims-participant-20261002`. The inspected implementation includes
the public recovery bridge. No official row or
gate credit is claimed; the licensed 25-family gate remains 0/25 pending.

Scope: inspect and declare the existing local database `ims_providers` route,
its run-unit undo/commit boundary, canonical request replay, authorization,
failure and SQLite process-restart behavior. TM execution, CardDemo, shared
Conformance IR, the IMS assurance matrix, and production coordinator/server
changes are outside this slice. Memory and file-backed SQLite are the bounded
test matrix; PostgreSQL and coherent mixed-resource restore remain pending.

The existing INT-1601 owner/modes/prepare/completion/compensation/outcomes/
idempotency/ordering/fencing/deadline-cancellation/security-audit/recovery-schema/
retention/compatibility obligations are the review checklist. Catalog context
is `ibm-ims-15.6-dli-2026-08-31:dli-call-families:0002/:0017/:0023`;
these supplemental contract tests emit no official verdict. IMS 15.6 source
review uses the hash-verified `ims-recovery-utilities-contracts` cache, notably
basic CHKP, ROLB and checkpoint execution contexts. Product local service-class
behavior must not be equated with every IBM execution environment.

Owners: readable participant authority/schema/generator/checker, generated
execution-api projection and bounded participant tests, a new IMS provider
integration test, this status, the participant contract documentation, and the
unique IMS participant changelog fragment. The manager also authorized the
bounded generated documentation-manifest update for these two documents.
No new store, UOW coordinator,
audit authority, recovery ledger, runtime dependency or public route is planned.
This is product contract preparation, reusing existing implementation owners.

Acceptance gates: focused schema/descriptor/freshness/binding Python tests,
execution-api participant tests, new IMS mutation/failure/replay/process-restart
tests, formatting, docs, changelog and dependency policy, followed by the exact
path work-package seal/check. Accepted IMS capabilities require every necessary
guarantee; if gaps need unowned production changes, preserve pending/null and
record only additive preparation plus concrete obligations for those owners.
The guarantee audit and focused tests are complete; admission remains pending.

The audit requires IMS to remain pending with null accepted capabilities. Its
optional preparation descriptor names the local provider test and blocking
INT-1601 obligations. The database route supplies local atomic object/replay
CAS and typed authorization in the authorized composition, but not effect
recovery-lease fencing, live cancellation or current-time deadline observation,
audit/effect atomic publication, a validated syncpoint/context owner, or complete
retention/backend compatibility. Batch mutation discards undo immediately;
rollback cannot be advertised universally. TM and distributed ownership,
coordinator reconciliation, PostgreSQL, and coherent restore remain unproved.
The exact guarantee/owner gaps are recorded in
`docs/contracts/TRANSACTION-PARTICIPANT-V1.md`; unowned production changes were
not guessed or added. CICS and the future Db2/MQ bindings retain their prior
dispositions. The prepared slice can pass its own tests while IMS admission,
parent IMS-1406, INT-1601, and the ims.programming/integration.transactions milestones remain pending.

Offline `ibm_docs.py search/read` verified the recovery scope's TOC and the
selected topic hashes using `ims-1405-topic-cache` (no network refresh). Source
baseline: `ibm-ims-15.6-recovery-utilities-2026-09-11`:

- basic CHKP `ims_basicchkpcall.htm`,
  `sha256:1029208c47f44b8a0144472c8127767b18140c57be70850fdfa00da83ef1743a`;
- ROLB `ims_rolbcall.htm`,
  `sha256:166bc5f6ac4b4a75be331419fd9a185d76867a3be2aa322316a8e385bca2158b`;
- checkpoint contexts `ims_chckpntcallsintro.htm`,
  `sha256:c3aaf84e538be688d44af8fe9072e6cb00c47e4f9e46277f2ba22aa053be013d`.

CHKP/ROLB forbid ODBA; checkpoint forms and restart differ by execution context.
This review does not map product ServiceClass to all IBM contexts or credit
catalog rows 0002/0017/0023. The selected sources were available and matching;
search reported unrelated topics outside this bounded cache as missing.

Validation: focused Python descriptor/binding tests, generated freshness,
execution-api participant tests, and the new IMS public-provider tests pass.
The SQLite parent test executes seed, rollback, unknown-result, and receipt
resolution in four separate processes over a file-backed database; the child
helper's ordinary no-environment return grants no evidence. Authorization and
malformed failures preserve every IMS row, replay preserves database versions,
and Batch rollback explicitly preserves its already-applied insert. These are
independent local expectations, not official row verdicts.

Formatting, dependency policy, changelog and diff checks pass. Architecture-fast
compiled and validated the participant schema/fixture instances and passed
participant, effect, provider-row, storage, authorization and retention guards,
then stopped at the unchanged missing CICS source
`SSJL4D_6.x/applications/designing/dfhp37p.html`. No refresh or broader campaign
was performed. The manager authorized normal documentation generation, limited
to the two owned documents' manifest entries, and its docs check before sealing.
The exact feature allowlist includes that generated manifest. Unchanged tests
and the independently confirmed CICS-blocked architecture gate are not rerun
for documentation hashes or commit metadata.
Receipts remain outside Git/target under the worker's `v014-completion-20261002`
cache in `ims-participant-binding`. No durable row schema or migration changed.

## Next executable step

Provider-owned lanes may now consume `mainframe-env.transaction-participant@1`
only after supplying their pending capability declarations and required
mutation/failure/replay/restart evidence. Keep parent `INT-1601` and integration.transactions in
progress until the full mixed-resource, recovery, operator-resolution, and
coherent-restore gates pass on one accepted candidate.
