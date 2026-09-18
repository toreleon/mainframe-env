# Execution and Durability Architecture

Status: **Accepted by repository owner**
Owner: **execution and store maintainers**
Scope: **execution lifecycle, effects, work, checkpoints, and durability**
Applies from: **mainframe-env 0.1.0**

## Design outcome

mainframe-env owns an explicit, checkpointable execution state machine. Tokio,
Tower, PostgreSQL, SQLite, object storage, and messaging systems implement the
asynchronous shell and stores; they do not define execution semantics.

## Invocation model

An invocation carries:

```text
execution and run-unit identity
parent/child identity
selector and immutable artifact identity
principal and grants
service class and priority
deadline and cancellation identity
resource limits
resource bindings
trace and audit correlation
idempotency key and attempt
plugin generation constraints
```

Identity is minted once at admission and propagated through every child
invocation and host effect.

## Execution outcomes

The logical outcome taxonomy is closed and versioned:

```text
Completed
Condition
Suspended
Invoke
Transfer
Abend
Cancelled
TimedOut
Rejected
ResourceExhausted
ProviderFailure
InfrastructureFailure
```

Protocol gateways map these outcomes to HTTP, z/OSMF, CICS, JCL/JES, terminal,
or CLI responses. Gateways cannot collapse categories before audit and durable
state are recorded.

## Deterministic machine driver

The machine executes a bounded quantum and returns one drive action:

```text
machine state + resume input
        |
        v
execute deterministic quantum
        |
        +-> continue with updated state
        +-> request one ordered host effect
        +-> invoke/transfer child program
        +-> persist suspension/checkpoint
        +-> complete
        +-> fail explicitly
```

One ordered effect at a time is the reference behavior. Batching is an
optimization permitted only when ordering, failure, and replay semantics remain
identical.

## Host-effect protocol

A host effect is an owned typed integration request, not a syntax transport.
Under
[ADR-0011](../decisions/0011-typed-language-hir-and-semantic-ir.md), providers
never receive source text, language HIR, or statement token lists. Executable
semantic operations carry pre-resolved resource and output bindings; the
machine evaluates runtime values, authorization context, provider generation,
and transaction state before emitting the request. Provider and store
authorities remain unchanged as semantic families migrate.

For a mutating effect:

1. Validate request, capability, principal, bounds, transaction, and deadline.
2. Allocate a monotonic effect sequence in the run unit.
3. Persist the effect intent, idempotency key, typed capability, dispatch
   owner/attempt, durable creation and recovery-not-before ticks, and
   execution-event epoch.
4. Invoke the provider.
5. Persist success, condition, failure, or unknown outcome.
6. Feed the typed result into the machine.
7. Persist the next machine state or terminal outcome.

Infrastructure retry never assumes that an external mutation did not occur.
Unknown outcomes remain explicit and require service-specific reconciliation.
If result persistence fails after a mutating provider was dispatched, the
caller receives `UnknownOutcome` even when the provider returned a usable
success. A bounded recovery worker enumerates only sufficiently old intents,
claims each under an expiring owner/attempt/epoch fence, and asks a
service-specific resolver to consult the provider's durable idempotency ledger.
It records the observed result under the original canonical digest domain and
never redispatches the mutation. A late dispatch owner is fenced once recovery
has claimed the intent.

CICS syncpoint uses this same rule. A durable commit or rollback intent is
written before its final decision. Replay returns the recorded final decision;
an intent without a final result remains `UnknownOutcome` across restart until
the CICS authority reconciles it. Pseudo-conversational COMMAREA continuations
and transient-data queue records use provider-state compare-and-swap identities
rather than worker memory.

Installed online CICS programs use the durable execution coordinator, not the
host-only driver. The product persists one bounded exchange record containing
the exact execution, run-unit, principal, artifact, grant, transaction,
COMMAREA, and idempotency identities before driving the machine. A process
restart reconstructs volatile CICS run state from that record and resumes the
same non-terminal execution. Previously completed effects may be replayed only
through the original provider idempotency identity and only when the returned
canonical digest matches the journaled result. `Intent` and `UnknownOutcome`
records stop before provider dispatch. When the CICS replay ledger proves an
outer result, reconciliation changes the effect to `Completed` before machine
execution resumes. Local LINK, XCTL, and RETURN resolve any explicit COMMAREA
length before this boundary, so the copied prefix—not adjacent task storage—is
the payload journaled for dispatch, replacement, or continuation. XCTL target
EIBCALEN is derived from that exact copied prefix.

