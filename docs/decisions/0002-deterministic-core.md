# ADR-0002: Deterministic Core and Asynchronous Shell

Status: **Accepted by repository owner**
Decision scope: **Compiler, interpreter, execution, and host effects**

## Context

COBOL/CICS/JCL execution includes program frames, conditions, ABEND, transfer,
terminal waits, dataset effects, job steps, cancellation, and resumable state.
Representing this as a long async call stack makes checkpointing, replay,
durability, and failure recovery dependent on runtime implementation details.

## Decision

Compiler transformations and executable machine transitions are synchronous,
deterministic operations over explicit immutable input and owned state.

The execution engine drives the machine in bounded quanta. A drive returns one
explicit action:

```text
Continue
HostCall
Invoke child
Transfer
Suspend
Complete
Fail
```

The asynchronous shell is responsible for:

- admission and quotas;
- lane scheduling and backpressure;
- cancellation and deadlines;
- persistence and leases;
- provider I/O;
- HTTP and CLI integration; and
- tracing, metrics, and audit export.

The machine never awaits a provider. It emits a typed effect request and later
accepts a typed result or failure as resume input.

## Required properties

- Same state and input produce the same transition and observations.
- Clock, random, terminal input, dataset results, identity, and configuration
  enter through explicit inputs/effects.
- Iteration and serialization order are deterministic.
- Each quantum checks step, allocation, output, recursion/frame, and effect
  limits.
- Machine state is serializable at declared safe points.
- Normal control flow is not encoded as `Error`.
- A provider panic or async cancellation cannot leave an unrecorded successful
  mutation.

## Effect ordering

The reference model emits one ordered host effect at a time. Each mutation has
an execution identity, monotonic sequence, idempotency identity, and persisted
intent/result lifecycle.

Parallel effect execution is a later optimization and must prove equivalence to
the reference ordering and failure model.

## Consequences

- Unit and property tests can drive the machine without Tokio or a database.
- Checkpoints contain domain state, not Rust futures or stack frames.
- SQLite, PostgreSQL, in-memory stores, and future distributed adapters share
  the same semantics.
- The interpreter contains more explicit state than a recursive implementation,
  but the state is reviewable, versionable, and recoverable.

## Alternatives rejected

- Persisting or reconstructing arbitrary async call stacks.
- Actor framework as the semantic model.
- Using exceptions/errors for CICS transfer or suspension.
- Temporal/Restate workflow definitions as the execution contract.
- One OS thread or permanently running task per idle session.
