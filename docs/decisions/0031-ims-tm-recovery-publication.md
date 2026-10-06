# ADR-0031: IMS TM recovery publication and work settlement

Status: **Proposed; shared contract-owner decision required**
Owner: **store/execution contracts and coordinator maintainers, with host and IMS owners**
Scope: **proposed shared publication/settlement boundary; no actual TM recovery admission**
Applies from: **mainframe-env current subsystem contracts**
Parent: **IMS-1405; mixed-resource closure remains INT-1601/integration.transactions**

## Bounded delivery

`IMS-1405.tm-application-backout` delivers this source/contract gap packet and
executable negative witnesses from clean base
`bf0dc41c798ea344a195e5c2b3d47bfed7908277`. It changes no runtime authority,
accepted participant descriptor, public shape, durable reader or schema.
Actual TM application backout is **unsupported**, not accepted. The DB-only
leaf and generic Batch undo/epoch/incarnation fences remain unchanged.
Official, licensed, maintainer, parent and release credit are zero.

## Existing authority and concrete counterexample

| Owner | Existing contract / missing guarantee |
|---|---|
| `tm/service.rs`, `tm/support.rs` | Signed package definitions select transaction, PSB, input, session and conversation. Commit/rollback build provider-row mutations, publish, then apply `ReplayWork` using a separate WorkStore call. There is no exposed owned recovery proposal. |
| `service/application_backout.rs` | One RecoverySession and generic DB UOW publish actual images, undo, PCB/Q effects and recovery receipt. Existing actual TM sessions reject before mutation. `has_session` is an observation; absence cannot fence concurrent admission. |
| `mainframe-env-store-api` | `ProviderStateStore::mutate_provider_states_atomic` fences only supplied row CAS versions. WorkStore owns typed claimed work, lease ID/epoch, expiry, deadline, cancellation, release and completion. No method joins those predicates to provider publication. |
| `mainframe-env-store` | Memory's work map is separate from its provider map. SQLite/PostgreSQL adapt work through the private `durable-work` codec. Provider writes to that encoding would bypass WorkStore validation and fail Memory parity. |
| `ExecutionCoordinator`, `StaleEffectRecoveryWorker` | Ordered canonical intent/result and lease-fenced reconciliation are retained. Intent observation before provider publication does not atomically fence a recovery claim that occurs after that observation. |
| `ImsRecoveryRequest/Result` | The public projection admits only DB-batch CALL. `Rolb` has no optional TM I/O area; `BackedOut.user_data` is savepoint data, not a returned input segment/PCB. Changing context validation alone cannot supply an authentic TM owner. |

The shared-store witness executes this schedule with real APIs:

1. A claims work at epoch 1 and observes a provider image at version 1.
2. B reclaims the expired work at epoch 2.
3. A atomically writes image version 2 and a receipt using valid provider CAS.
4. A's work completion rejects `LeaseConflict`; image and receipt remain.

Memory and two independently opened file SQLite adapters exhibit the same
boundary. Separate SQLite processes retain the partial disposition. Real
competing provider CAS allows one winner and rolls back the loser, proving
that provider atomicity works but covers a different predicate. A cancellation
observation likewise cannot be included in the publication call. These tests
document an unavailable guarantee; they do not approve stale TM publication.
They should be replaced by rejection assertions when an owned extension lands.

The closure audit must also cover existing ordinary DB Commit/Rollback/
Checkpoint and `service/application_recovery/checkpoint.rs`. Those DB-only
paths have no typed work predicate; the checkpoint adapter's scope/session
checks do not include the backout adapter's TM-session guard. A caller-supplied
DB-batch label is not authentic TM admission or proof of a common commit.
This delivery preserves those DB-only semantics and reports that seam rather
than claiming their separate publication is accepted TM CHKP. Even adding a
second absence check would leave the concurrent admission race unresolved.

