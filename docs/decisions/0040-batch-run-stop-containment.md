# ADR-0040: Batch run stop containment

Status: Accepted bounded opt-in prerequisite; enclosing host activation pending.
Owner: **Batch maintainers**
Scope: **MQ-1503.batch-run-stop-containment, target 0.15.0**
Applies from: **mainframe-env 0.15.0 development**

## Decision

`run_claimed_with_run_observer` shares the existing scheduler, selected/Running
CAS, checkpoint, registration, Program request builder and retirement body.
Its first profile requires the same physical checkpoint/state adapter, a fresh
queued job and ProgramService registrations. It refuses recovered attempts and
legacy embedded spool migration. It creates no independent scheduler or journal.
The old `run_claimed` and Program-only transport remain their historical paths;
the new containment contract must be selected explicitly by an enclosing host.
Contained internal-reader submission reserves the existing job number, releases
Batch state for spool/provider calls, then rechecks replay/row absence and finite
submission quotas before the existing publication. Failures stop without purging
known history. The legacy path retains its original lock and cleanup behavior.

The private run owner is minted only after the actual known Running Program row,
checkpoint and registration checks. Higher-ranked `RunningStepAdmission` borrows
that genuine site; retained step views share its irrevocable run revocation and
individual step revocation. Neither equal rows, public IDs nor recovered state
can construct an owner. Views never extend dispatch lifetime.

One explicit private run context threads through the existing host helper graph,
including the four DD attributes/DCB/access-path methods. A plain Invocation is
a no-op context preserving old builders, keys, schemas, cadence and behavior.
There is no TLS, service-global permit, second engine or context cache.

The opt-in captures the actual Job under its mutex, releases it for authorization
and observation, then reacquires and checks owner/state/version and the physical
CAS dependency before selected/Running writes. Observer and provider callbacks
are outside Batch mutexes. Observers must be bounded, nonblocking and nonreentrant;
panic, reentry and invalid/nonmonotonic observations stop the run. The observer's
tick and live cancellation probe do not prove decision-time physical Work expiry.

Unknown, callback panic, wrong sequence, cancellation, timeout and control failure
irrevocably stop subsequent effects, DD disposition, temporary cleanup, output and
terminal/checkpoint publication. A known effect/audit or Job edge preceding a late
stop stays recorded. Nothing rewinds, retries or chooses opposite cleanup. Unknown
is not replaced with a known success or later error. Known modeled ABEND/condition
continues through the existing disposition/retirement semantics if controls allow.

The contained path also fences `Malformed` replies from the shared host boundary
before its next control callback or action. Shared validation normalizes outward
sequence, so it cannot expose whether a malformed read-only reply had the wrong
raw sequence or another invalid shape. This finite path conservatively stops
known malformed replies too, after preserving the actual shared failure audit.
It is invalid-transport containment, not an IBM reason or durable UOW decision.
Plain Invocation and the older Program-only route retain known-error disposition.

The owner revokes before retirement or cleanup, on error and on unwind. Drop only
revokes; it never dispatches or settles durable state. `BatchRunExit` is non-Clone,
non-Serde and has no public constructor. It reports the exact original Invocation,
same adapter, first known Running row, last privately published physical row and
actual retired or latched stopped disposition. A competing physical row cannot
mint a matching exit. These are observations, not a terminal publication permit.

The original PRE-binding Invocation and the actual post-`jes.work-id` Program
Invocation remain distinct exact observations. Parent, grants, limits, generation,
probe, deadline, sequence, key, payload and canonical identities are unchanged.

## Remaining prerequisites and evidence limits

This boundary does not activate Program Any transport, a controller entrypoint,
coordinator-owned original Intent, JesClaimRun, scheduled root terminal policy,
physical Work/Job decision-time clock, recovery settlement or DefaultContext.
The future privileged closed host driver must independently establish those
owners. Matching Invocation/binding/Job bytes or positive return codes cannot.

Actual Batch Memory and owned SQLite fixtures exercise this transport/containment
boundary. Fixture SAF and Program callbacks are not compiled installed, native,
JES, core-original, participant, full26 or licensed acceptance. SQLite orderly
reopen is not a process crash. All nonlicensed parent obligations remain required;
only the licensed oracle is human-skipped 0/26.
