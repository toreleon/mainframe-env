# ADR-0027: CICS task ownership across logical program frames

Status: **Proposed for CIC-902.program-task.frames; acceptance pending**
Owner: **CICS provider and selected execution maintainers**
Scope: **local LINK and INVOKE APPLICATION on an existing task**
Applies from: **mainframe-env 0.9.0 development**

## Context

A CICS command removes its Run from the live map while invoking a host. A
selected program executes synchronously on the same run unit, and may issue
CICS commands. Registering another Run in that interval creates a second task
snapshot. Returning the held caller overwrites child file/UOW state. With an
explicit session binding, registration also reloads the caller's persisted
HANDLE stack into the child. A compiled caller PUSH/child POP regression
reproduces NORMAL instead of required INVREQ 16 at the child's logical level.

## Decision

The existing CICS host-boundary owner supplies exclusive command and program
frame leases. A task has one live Run, stable task invocation and shared file,
undo, browse and UOW state. Its current program frame separately carries the
effect invocation. Child effects and host audits use that frame invocation;
existing task-owned BTS, enqueue and UOW rows retain the task execution owner.
No second security evaluator, coordinator, task store or universal frame IR is
introduced. Move dispatch bookkeeping out of the protected service facade.

A synchronous program loan admits only the requested program and selected
artifact, same run unit and principal/grants/generations, the expected parent
execution, and a deadline/resource envelope no wider than its caller. Reentry
is confined to the current synchronous executor thread and bounded by the
invocation's frame limit and a fixed provider ceiling. Unrelated concurrent
entry cannot create another Run. Asynchronous execution would require an owned
explicit admission token rather than widening this thread-confined contract.

HANDLE/IGNORE/AID/ABEND specifications and PUSH/POP stacks are frame-local.
Child changes do not replace the durable root session's HANDLE state. Caller
specifications, current program and outer command context are restored on
normal and failed host return; shared file/UOW changes are retained. A lower
logical-level RETURN completes that frame without task-end resource cleanup.
Explicit child ABEND searches active exits at preceding logical levels through
the same program loans, not ordinary handler inheritance. The current-level exit
wins; suspended PUSH HANDLE entries are not active. Entry deactivates the selected
exit, retaining it for RESET. CANCEL bypasses and clears all traversed levels.
Task-wide virtual addresses are not supplied
by a Run lease and remain a separate unresolved storage boundary.

## Recovery and compatibility

The current selected child executor is synchronous and not independently
resumable. In-flight frame leases are volatile, not a new durable workflow.
Root HANDLE persistence and existing installed-call reservations, private CICS
replay, core result digests, UOW and checkpoint authorities remain durable.
An unresolved installed call stays an unknown outcome requiring fenced
reconciliation; it must never restart a child merely to reconstruct a frame.
Completed outer replay returns its retained result without reentering the child.

Nested program identities must bind the original outer effect and frame actor,
with deterministic per-command sequencing. Before integration, compatibility
tests must prove that legacy pending installed calls cannot escape their fence
through a changed nested key. Any required owned protocol reader change must
declare bounds, retention and rollback; no destructive migration is authorized.
This ADR does not claim that those pending acceptance gates already pass.

Terminal lifecycle cleanup also owns an exclusive volatile session lease while
releasing task resources outside the state mutex. Command/frame admission and
session mutation cannot interleave with that cleanup. Cleanup failure releases
the lease; it does not create a durable completion claim or erase the task.

The compiled child SYNCPOINT probe revealed a second durable identity boundary:
the existing UOW retention metadata uses one execution identity for both core
effect provenance and root task/BTS settlement. Those identities differ in a
linked frame. The focused repair represents both without reassigning existing
root acquisitions. Local codec/read compatibility, two-owner retention, BTS
reconciliation, SQLite/PostgreSQL reopen and compiled child commit/rollback
proofs are recorded in the status document. They do not close the remaining
unknown-call, general/default-condition ancestor ABEND or selected failure-path
acceptance obligations.