Due local CICS START work uses the work row's immutable execution identifier
as the started task's coordinator identity. At expiration the worker resolves
the installed transaction and program, restores the principal captured by the
START row, and creates a facility-less CICS run before driving the ordinary
online exchange. A lease retry after target completion validates the retained
execution tuple and completes without redispatch. Task conditions and known
terminal failures belong to the asynchronously created task; unresolved
infrastructure or effect uncertainty still blocks work completion.

CICS also retains the exact bounded outer response for every mutating file,
transient-queue, program-link, enqueue/dequeue, and syncpoint request. The replay key is checked
against the canonical request digest. New replay envelopes also retain the
owning execution and conservative effect deadline; legacy envelopes remain
replayable but are not retention-eligible. A crash after the provider mutation
but before the caller observes the response therefore cannot apply that
mutation twice. An unresolved result remains an explicit HTTP 409
`unknown_outcome`; it is never translated to a normal CICS condition or the
generic `conflict` code.

Local CICS ENQ state is stored in `cics-enqueue-v1`, with one content-addressed
resource row containing the exact owner, nested UOW/TASK counts, pending grant,
and bounded FIFO waiter list. `cics-enqueue-catalog-v1` is the CAS-protected
row-count authority. A first acquisition or final release changes the resource,
catalog, and normal `cics-effect-replay-v1` receipt in one provider-state
transaction; waiter registration is committed with its suspended response.
Reopen validates every row and the catalog count before admission. Syncpoint
releases UOW ownership, while task completion, abnormal termination,
cancellation, timeout, discard, and disconnect remove both owned locks and
waiter entries. Promotion reserves the lock for the oldest waiter, which must
resume under the same execution and run-unit identity before continuing.

Installed ENQMODEL definitions are immutable rows in
`cics-enqueue-model-v1`, bound as one complete set by the
`cics-enqueue-model-catalog-v1` count/digest row. The first installation is
allowed only while the enqueue authority is empty; this makes the transition
from the single-region compatibility profile to explicit scoped identities
atomic and rollback-safe. Once installed, unmatched resources and blank-scope
matches include APPLID/SYSID in their local lock identity. A nonblank
four-character ENQSCOPE replaces that region identity and therefore coordinates
all CICS services sharing the durable store. Address-based requests never use
an ENQMODEL. Disabled matches abend ENQ, and corrupt or partially installed
model/catalog state prevents provider open. The pinned source set does not
establish precedence among overlapping generic models, so installation rejects
overlap rather than choosing an undocumented winner.

CICS interval START records use `cics-interval-start-v1`, keyed by the exact
one-to-eight-character request identifier. The version-one payload binds the
target transaction, issuing principal and run unit, expiration tick, optional
terminal and originating metadata, bounded data and FMH state, plus canonical
producer and consumer idempotency identities. Pending, protected-pending,
ready, consumed, and cancelled are explicit states. Promotion is ordered by expiration
then request identifier; consumption is one fenced CAS transition and only the
same canonical consumer request can replay it. The bounded local-data route
adds one `cics-start-v1` work row for each accepted START record. The core
workers claim that generation under the ordinary lease/epoch fence and promote
the matching record to ready; they do not yet create the target CICS task.
Typed RETRIEVE then consumes the oldest ready record for the target transaction
through explicit INTO and in/out LENGTH bindings, or through SET with an
output-only LENGTH. START admission, the work row, interval state, and replay
receipts share the durable store, so a SQLite reopen preserves the
producer-to-consumer cycle. Omitted REQID uses a deterministic internal
identifier for the same row and work ownership; ordinary START returns it in
EIBREQID, while local NOCHECK intentionally leaves EIBREQID null. Remote
routing, terminal starts, remote NOCHECK behavior, WAIT, and automatic
target-task launch remain outside this slice.
START `AFTER`/`AT` uses the same durable deadline authority as packed
INTERVAL/TIME. Append-only plan operand tags retain each explicit component and
append-only option tags retain relative versus absolute mode; literal and
storage-backed values therefore reach the provider through the same canonical
component request. The provider applies source-defined single-unit versus
combined-unit bounds and exact per-component INVREQ response2 values. Every
valid path persists one resolved expiration tick, so replay and restart never
recalculate against a later wall-clock observation.
FROM is optional on the local START schedule. A no-data record still owns the
same request/work/replay identities so a later worker can honor it, but it has
no retrievable payload. If RETRIEVE reaches that exact ready record, the consume
CAS occurs once and the canonical consumer receives ENDDATA 29/0; replay by the
same consumer remains ENDDATA. RTRANSID, RTERMID, or QUEUE make the record
metadata-bearing and therefore retrievable with actual length zero. LENGTH and
FMH cannot appear without FROM.
The bounded metadata extension
also accepts local START RTRANSID, RTERMID, and QUEUE names and returns only the
requested values through exact-width RETRIEVE outputs. A requested value absent
from the producing START returns ENVDEFERR before the record is consumed, so a
corrected canonical request can still retrieve it. Those fields use the same
row codec and producer/consumer replay fence; they add no side queue or worker.
START's FMH flag is retained in that row. RETRIEVE emits one strict typed
EIBFMH byte, and the interpreter updates its implicit field to `X'FF'` for FMH
data or `X'00'` otherwise. Historical replay responses without this additive
output preserve their prior implicit value.

