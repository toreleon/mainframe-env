# Transaction participant contract, version 1

Status: **Frozen early additive INT-1601 prerequisite**
Owner: **execution-contract and coordinator maintainers**
Scope: **provider-neutral transaction participant capabilities and obligations**
Applies from: **mainframe-env 0.16.0 early contract**
Contract: `mainframe-env.transaction-participant@1`

## Boundary

Version 1 describes what a mutating provider must declare before it can join a
shared unit of work. It does not introduce a participant dispatcher or a second
transaction runtime. The accepted `ExecutionCoordinator`, canonical effect
journal, store CAS operations, and service-specific fenced reconciliation
remain the only execution authorities.

The readable authority is
`conformance/0.16/contracts/transaction-participant.json`. The generator checks
its semantic invariants and writes the bounded Rust projection consumed by the
execution contract. The Draft 2020-12 schema fixes the serialized shape. A
reader accepts version 1 only; additive optional capability vocabulary may be
added within version 1, but changing an existing meaning requires a new
contract version.

## Shared ordering

Every participant uses one effect order:

1. observe the finite deadline and live cancellation state;
2. validate the bounded request, capability, and execution context;
3. persist the canonical coordinator effect intent;
4. authorize the typed resource and mutation intent before mutation;
5. apply the participant mutation at most once for that dispatch identity; and
6. persist audit plus a result, or preserve an explicit unknown outcome.

The corresponding logical lock/CAS order is coordinator intent, participant
state fence, canonical resource locks, participant UOW/replay publication, then
coordinator result. Store transactions are not held across provider dispatch.
Resource locks use deterministic provider/resource identity order. A stale
lease or recovery epoch cannot finalize a newer owner's effect.

## Capability limits

Prepare is explicitly one of not applicable, durable-intent only, or provider
prepare. Durable intent is not a vote and does not claim two-phase commit.
Commit and rollback applicability is per execution context. Rollback covers
only uncommitted recoverable work; it is not compensation after commit.
Automatic compensation is prohibited in v1, and any future explicit
compensation must remain service-specific.

The closed outcome vocabulary is committed, rolled back, failed, heuristic
commit, heuristic rollback, heuristic mixed, in doubt, and unknown outcome.
Each accepted participant must partition that vocabulary between outcomes it
can report and outcomes it does not produce. It may not collapse heuristic,
in-doubt, or unknown observations into success. An unknown post-dispatch result
is reconciled only by a fenced observation of the participant's authoritative
replay/UOW state; it is never automatically redispatched.

Idempotency is scoped to execution, run unit, and effect sequence and lasts for
the configured durable idempotency-retention window. Reuse after safe archival
and pruning is a new operation, not exactly-once execution. Principal,
delegation, authorization decision, correlation, causality, deadline,
cancellation, and audit identity remain attached across every admitted
boundary.

## Accepted and pending mappings

CICS is the sole accepted early mapping. Its local task and DPL mirror with
`SYNCONRETURN` own their syncpoint. DPL without `SYNCONRETURN` and
`EXECUTIONSET=DPLSUBSET` are subordinate contexts and reject explicit
SYNCPOINT with `INVREQ` 16/200 before UOW mutation. CICS uses durable intent,
not a prepare vote; it reports committed, rolled-back, failed, and unknown
outcomes and explicitly does not produce heuristic or in-doubt classifications
in this boundary.

Db2, IMS, and MQ entries are capability-pending. Their current provider code,
private state, or historical fixtures do not become accepted participant
bindings merely because the names appear in this contract. Each owned release
must supply its descriptor plus minimum mutation, failure, replay, and restart
proof before integration.

IMS has an optional `preparation` object under v1. It names only the bounded
local database contract test and the INT-1601 obligations still blocking
admission. The generated descriptor exposes `preparation_scope`,
`preparation_contract_test`, and `blocked_obligations`; readers of the earlier
shape need no preparation object. This metadata never supplies capabilities,
permits dispatch, or grants coverage. CICS keeps its existing accepted mapping;
Db2 and MQ retain their pending/null declarations. Removing a blocker or
promoting IMS without an owned reviewed extension fails validation.

