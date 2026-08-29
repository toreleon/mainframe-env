# Execution and Durability Architecture

Status: **Accepted by repository owner**

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

For a mutating effect:

1. Validate request, capability, principal, bounds, transaction, and deadline.
2. Allocate a monotonic effect sequence in the run unit.
3. Persist the effect intent and idempotency key.
4. Invoke the provider.
5. Persist success, condition, failure, or unknown outcome.
6. Feed the typed result into the machine.
7. Persist the next machine state or terminal outcome.

Infrastructure retry never assumes that an external mutation did not occur.
Unknown outcomes remain explicit and require service-specific reconciliation.

CICS syncpoint uses this same rule. A durable commit or rollback intent is
written before its final decision. Replay returns the recorded final decision;
an intent without a final result remains `UnknownOutcome` across restart until
the CICS authority reconciles it. Pseudo-conversational COMMAREA continuations
and transient-data queue records use provider-state compare-and-swap identities
rather than worker memory.

Keyed dataset insert, rewrite, and delete commit the base cluster, every
upgradable alternate-index generation, and the idempotency result as one atomic
provider-state write. A restart that observes only the preceding intent can
safely retry; a final result replays without applying the record twice.

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
required selector and generation
lease ID and worker ID
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