For SET, the interpreter places its exact remaining task-allocation capacity in
the canonical host request. The interval authority rejects a larger record
before its consumed-state CAS. A successful response is copied into a new
bounded virtual base, and only its checked four- or eight-byte virtual address
is written to the COBOL pointer. Base bytes, pointer value, and linkage-address
state use the ordinary machine checkpoint codec, so replay from the same
pre-response checkpoint recreates the same allocation identity without exposing
a native address.

The bounded local PROTECT extension keeps its START record in
`protected-pending` and deliberately creates no shared work row. An explicit
committing SYNCPOINT first durably finalizes the UOW, then changes every
matching record from that issuing run to pending and admits deterministic work.
A finalized retry scans pending records from the same run whose work is still
absent, closing the state-transition/enqueue crash gap without duplicating work.
Explicit SYNCPOINT ROLLBACK durably finalizes the rollback before deleting
protected-pending records, so the REQID can be reused. Typed ABEND performs the
same bounded deletion before its handler transfer or terminal disposition.
Known non-command failure/disconnect/timeout and implicit task-end behavior are
covered below, including recovery after durable terminalization precedes
product/CICS cleanup.

CANCEL follows the source-defined PROTECT boundary: protected-pending rows are
not cancelable and return NOTFND, while a committing SYNCPOINT first makes the
record ordinary pending work. CANCEL after that commit uses the same durable
tombstone, work cancellation, replay identity, and worker fence as every other
local START cancellation.

REQID is optional on the typed local START route. When absent, the provider
hashes a domain separator, the mutation idempotency key, and the canonical
request digest into an eight-character uppercase identifier. That deterministic
name closes retries before the outer response journal exists, becomes the
interval row and work payload identity, and is returned as a strict EIBREQID
output. The interpreter validates and checkpoints the eight-byte implicit EIB
field. Explicit names never receive this synthetic output.

Protected START also participates in implicit task-end syncpoints. Normal
compiled completion and highest-level RETURN transition the issuing run's
protected rows to pending and idempotently admit work before volatile task
cleanup. Known execution failure deletes only that run's still-protected rows
before releasing other task state. A scheduler or data wait suspension performs
neither transition. Terminal disconnect and idle timeout delete still-protected
rows in the same caller-held cleanup pass that abandons delays and releases
enqueue state. Recovery that begins after a terminal execution outcome but
before product/CICS cleanup reads the retained exchange and machine
continuation, reconstructs the exact invocation and priority, reloads durable
CICS undo state, then follows the journaled disposition. `Completed` commits
protected rows and admits deterministic work; cancelled, timed-out, failed, or
dead-letter outcomes delete protected rows without work. Handoff completion
uses its already-applied RETURN finalization and does not repeat the syncpoint.
Cleanup removes continuation, checkpoint, and exchange state only after that
disposition-bound CICS transition has been attempted.

An optional local START USERID is part of the canonical producer request and
the duplicate-REQID fence. Before schedule persistence, the provider authorizes
the issuing principal for READ against `SURROGAT <userid>.DFHSTART`; denial is a
known NOTAUTH 70/9 result and creates neither interval nor work state. Success
stores the selected identity in the versioned interval row for future task
creation. Without USERID, the existing issuer principal remains bound. User
existence/revocation conditions and worker-driven target launch are not yet
implemented by this child.

