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
  and output groups;
- `mainframe-env.spool-request@2`, `mainframe-env.spool-result@2`, and
  `mainframe-env.spool-state@1` for the typed host-provider and its durable
  authority; and
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

`mainframe-env.jes-dd-allocation@1` classifies every accepted DD as dataset,
inline data, DUMMY, or SYSOUT before an effect is issued. Dataset status and
normal/abnormal dispositions are position-validated and defaulted explicitly;
invalid source combinations, output concatenations, temporary cataloging, and
abnormal PASS fail closed. OLD and MOD obtain exclusive dataset-wide ownership,
SHR obtains shared ownership, and duplicate references are folded into the most
restrictive single lock. The lock is owned by the submitting principal and
scoped by job identity in the shared dataset authority, so two jobs owned by the
same principal still conflict correctly.

New non-GDG datasets begin in the dataset authority's allocated state. CATLG
publishes the cataloged lifecycle state and UNCATLG returns it to allocated;
catalog listing omits allocated entries while direct allocated-name access
remains possible. MOD writes use the typed append request. Compatible
concatenations use the authority's bounded ordered concatenation request, while
mixed inline/member groups are read in declared order with the same aggregate
bound. PASS retains a job-scoped temporary allocation between steps, and a
terminal cleanup pass removes any remaining temporary datasets. A partial
allocation failure applies abnormal disposition to completed allocations and
releases every acquired lock before it is surfaced.

## Spool and output

Spool files and output groups have stable identities, exact job/step/DD
ownership, class, destination, writer/forms selection, counts, bounded
retention, and an explicit held/released/selected/complete/cancelled/purged
state. Large payloads use the common artifact/object-store boundary while SQL
metadata remains the durable authority. Output access and every state mutation
are SAF checked. Purge removes metadata and referenced payloads only after the
terminal transition succeeds; an indeterminate delete remains visible for
reconciliation.

The spool provider keeps bounded metadata and replay receipts in
`ProviderStateStore/jes-spool`. Each append creates an immutable
`mainframe-env.spool-chunk@1` artifact containing the exact record bytes and
then CAS-publishes its reference. A failed metadata publication deletes the
unpublished artifact; if compensation cannot be established it returns
`unknown-outcome`. Reads validate the artifact identity, media type, digest,
job, file, sequence, record count, and byte count before returning data.

At job terminal transition every file is sealed. Non-held output enters
awaiting-selection; held output remains held. Authorized controls provide the
only held-to-released, released-to-selected, selected-to-complete, reroute, and
purge transitions, and update the job projection by CAS. The SAF resource is
the active `JESJOBS` class with `JOB.<job-name>.<stable-spool-or-output-id>` so
authorization happens before artifact access or mutation. The product grants
only the matching typed `host.spool.read` or `host.spool.write` capability to
each request.

Purge first writes a durable `purge_pending` intent, then deletes all referenced
artifacts, and only then publishes the purged tombstone and removes terminal
job metadata. An artifact-delete failure leaves the intent visible and retryable
instead of claiming success. Retention cleanup uses the same authorized purge
path. Version-2 jobs containing legacy embedded records retain those bytes until
an authorized, idempotent provider migration completes and the job CAS clears
the embedded projection.

NJE and MAS are bounded routing and ownership abstractions over these same job,
work, and spool authorities. They are not a claim of physical JES2 sysplex,
SNA, printer, punch, or byte-for-byte spool implementation parity.

Every job records its submission kind and origin. External jobs retain an
external origin, internal-reader jobs retain the parent job and producing step,
and started tasks retain the authorized started-task name. Internal-reader
records are reparsed and admitted through the normal converter and durable job
authority; denied admission creates no child job. Started-task start and stop
use the `STARTED` SAF class, while subsequent selection and job/output controls
remain protected by the job resource.

`mainframe-env.jes-topology@1` is the bounded NJE/MAS projection. Nodes expose
connected/enabled state and inbound capacity. MAS members name exactly one node
and expose enabled/active capacity. A queued or held job may be routed only to
available execution and output nodes; selection is then restricted to an
eligible member on the execution node. The selected member is recorded in the
same CAS-protected job projection as the lifecycle transition. It is an
operator-visible semantic owner, not a replacement for common `WorkStore`
claims, leases, retries, or cancellation.

Scheduler enablement and topology configuration have versioned single-writer
records in `ProviderStateStore/jes-scheduler` and
`ProviderStateStore/jes-topology`. `OPERCMDS` protects their start, stop, and
install controls. `JESJOBS` protects hold, release, cancel, class/priority
change, job routing, selection, output controls, access, and purge. Every
control validates its source state and all capacity/routing invariants before
publishing a CAS transition.

## Typed program and utility routing

The common program catalog resolves an external program name once into a typed
registration during job admission and retains it in the durable job projection.
JES dispatches only that registration: registered utility,
registered subsystem controller, or ordinary `ProgramService`. The execution
loop validates the step-to-registration binding and does not resolve or branch
on a raw program name. Unknown ordinary programs reach `ProgramService`;
cataloged unavailable routes fail explicitly. A missing or substituted durable
registration fails before any step effect.

The required utility identities are IEFBR14, IEBGENER, IEBCOPY, IEBCOMPR,
IEBDG, IEBEDIT, IEBUPDTE, IDCAMS, and SORT. Their typed families are allocation,
copy, compare, generation, edit, update, catalog, sort, and diagnostic. A
utility may emit an explicit condition or unavailable capability, but it may
not return a DD-count or command-name summary as semantic success.

IEFBR14 applies only typed allocation/DISP effects. IEBGENER and IEBCOPY copy
exact records from their selected input DD to output DD; IEBCOMPR returns CC 0
or 8 from byte-exact comparison. IEBDG implements a bounded deterministic
DSD/FD/CREATE record generator. IEBEDIT selects a bounded job or positional
range and optional step into SYSUT2. IEBUPDTE applies one explicit ADD/REPL
member body to the allocated SYSUT2 member. IDCAMS remains the typed
dataset/catalog command family, and SORT performs bounded record ordering and
OUTREC projection. Unsupported controls fail explicitly before output
mutation; diagnostic record counts are not used as proof of semantic success.

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