The repair adds `MECU3` only when a SYNCPOINT effect actor differs from
its root task owner. Existing `MECU1` and `MECU2` remain readable and root writes
remain byte-compatible `MECU2`. V3 retains effect owner/run/deadline/terminal
metadata and adds one bounded, distinct root task execution. BTS reconciliation
uses the root identity; core provenance uses the effect identity. Retention
protects both executions and atomically rechecks the root owner for nested
enterprise replay as well as for the UOW row itself. No row is rewritten merely
on read, no missing identity is inferred and no unresolved effect is redispatched.
An old reader rejects V3. Downgrade therefore requires drained writers and a
verified backup or a compatible reader retained through the rollback; it must
not strip the root owner or relabel a V3 row as V2.

Explicit ABEND unwinding uses a volatile marker only after the provider observes
the child command. Only the matching known `INSTALLED-CALL-ABEND` executor result
may select an ancestor exit; an uncertain or mismatched reply returns
UnknownOutcome and fences further commands. Root exit deactivation and task-wide
latest/original ABEND metadata use the existing session authority. A child-local
exit returning normally also persists that metadata when the root is restored.
Handled ABEND retains task resources; still-protected START requests are always
discarded, as required by their separate pre-syncpoint cancellation contract.

Outer LINK/INVOKE replies add bounded `ABEND.CODE` with schema
`mainframe-env.cics.abend-code@1` and the existing `ABEND.DUMP` decision. Replay
validates their paired control disposition, target and payload without redispatch.
Unhandled replies preserve the original code/dump in interpreter outcomes.
Historical replies without the new output keep their existing interpretation.
No new persisted protocol generation or in-flight restart is introduced. Known
child ABEND does not fabricate a completed installed-call reservation; that
reservation remains protected by the existing recovery/retention fence. Older
interpreters do not honor this additive code on LINK, so downgrade requires
draining affected writers and retaining a compatible reader or restoring a
verified pre-change backup, not silently rewriting replies. General/default
condition ABEND and ancestor PROGRAM exit execution remain unaccepted.

## Source and acceptance

Pinned authority: CICS TS 6.x sources B baseline
`ibm-cics-ts-6x-application-api-sources-b-2026-09-10`, application catalog rows
0138 LINK, 0106 INVOKE APPLICATION, 0099 HANDLE CONDITION, 0100 IGNORE
CONDITION, 0146 POP HANDLE and 0149 PUSH HANDLE. The status document records
verified topic hashes. RETURN row 0178 uses sources C baseline
`ibm-cics-ts-6x-application-api-sources-c-2026-09-10`, topic
`SSJL4D_6.x/reference-applications/commands-api/dfhp4_return.html`, SHA-256
`71396046a1caeab92a27bc0debfac79b2bb5a0d5567db69887c8e5ad1e0b84c4`.
ASSIGN row 0011 uses sources A baseline
`ibm-cics-ts-6x-application-api-sources-a-2026-09-10`, topic
`SSJL4D_6.x/reference-applications/commands-api/dfhp4_assign.html`, SHA-256
`594c0885848525ccc9c0c1f939a449f0d759f90ada7d29a82ed580342a99bbee`.
Shared file/UOW regression consumers are READ row 0156 (sources B), REWRITE
row 0181 and SYNCPOINT row 0218 (sources C); the status document records their
verified topic hashes and distinguishes local proofs from licensed evidence.
Explicit ABEND uses sources A row 0001 `dfhp4_abend.html` and sources B row
0097 `dfhp4_handleabend.html`, plus `applications/designing/dfhp378.html`.
START row 0205 uses sources C `dfhp4_start.html` for PROTECT cancellation.
The status document records the hash-verified offline reads and selected proofs.

Require compiled handler isolation/caller restoration, shared file update and
rollback, lower RETURN, identity/depth/concurrency fences, deny/cancel/failure
and unknown-outcome recovery, SQLite/PostgreSQL reopen and mandatory gates.
This is neither full program-family closure nor licensed differential evidence.