Typed RETRIEVE WAIT uses the ordinary resumable execution boundary. A no-data
attempt returns no condition and mutates no interval row; the interpreter moves
the program counter back to the RETRIEVE statement and records a
`cics-retrieve` suspension in the durable machine checkpoint. The online
continuation keeps that exchange open. After a matching START work item is
promoted, explicit execution re-entry issues a fresh effect and consumes the
oldest eligible record under the existing CAS/replay identity. Automatic wake,
deadlock timeout, shutdown/AICB, and process-restart proof remain separate.

Typed local CANCEL requires an explicit REQID and accepts an optional local
TRANSID solely for routing authorization. It first verifies the matching shared
work identity, then CAS-transitions only a committed pending record to a
cancelled replay tombstone and requests cancellation of the queued or claimed
work. A worker that crossed the claim boundary cannot promote the cancelled
record. The identical canonical CANCEL request can finish or replay a crash-gap
cancellation; another request receives NOTFND. Protected-uncommitted, ready,
consumed, or already-cancelled-by-another-request records are not cancellable.
The tombstone intentionally defers immediate REQID reuse until the parent
START/CANCEL lifecycle owns bounded replay retention.

Typed zero-delay DELAY is deliberately stateless. Bare DELAY and a
compile-time literal `INTERVAL(0)` return NORMAL immediately and never create a
timer row, work item, checkpoint, or suspension. Positive literal or
storage-backed packed intervals and typed FOR/UNTIL unit forms use the durable
path below; they are not inferred from this immediate boundary. Remote forms
use separate reviewed boundaries.

Positive literal packed `INTERVAL` DELAY uses `cics-delay-v1` provider rows and
work generation. A hidden task/run-unit plus statement-position identity keeps
the same source command stable across checkpoint reissue. First admission
persists one pending cycle and deterministic work row; the shared worker can
promote only that due cycle under its lease/epoch fence. The interpreter keeps
the online exchange and continuation attached, reissues the command, and only
the ready-to-consumed CAS completes it. The same consumer request replays the
completion, while a later loop encounter creates a new cycle and work identity.
Provider open strictly validates retained rows, and Memory and SQLite reopen
preserve the transition. A positive literal delay may also carry a bounded
application REQID. Another task can atomically turn an unexpired pending cycle
into early expiration, cancel its queued work, and let the suspended issuer
complete with NORMAL RESP2 23; issuer cancellation and cancellation after the
expiration boundary return NOTFND. Disconnect, timeout, return, abend, and
terminal teardown move an outstanding cycle to an abandoned tombstone and
cancel queued or claimed work. Version-two rows retain strict version-one
reads. FOR/UNTIL HOURS/MINUTES/SECONDS reuse the same rows after resolving one
relative or absolute deadline from the durable and host clocks. An already
elapsed UNTIL target returns EXPIRED 31 through normal condition policy, whose
default is ignored; invalid components retain INVREQ RESP2 4/5/6. FOR
MILLISECS extends the same value with an exact millisecond remainder: pure
values admit 0–359,999,999, combined values admit 0–999, and sub-50 ms delays
return EXPIRED before state. Invalid milliseconds return RESP2 22. Automatic
redispatch, remote routing, PostgreSQL evidence, and retention eligibility
remain separate obligations. Packed TIME shares the absolute path:
the compiler preserves either an integer constant or packed numeric storage,
the interpreter emits its canonical decimal value, and the provider retains a
domain-separated identity plus the resolved deadline before suspension.
Packed INTERVAL now follows the same typed storage path under its existing tag;
literal and dynamic values are both validated at provider execution time so
INVREQ conditions and no-state failures do not diverge by source form.

After lease-fenced DELAY promotion, the product worker scans the bounded
durable online-exchange namespace and selects the unique matching run-unit. It
then resumes the saved machine checkpoint through the ordinary durable
coordinator before completing the work lease. Promotion and resume are both
retryable; if execution finalized and cleared its exchange before the work CAS,
a reclaimed worker treats the missing exchange as completed. Duplicate matches
fail closed. SQLite reopen reconstructs the installed online application,
terminal session, exchange, continuation, provider timer, and queued work before
the same worker path resumes the task. PostgreSQL restart wake remains a
separate acceptance boundary.