The IMS test invokes `ims_providers` over generic HIDAM metadata. It exercises
local Interactive and generic Batch explicit commit/rollback, canonical
request conflicts and replay, authorization denial/failure before mutation,
missing-idempotency rejection, and separate SQLite processes retaining commit,
undo and a post-publication unknown result. A failed replay clock leaves the
insert durable while returning `UnknownOutcome`; an exact retry consults the
retained receipt without applying that insert again. That is local replay
evidence, not fenced coordinator reconciliation or licensed IMS equivalence.

The application-backout integration changes generic Batch settlement: writes
keep the same local undo until an explicit boundary. Clients relying on implicit
per-write settlement must explicitly commit or checkpoint. The legacy definition
route retains its prior Batch policy. Existing pending/null participant metadata
is unchanged; neither this behavior nor a bounded leaf seal accepts INT-1601.

The remaining acceptance gaps are concrete:

| Obligation | IMS gap requiring owner work before admission |
|---|---|
| owner / modes | Generic database Batch writes retain witnessed undo until explicit commit/checkpoint/backout; legacy definition-route Batch writes retain immediate settlement. These are local projections, not an accepted IMS execution-context/syncpoint owner. Distributed and authentic TM participant contexts remain pending. |
| ordering / fencing | Atomic provider-row CAS, local postimage witnesses and reservation fences protect database dependencies across run units. They do not atomically bind coordinator effect/recovery ownership or the WorkStore lease to publication, and do not establish accepted shared resource-lock/participant admission. |
| deadline-cancellation | The route uses the deadline as a replay metadata lower bound; it has no trusted current-time deadline check or live cancellation probe. Coordinator pre-dispatch checks alone do not prove the full participant boundary. |
| security-audit | The authorized constructor checks typed IMS PSB/database/UOW resources. The public service also has unauthorised constructors; no participant admission enforces composition. It does not itself publish denial/failure audit or atomically publish audit with mutation/replay. Principal-only authorizer calls do not establish delegation/decision propagation. |
| retention | Clockless replay remains conservatively unresolved. Protected replay metadata is useful but does not prove a participant UOW/checkpoint/audit retention watermark and all idempotency lifecycle obligations. |
| compatibility | This slice proves Memory behavior and SQLite process restart only. PostgreSQL, accepted coordinator journal/recovery composition, audit failure/saturation and coherent backup/restore are not supplied by these tests. |

These gaps require the existing IMS, coordinator, store/security and product
composition owners; this slice adds no private replacement authority. Local
commit and rollback do not establish provider prepare, post-commit compensation,
heuristic or in-doubt classification, universal 2PC, or exactly-once behavior.

Source context: IMS 15.6 baseline
`ibm-ims-15.6-recovery-utilities-2026-09-11`, topics
`SSEPH2_15.6.0/com.ibm.ims156.doc.apr/ims_basicchkpcall.htm` and
`ims_rolbcall.htm`, plus
`SSEPH2_15.6.0/com.ibm.ims156.doc.apg/ims_chckpntcallsintro.htm`.
Basic CHKP establishes a commit point and loses database position; ROLB backs
out changes and returns control; neither CHKP nor ROLB supports ODBA. Checkpoint
context differs across MPP/IFP/BMP/JMP/batch, and symbolic restart requires XRST.
The reference catalog is
`ibm-ims-15.6-dli-2026-08-31:dli-call-families:0002/:0017/:0023`.
Offline source review gives no official verdict or licensed execution credit;
the licensed gate remains 0/25 pending and mixed-resource closure stays v0.16.

## Compatibility and recovery ownership

The coordinator owns canonical effect schema, intent/result transitions,
deadline/cancellation observations, and recovery leases. The participant owns
its versioned UOW, undo, resource, and replay rows and their readers,
migrations, rollback procedure, and retention descriptions. A compatible
restore must preserve both sides and every referenced checkpoint, audit, and
replay identity; independent provider restore is not mixed-resource closure.

CICS continues to write `MECU2`, read `MECU1` and `MECU2`, use
`MECUNDO1`, and publish outer replay through `MECER003`. No durable schema or
public route changes in this early contract.