The signed product witness installs a signed TM/DB package, enqueues and claims
actual conversational input, starts its session, consumes GU, mutates a real
database, and buffers ordinary and express output with one express PURG.
The source-derived fail-first ROLB expectation reaches `Unsupported`. The
retained regression requires rejection with identical IMS rows and WorkRecord,
including attempts to label the real session DB-batch. DB/DC context, other
backout calls, malformed/quota operands and SAF denial remain fail-closed.
Later GN and read-only observation cannot manufacture a recovery receipt.
This is negative route proof, not recovered execution or successful replay.

## Source identities and applicability

Reference product is **IMS 15.6, `SSEPH2_15.6.0`**. Catalog:
`ibm-ims-15.6-dli-2026-08-31:dli-call-families:0017` ROLL/ROLB,
`:0018` ROLS, `:0020` SETS, `:0021` SETU. Dependencies are `:0002/:0023`
CHKP, `:0005` GU/GN, `:0008` ISRT and `:0024` TERM; PURG is supplemental.
All paths below start with `SSEPH2_15.6.0/`.

Baseline **`ibm-ims-15.6-recovery-utilities-2026-09-11`**, manifest
`conformance/subsystems/ims/manifests/ims-recovery-utilities-contracts-topics.json`:

| Topic | SHA-256 | Decision constraint |
|---|---|---|
| `com.ibm.ims156.doc.apr/ims_rolscall.htm` | `b7e15d0c110d3296eac11d895326b3ef48ac913fd682b94b312aa6c59ad14af5` | Intermediate continuation differs from prior-commit suspension; DEDB/MSDB restrictions remain. |
| `com.ibm.ims156.doc.apr/ims_rollcall.htm` | `01a33e88387636985ef575bdd7bdb99e0d3ce6a794dcf227d2f1e75c843ec61f` | U0778 terminates without return. |
| `com.ibm.ims156.doc.apr/ims_rolbcall.htm` | `166bc5f6ac4b4a75be331419fd9a185d76867a3be2aa322316a8e385bca2158b` | Returns control; optional I/O area can return input. |
| `com.ibm.ims156.doc.apr/ims_setssetucall.htm` | `53b9a76d65aed978e2eca2d9c10a295bea6abf16e88cb5f6949e3effa5709336` | Named point and saved area authority must share the current interval. |
| `com.ibm.ims156.doc.apg/ims_backingoutintermediate.htm` | `73e987b85ca10963e4bfc68e83c4612433689052b7baba385eaff735ae42f4f2` | Restore point-relative DB/message state and lose DB position, not prior committed work. |
| `com.ibm.ims156.doc.apr/ims_basicchkpcall.htm` | `1029208c47f44b8a0144472c8127767b18140c57be70850fdfa00da83ef1743a` | MPP/message-driven BMP CHKP returns the next input and loses DB position. |
| `com.ibm.ims156.doc.apg/ims_chckpntcallsintro.htm` | `c3aaf84e538be688d44af8fe9072e6cb00c47e4f9e46277f2ba22aa053be013d` | MPP/IFP use basic checkpoint; BMP/JMP/batch forms differ. |

Baseline **`ibm-ims-15.6-tm-contracts-2026-09-11`**, manifest
`conformance/subsystems/ims/manifests/ims-tm-contracts-topics.json`:

| Topic | SHA-256 | Decision constraint |
|---|---|---|
| `com.ibm.ims156.doc.apr/ims_isrtcalltm.htm` | `9b0bd68473b41b3614641637776047ba148e2f28d9d5133ec0d78cf931560a31` | Message groups complete at new GU/commit or express PURG. |
| `com.ibm.ims156.doc.apr/ims_purgcall.htm` | `3b6414156c4c76da888278ff17a1a3d8ed9cedfee1068373f11c46588719e0a6` | Completed output, optional next segment, and express/nonexpress behavior depend on context; spool rules are not universal TM rules. |
| `com.ibm.ims156.doc.apg/ims_conversationrecovery.htm` | `5afd0e6ec7fd527047ff0ecbb30be819cfb11fc9efa894adae232299c9fbc6c3` | Previous conversation steps remain committed; current-step ordinary output backs out; completed express output survives. |
| `com.ibm.ims156.doc.ccg/ims_tm_plan_terminals_msgsched.htm` | `38a382955cc478134c1345691d29fd1943511032c08418e4f3ca215aafff5a56` | Program scheduling is transaction/context dependent, not an arbitrary run label. |