An ENQ wait is not a terminal-input handoff. Its execution remains
`Suspended`, and both the coordinator checkpoint and product continuation stay
attached to the same online exchange. Resume reissues the ENQ as a new bounded
effect attempt; a promoted waiter completes without incrementing the nesting
count. A crossed deadline or cancellation terminalizes the execution and runs
the task cleanup path.

Typed `CHANGE TASK PRIORITY` and `SUSPEND` use a distinct
`cics-scheduler` suspension. The machine advances past the command before
yielding, so resume cannot execute the same scheduling request twice. A valid
priority change is returned as typed control metadata and updates both the CICS
run and interpreter invocation; omission and `-1` do not yield. Product
continuation format `MEOM4` stores the resulting priority beside the machine
checkpoint and provider generations. It can also stage a program-transfer
exchange before terminalizing the artifact-bound source execution. The reader
retains `MEOM3` priority rows and `MEOM2` compatibility, using the enclosing
exchange priority when the oldest row has no explicit field. Scheduler
suspension keeps the execution and online exchange live until the next bounded
redispatch, unlike terminal-input handoff.

Typed `SET ASSOCIATION USERCORRDATA` mutates the durable session that owns the
originating task. The provider verifies the issuing and originating run-unit
identities, overwrites rather than appends, and silently truncates the supplied
bytes to 64. Current `MECS7` session rows bind the result to both the mutation
key and canonical request digest and also carry typed CICS HANDLE state.
A replay-ledger crash gap can complete only the identical association request;
a failed HANDLE-state CAS restores the prior volatile run. Readers retain
`MECS1`–`MECS6`: versions 1–4 begin with no user correlator, versions 1–5 begin
with empty HANDLE state, and version 6 preserves label-only ABEND exits. Session
CAS is the single state authority across memory, SQLite, and PostgreSQL
adapters.

An online program transfer never changes an admitted execution's artifact
identity. The product first stores an `MEOM4` start checkpoint and next-exchange
identity, terminalizes the source execution with `HandoffCompleted`, CASes the
exchange to a new execution over the same CICS run unit, and then clears the
staging marker. Recovery accepts only the exact prior or next exchange version,
finishes an observed suspended predecessor once, and verifies the terminal
handoff event before advancing. Thus a crash between any two writes cannot run
the source artifact under the target identity or strand its checkpoint.

When a terminal RECEIVE suspends a machine, the product first commits its own
session continuation and then atomically moves the interpreter execution from
`Suspended` to terminal `Completed` with `HandoffCompleted`. Only after that
handoff does it delete the redundant interpreter checkpoint and finish the
volatile COBOL/CICS run while retaining its durable HANDLE state. The next task
restores both the machine checkpoint and those specifications before consuming
terminal input. A restart in the cleanup gap recognizes the handoff event,
preserves the product continuation and HANDLE state, completes the remaining
cleanup, and admits the next terminal task under a new execution identity.
Other stale terminal exchanges discard both continuations and clear HANDLE
state before retaining their conservative `Cancelled`, `TimedOut`, or
provider-failure outcome; terminal journal rows are never passed back to
resumable execution.

Keyed dataset insert, rewrite, and delete commit the base cluster, every
upgradable alternate-index generation, and the idempotency result as one atomic
provider-state write. A restart that observes only the preceding intent queries
that provider ledger through stale-intent reconciliation; a final result
replays without applying the record twice.

Application seed generations retain verified source-object identities and a
provider-neutral dataset snapshot. Install, compatible upgrade, and rollback
atomically select one retained generation together with all dataset/index
state; corrupt or capacity-exceeding input cannot become selected.

## Scheduling

The single-node scheduler provides bounded lanes:

| Lane | Workload | Rule |
|---|---|---|
| Interactive | CICS and terminal resumes | Short bounded quantum, low latency |
| Batch | JCL, utilities, reports | Throughput-oriented, spool-backed output |
| Compiler | parse, analysis, lowering | CPU and memory quotas, content cache |
| Blocking | filesystem and legacy adapters | Isolated from async runtime workers |
| System | recovery, audit, administration | Strong authorization and reserved capacity |

Every lane has a bounded queue, concurrency semaphore, admission policy,
deadline behavior, and saturation metrics. Work is accounted until terminal
state or durable suspension, not only until an HTTP response is returned.

