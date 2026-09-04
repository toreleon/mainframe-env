# JES execution, scheduling, spool, and utilities

Status: **Normative from mainframe-env 0.8.0**

## Authority boundary

`mainframe-env-batch` owns deterministic JES-observable state transitions. It
does not own a second work queue, dataset catalog, lock table, security policy,
effect journal, checkpoint store, artifact store, or program catalog. Durable
job projections use `ProviderStateStore/jes-job`; asynchronous claims continue
through the common `WorkStore`; dataset DD effects use the 0.6 dataset
authority; and every admission, selection, execution, output, and control route
uses the 0.5 SAF authority.

The public contracts are:

- `mainframe-env.jes-runtime@1` for class, initiator, queue, lifecycle, control,
  condition, and cancellation values;
- `mainframe-env.jes-durable-job@2` for the bounded durable job projection;
- `mainframe-env.jes-checkpoint@1` for restart-safe step/effect progress;
- `mainframe-env.jes-spool@1` and `mainframe-env.jes-output@1` for spool files
  and output groups; and
- `mainframe-env.jes-utility-registry@1` for typed utility registrations.

## Lifecycle and scheduling

The projected lifecycle is:

```text
submitted -> held <-> queued -> selected -> running -> output -> completed
     |          |         |         |          |          |
     +----------+---------+---------+----------+----------+-> cancelled
                                      +---------------------> failed
```

No state may jump directly from queued to running or completed. Selection is a
durable transition naming the initiator. Eligible work is filtered by enabled
initiator, configured classes, initiator capacity, per-class capacity, and
priority range; it is then selected by descending priority and ascending JES
job number. A warm start returns a selected or running job to the execution
queue unless its bounded attempt limit is exhausted. A durable output-phase
job completes without re-executing program or dataset effects.

Each step records `pending`, `allocating`, `running`, `disposing`, and one exact
terminal state. Bypassed-restart and skipped-condition states are explicit.
Completed steps retain their return codes across retry. An abended step retains
its code and permits only applicable `EVEN` or `ONLY` cleanup steps before the
job publishes the original abend. Return codes outside 0 through 4095 fail
closed rather than becoming successful output.

## DD, effect, and restart rules

DD resolution is a typed phase over the immutable 0.7 plan. Allocation,
catalog, GDG, member, record, lock, and lifecycle effects are requests to the
0.6 dataset authority. Mutations use stable job/step/effect identities. JES
checkpoints retain the completed-step set, monotonic effect sequence, temporary
and GDG resolution map, cancellation state, and an integrity digest. A retry
must replay recorded provider results or surface `unknown-outcome`; it may not
assume a lost acknowledgement means that no effect occurred.

Normal and abnormal DISP are evaluated only after the typed program outcome is
known. Concatenation remains ordered. Temporary datasets are scoped to the job
identity and are removed at their specified terminal disposition or bounded job
cleanup. Cross-resource outcomes remain explicit under the common UOW contract;
0.8 does not claim the 0.16 mixed-provider matrix.

## Spool and output

Spool files and output groups have stable identities, exact job/step/DD
ownership, class, destination, writer/forms selection, counts, bounded
retention, and an explicit held/released/selected/complete/cancelled/purged
state. Large payloads use the common artifact/object-store boundary while SQL
metadata remains the durable authority. Output access and every state mutation
are SAF checked. Purge removes metadata and referenced payloads only after the
terminal transition succeeds; an indeterminate delete remains visible for
reconciliation.

NJE and MAS are bounded routing and ownership abstractions over these same job,
work, and spool authorities. They are not a claim of physical JES2 sysplex,
SNA, printer, punch, or byte-for-byte spool implementation parity.

## Typed program and utility routing

The common program catalog resolves an external program name once into a typed
registration. JES dispatches only that registration: registered utility,
registered subsystem controller, or ordinary `ProgramService`. The execution
loop does not branch on a raw program name. Unknown ordinary programs reach
`ProgramService`; cataloged unavailable routes fail explicitly.

The required utility identities are IEFBR14, IEBGENER, IEBCOPY, IEBCOMPR,
IEBDG, IEBEDIT, IEBUPDTE, IDCAMS, and SORT. Their typed families are allocation,
copy, compare, generation, edit, update, catalog, sort, and diagnostic. A
utility may emit an explicit condition or unavailable capability, but it may
not return a DD-count or command-name summary as semantic success.

## Evolution and recovery

Writers emit durable job version 2. Version 1 remains readable and migrates by
CAS, preserving all legacy fields and deriving pending step executions from the
immutable plan without running effects. A failed or conflicting migration
publishes no partial state. The finite reader range and rollback projection are
recorded in
`conformance/0.8/migrations/jes-durable-job-v1-to-v2.json`.

Licensed z/OS 3.2/JES2 observations remain a distinct oracle authority. Local
models, CardDemo, generated catalogs, or historical transcripts cannot produce
`differential=pass`.