Twenty selected registered topics (including GU/GN, I/O PCB, checkpoint and
conversation dependencies) were verified against committed SHA/byte pins and
read with explicit bounded cache via `ibm_docs.py search/read` and its local
plain-text parser. Retained topic paths were absent; matching raw SHA archive
bodies and pinned TOC supplied review. Selected bodies unavailable/mismatched:
none. Search reports omitted unrelated recovery/programming topics because the
cache is intentionally partial; that is not an unavailable required source.
No whole-cache audit, network/browser refresh, or source body in Git occurs.

The pinned recovery topics link to the retained prior-commit supplement
`com.ibm.ims156.doc.apg/ims_backingoutpriorcommit.htm`, independently verified
at 21,191 bytes and SHA-256
`38fab0bbc03546ddaf2cfecbb92c0a19631f32c79eb24f29ba25ffe56b639b23`.
This is an archive-index identity, **not a newly registered semantic baseline**.
It distinguishes unpurged express buffers (cancel) from PURGed complete
messages (survive), ROLB input-area/GN behavior, ROLL current-input discard
and earlier-input requeue, and tokenless ROLS U3303 suspension/requeue.
Discardable APPC input has another disposition. Broad APPC/CPI-C behavior
cannot be inferred from a generic transaction row. The registered
`ims_conversationstate.htm` is specifically CPI Communications state, not a
universal SPA checkpoint model. No reviewed source establishes a mapping from
every local ServiceClass/TmExecutionContext to these execution environments;
that admission decision and any supplemental source registration remain open.

## Proposed shared boundary for owner review

Prefer one additive **typed conditional provider publication** on the existing
shared store authority, with a default unsupported/fail-closed method for old
implementors. The name and Rust shape are owner decisions, not new normative
ABI in this proposal. One bounded request would carry:

- Exact WorkRecord identity: work/execution, selector, artifact, generation,
  scheduling incarnation, claimed lease ID and epoch. Core WorkStore owns
  decoding, lease/expiry/deadline/cancellation validation and transitions.
- Exact canonical effect identity/domain, dispatch owner/attempt and intent
  epoch, or explicitly authorized recovery owner/epoch. Coordinator authority
  must reject unknown, superseded or recovery-claimed dispatch ownership at
  the same publication linearization point, not via a prior read.
- A trusted finite clock observation and bounded provider mutations/dependency
  predicates, including TM session/message/conversation, selected package,
  actual DB/undo/Q state, RecoverySession, result and settlement receipt.
- A typed work action: keep the current live lease for intermediate/returning
  operations, or an owner-defined settlement for terminal/requeued/suspended
  input. ROLB is not ordinary `TmCall::Rollback` session deletion/release.

The preferred operation publishes provider rows, required audit/result and
work settlement **inside one backend transaction/Memory journal**. Validate
all predicates and capacity before any visible mutation. Returning success
must prove the exact chosen disposition. Conflicts, stale lease, cancellation,
deadline, malformed rows or capacity failures leave all authorities unchanged;
acknowledgement loss is UnknownOutcome. Do not hold a transaction over program
execution. Reuse core work/journal codecs, migrations and quota counters;
providers never edit opaque core rows or acquire a private lock service.

If maintainers choose retained asynchronous settlement instead, they must first
define a shared pending-settlement/claim fence: no newer claim may overtake the
published disposition before settlement resolves. The existing `ReplayWork`
tuple is insufficient. Retain explicit pending/applied/conflicted/unknown
resolution with exact work incarnation, lease and effect identities. Queued or
Completed alone cannot prove that this particular receipt caused settlement:
an unrelated later claimant may have produced that state. A fenced repair may
finish only the retained disposition; read-only observation never repairs,
redispatches the original call, or resolves a core unknown effect on its own.
This alternative's linearization and crash contract needs its own reviewed
backend tests before use; two sequential commits cannot be called atomic.