The z/OSMF adapter places synchronous composition calls on a dedicated bounded
OS-thread lane. Request cancellation is a live shared probe on the invocation,
not merely cancellation of the HTTP future, and the absolute gateway deadline
replaces unbounded invocation deadlines. A timed-out call keeps its worker
capacity until it cooperatively exits, while Tokio remains free to return the
timeout and serve unrelated work.

Suspended sessions own bounded serialized state but no dedicated CPU worker or
OS thread.

## Cancellation

Cancellation is a durable request with identity and reason. It is checked:

- before admission and dispatch;
- at machine quantum boundaries;
- before and after host calls;
- during blocking/remote execution where supported; and
- before checkpoint restore or child invocation.

Dropping an async future is not the cancellation contract. The driver records
the requested and observed cancellation states and releases all permits and
handles on termination.

## Store contracts

The platform owns the following store interfaces:

| Store | Authority |
|---|---|
| Execution store | lifecycle, attempts, owner lease, terminal outcome |
| Event store | ordered bounded execution and audit events |
| Checkpoint store | resumable machine and plugin snapshots |
| Session store | terminal/session metadata and resume tokens |
| Artifact store | immutable source, IR, executable, report blobs |
| Generation store | plugin generations, readiness, compatibility, draining |
| Idempotency store | effect intent/result and replay decisions |
| Work store | queued claims, heartbeats, expiry, dead letters |

In-memory implementations support deterministic tests and local development.
SQLite supports the standalone local profile. PostgreSQL plus object storage is
the first production durable profile.

## PostgreSQL durable model

PostgreSQL is authoritative for metadata and transactional state. Object
storage is authoritative for immutable large blobs.

Work claims use row state containing:

```text
work ID and execution ID
required selector and generation, plus scheduling priority
lease ID, worker ID, and monotonic fencing epoch
lease expiry and heartbeat
monotonic attempt
deadline and cancellation
checkpoint and artifact references
effect sequence
terminal/dead-letter policy
```

`FOR UPDATE SKIP LOCKED` may be used to select queue-like candidates, but the
lease columns and state machine define ownership. A database row lock is not a
durable lease and is not held while program code or user input waits.
Queued work at or beyond its deadline is moved directly to dead letter and is
never returned to a worker. Every heartbeat, release, completion, and explicit
dead-letter transition supplies the observed clock and fencing epoch; the
store's compare-and-swap transaction rejects expired, superseded, or
clock-regressed owners. Lease expiry is clamped to the work deadline.

Durable work schema 3 adds the explicit scheduling priority used by the JES
worker pool. Schema 1/2 rows remain readable with priority zero and are
rewritten as schema 3 on their next lease mutation; their existing
attempt-derived or explicit fencing epoch remains authoritative.

State transition, current projection, effect record, and outbox notification
are committed in one transaction where they share an authority boundary.

## Event model

Execution events are append-only and ordered per execution. A materialized
current-state row enables efficient reads. The event log is not used as a
religious full-event-sourcing framework: subsystem business data remains in its
owned provider store.

Notifications are delivered through a transactional outbox and bounded local
wakeups. Loss or duplication of an in-process wakeup cannot change authoritative
execution state because workers rescan durable state.

## Checkpoints

A checkpoint envelope includes:

```text
checkpoint schema version
execution/run-unit/session identity
machine or plugin state schema
artifact identity
plugin generation
required host interface versions
effect sequence and transaction metadata
principal/security classification
size and integrity digest
encryption/key reference where required
```

Restore validates all compatibility requirements before state is made active.
Readers support only documented version ranges. Writers emit the current
version. Incompatible state is rejected or passed through an explicit upgrade
function; it is never silently coerced.

## Consistency and retry

- Dispatch is at least once.
- A work item has at most one valid, unexpired owner lease according to the
  durable state machine.
- A stale fencing epoch cannot heartbeat, release, complete, or dead-letter a
  later claim, even when the same worker name is reused.
- Lease expiry does not prove a side effect did not occur.
- Mutating host services provide idempotency, a transaction, or an explicit
  non-retryable/unknown-outcome policy.
- Read operations declare snapshot/consistency requirements.
- Exactly-once product claims are prohibited.

## Distributed evolution

Horizontal execution changes store and queue adapters, not compiler, machine,
plugin, effect, or outcome semantics. Multi-node promotion requires model and
chaos evidence for lease expiry, partition, stale owner, duplicate delivery,
generation draining, checkpoint transfer, and reconciliation.