An optional composed constructor must require the same legitimate shared
authority; existing separate-store constructors remain unchanged and reject
the new class. Pointer equality or one process mutex cannot establish a
durable common authority across reopen or another process. Backward-compatible
capability probing must not admit old/fake implementations through a default
success. Existing frozen participant capabilities remain pending/null until
their owner approves the supported guarantees.

## IMS and host decisions needed after that boundary

1. Expose the existing TM mutation computation as an owned staged helper that
   cannot publish by itself. Compose it with the existing RecoverySession and
   witnessed DB UOW in one publication. No private recovery ledger or second
   TM queue may be introduced. All ordinary commit/CHKP/terminal paths must
   use this authority once admitted, or reject the mixed context.
2. Bind TM admission and DB recovery to one scheduling incarnation and interval
   fence; concurrently creating a TM session must conflict with DB-only
   settlement. Decide the common row/proof owner through the shared boundary,
   not by another absence scan or provider-local lock.
3. Capture input history/cursor and current conversation baseline at commit
   and each point. Preserve earlier committed SPA steps and output. Track
   ordinary pending outputs and both buffer classes; never undo already
   completed express PURG output. Output identities need an incarnation and
   monotonic message ordinal: current express PURG does not advance
   `pending_output_ids`, and run/PCB/ordinal alone can collide on a later PURG
   or reused run. No prior receipt may erase a later output or input claim.
4. Decide suspended versus runnable requeue, discardable input and terminal
   reasons. Current `TmMessageState` lacks suspended/discarded states and
   `WorkDisposition` has only Complete/Release. ROLL must not turn into a
   generic reschedule; tokenless ROLS must not become ordinary completion.
   ROLB retains continuation with source-defined input/PCB behavior.
5. Extend the host projection additively for authentic context and optional
   ROLB input area/result, with appended canonical tags and unchanged prior
   vectors. Do not overload savepoint user data. Raw LLZZ/AIB, region/log/BKO,
   CMPAT and APPC/distributed authority are still separate owners.
6. Declare bounded retention and coherent restore for both work/effect and
   TM/DB/recovery/settlement rows. Legacy receipts without new authority remain
   readable but never gain restore/settlement permissions. Drain old writers;
   live downgrade requires compatible readers or a coherent verified backup.

## Required acceptance after an owner decision

| Class | Required proof; currently pending unless explicitly a gap witness |
|---|---|
| Admission/normal route | Source-derived fail-first signed scheduled TM, real claim/start/GU and DB updates, selection/context/PSB and exact lease/incarnation. |
| Recovery | Named points and interval fences; ordinary pending/unpurged express/PURGed express; conversation continuation, prior steps and input history; ROLB area/no-area, ROLL discard/U0778 and ROLS suspension/U3303. |
| Race/controls | Authoritative stale lease and real competing CAS, concurrent admission, core recovery claim, cancellation/deadline and SAF/malformed/quota/audit saturation: no partial publication. |
| Uncertainty | Crash before/after publication/settlement, lost acknowledgement, retained read-only observation, fenced repair and canonical replay after later work; conflicting or unprovable settlement stays unknown. |
| Backends/compatibility | Shared-owner Memory, file SQLite reopen and substantive subprocess phases, PostgreSQL where admitted, old/fake default rejection, reader/migration/rollback/retention and coherent restore. |

The current signed rejection and shared-store counterexamples are substantive
investigation evidence only. Lost acknowledgement, settlement repair,
suspension, successful TM replay and acceptance backend phases are not run as
unsupported mock successes. The implementation gate remains open. Owner action:
approve the shared conditional publication/settlement contract and authentic
host/context projection, then implement the TM helper in its existing owner and
replace these witnesses with the required positive and negative matrix.
