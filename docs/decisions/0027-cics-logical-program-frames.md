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

Nested program identity now binds the original durable outer command, root run
unit, current frame actor and a per-command program occurrence. SHA-256 consumes
`mainframe-env.cics-program-occurrence@2` plus a zero byte, then the root run
unit, frame actor execution and outer command key UTF-8 fields in that order,
each prefixed by its u64 big-endian byte length,
then the u64 big-endian occurrence. The key is `cics-program-v2:` followed by
64 lowercase hexadecimal digits. Occurrences start at one, reset only when an
outer command actually dispatches, and cannot exceed the actor's `max_effects`.
Restoring a caller frame restores its occurrence counter. The volatile global
host counter and request inputs cannot select another program key; the existing
installed-call fingerprint independently binds inputs and selected generation.
Non-program nested effect keys are unchanged and remain a separate acceptance
obligation. LINK, INVOKE APPLICATION and existing BTS, bridge and web-service
program dispatch share this owned boundary, with no new coordinator or ledger.

Fresh installed-call admission writes JSON protocol schema 3 in the existing
`cobol-call-protocol@2` namespace, including ordinary COBOL admission which may
precede a CICS LINK. Protocol metadata uses the existing framed digest algorithm
with field domain `protocol-metadata@3`, preventing schema-only relabeling to
the old digest domain. Row CAS versions remain 1 active / 2 terminal; owner,
run-state, deadline and terminal retention dependencies are unchanged. Readers
accept valid schema 2 and 3, but a new program key requires schema 3. Legacy
protocol markers and active schema-2 runs therefore require drain/reconciliation
before using the new domain. Cached old replies remain readable, and no row is
rewritten merely on read. Replaying a retained new-key receipt requires an
already present valid schema-3 protocol; it cannot recreate a missing protocol.
Malformed, foreign, unknown-version and over-bound identities fail closed.
Old readers reject schema 3. Downgrade requires drained writers and a verified
pre-change backup, or a compatible reader retained through rollback; never strip
the schema or relabel retained metadata. The status records focused warm/cold
SQLite/PostgreSQL uncertainty proofs, not full program-family acceptance.

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
The ABEND repair itself introduces no new protocol generation or in-flight restart. Known
child ABEND does not fabricate a completed installed-call reservation; that
reservation remains protected by the existing recovery/retention fence. Older
interpreters do not honor this additive code on LINK, so downgrade requires
draining affected writers and retaining a compatible reader or restoring a
verified pre-change backup, not silently rewriting replies. General/default
condition ABEND and ancestor PROGRAM exit execution remain unaccepted, except
for the bounded unmatched POP HANDLE recovery below.

### Source-defined unmatched POP HANDLE default recovery

Sources-B POP HANDLE row 0146 defines INVREQ/default abnormal termination when
no PUSH exists at the current link level. With no current RESP/NOHANDLE,
IGNORE or condition handler, propagate through the existing program loans to
the nearest active ABEND exit; parent LINK RESP cannot turn the child abend
into an ordinary return. A current-level exit deactivates normally. Cancel
still-protected START requests even when recovery retains task resources.

The volatile pending response and outer LINK reply carry `ABEND.DEFAULT`,
schema `mainframe-env.cics.default-abend@1`, exact bytes `POP-HANDLE`. Validate
INVREQ 16/0, disposition/target/payload and origin before selecting an exit or
replaying. Only the matching known child executor Abend result can unwind.
Outer replies additionally carry empty `ABEND.CODE` with the existing schema;
this means no attested IBM code, not a guessed ABCODE. No dump decision is
invented, and prior explicit ABEND metadata is not reused. This is not a generic
default-condition or machine-check recovery table.

No durable namespace, protocol generation, dependency graph or completed CALL
reservation is introduced. Pending calls and exact known-Abend instance proof
remain fenced/protected. Old readers reject these marked outer replies because
they do not satisfy the explicit ERROR/code/dump contract. Downgrade requires
drained writers and a compatible reader or verified pre-change backup; never
strip the origin marker, invent a code or relabel pending calls as completed.

### Unresolved installed child control

Installed child Transfer, Invoke and Suspended outcomes are not terminal proof.
Until an owned installed continuation/replacement protocol can finish them, the
executor returns UnknownOutcome, not a negative condition consumable by LINK
RESP or COBOL exception handling. The existing host-boundary dispatch records
a volatile uncertain-session fence after a linked unknown result; subsequent
commands, warm restoration and task cleanup reject without releasing resources.
This fence is bounded by held tasks, does not affect unrelated sessions and is
not a new durable ledger or a recovery mechanism. Cold restore must be attested
by the embedding against the existing core/CALL authority; replay of an unresolved
reservation remains unknown and reinstates the live fence. No busy instance,
suspended checkpoint or pending CALL is turned into completed handoff proof.
No persisted schema changes. Previously recorded definitive negative-condition
results are not rewritten as new evidence; affected active states require drain
and owned reconciliation, never automatic redispatch. Non-root PROGRAM/XCTL
replacement and child suspension resume remain implementation obligations.

### Installed transfer intent: first replacement-protocol phase

The executor now captures an observed same-level Transfer in its original
pending CALL reservation after verifying the exact Suspended child, journal
event and retained checkpoint against the current machine. JSON receipt schema
3 is pending-only, at row CAS version 2, with no reply/completion tick. It adds
one strict bounded intent: observed target selector, payload schema/bytes,
source selector/artifact/attempt/suspended version and checkpoint digest,
effect sequence, store machine-schema metadata and actual payload schema.
The receipt metadata domain changes from
`mainframe-env.cobol-call-receipt-metadata@2` to `@3`; it also includes the
intent digest using the existing framed `installed-call@1` algorithm with first
field `installed-transfer-intent@1`. All intent fields, in serialized order,
are framed UTF-8/byte fields; integers use big-endian u32/u64 bytes. The intent
digest's 64 lowercase hexadecimal bytes are appended to receipt metadata.

The existing CALL namespace, protocol generation and retention dependency graph
are unchanged. Schema 1/2 cached/pending receipts remain readable; ordinary
writers remain byte-compatible schema 2, with the absent optional field omitted.
Schema 3 can only represent an online-call pending intent, never a completed
reply. Strict phase/bounds/digest validation protects it as Active and retains
the source/caller/run/protocol/instance authority. There is no target execution
or artifact-retention claim yet: this captures the observed target name, not
an immutable resource selection. Checkpoint record metadata machine schema 1
is distinct from its reference-machine payload schema 12; neither is relabeled.

The writer advances only the original CALL CAS. Core source state is not in
that provider-row transaction and is not advanced. Any future consumer must
revalidate source proof and resolve the immutable target through the existing
CICS resource/security owner, attest the recorded target/payload against the
retained source command/result (an integrity digest is not execution authority),
stage its exact initial checkpoint, acquire owned
same-level admission/instance disposition, then complete handoff and execute.
Neither failed capture nor cold pending replay may infer a normal return or
automatically redispatch. Source checkpoint and busy instance remain intact.
A secondary cursor-write failure before capture also preserves pending
Transfer/Invoke/Suspended as UnknownOutcome, not a known error consumable by
the caller. A crash before intent publication leaves a protected unstaged
reservation; this phase does not close that recovery window or authorize
reconstruction by redispatch.
Old pending receipts cannot reconstruct a lost Transfer from the checkpoint
alone and are not rewritten on read. Old readers reject schema 3; downgrade
requires drained writers and a compatible reader or verified pre-change backup,
never stripping the intent or relabeling it as a completed/schema-2 receipt.

### Known-ABEND installed-instance disposition

After the existing durable coordinator returns `ExecutionOutcome::Abend`, the
installed executor validates the exact child's Failed execution and terminal
Abend event, including execution/run/principal, selected program/artifact,
attempt, event/version and terminal tick. It then atomically decrements only
that run's active count and marks the leased instance inactive under both run
and instance CAS versions. Generic Failed, cancellation, rejection, missing
proof and unknown outcomes cannot enter this transition. A disposition failure
returns UnknownOutcome, not a handleable successful return.

Only this disposition writes instance JSON schema 2 in the existing
`cobol-instance@1:<run-key>` namespace. Schema 2 is a non-reusable abandoned
frame, not a normal last-used image: busy is false, saved state is absent, and
the current open-file flag remains explicit. Its bounded proof binds root owner,
child execution, terminal version/attempt, namespace, program, artifact and
open-file flag with a domain-separated metadata digest. Its retention descriptor stays
Active and protects both the exact root and child executions. The installed-call
reservation is unchanged and pending; no completed CALL reply is fabricated.
The existing installed-call digest framing hashes `mainframe-env.installed-call@1`
plus a zero byte, then u64 big-endian length-prefixed fields in this order:
`known-abend-instance@2`, namespace, program, artifact, root owner, child execution,
terminal version (u64 big-endian), attempt (u32 big-endian) and open-file flag (one byte,
zero or one). The golden vector uses namespace suffix 64 zero digits, program
LEAF, artifact `sha256:` plus 64 `a` digits, root `root-execution`, child
`child-execution`, version 9, attempt 1 and closed files; its digest is
`14a03ab133ac156ecff00c2d6efd3e594d0672f059ca439e27b240cd1b7c2a27`.

CALL and CANCEL cannot reset or reuse an abandoned instance. Owned run-unit
completion may atomically remove it with the other instances only after all
active counts and open-file obligations are clear and its exact durable Abend
proof is revalidated. A surviving active or uncertain instance, foreign owner
or stale CAS keeps the run fenced. This does not implicitly close files or
back out handled CICS work. Marking a known abandoned child inactive alone
does not provide the independent run-owner handoff described below.

Schema-1 instances remain readable and normal/CANCEL-reset writes remain
byte-compatible schema 1 (no new null field is emitted). Legacy busy rows are
never reclassified on read. Unknown/mislabeled schemas and incomplete or corrupt
proofs fail closed. Old readers reject schema 2. Downgrade requires drained
writers and resolved abandoned frames plus a compatible reader or verified
pre-change backup; never strip the proof, relabel the schema or turn pending
installed calls into completed replies.

### Durable online COBOL run-owner handoff

New admissions write `mainframe-env.online-exchange@2` in the existing
`online-exchange-v1` namespace. Its bounded `run_owner` contains the original
execution and a metadata digest; actor execution still changes on PROGRAM/XCTL
replacement. The existing staged `MEOM4` continuation carries this same owner
in its next exchange. Admission, invocation reconstruction, transfer binding,
CAS updates and recovery validate it without a new workflow or execution ledger.
Restore never overwrites a different installed-COBOL owner binding. When root
and actor differ, the root must have the same run/principal and an exact durable
Completed/HandoffCompleted terminal event, including version, attempt and tick.
Missing, failed, ordinarily completed or foreign roots fail closed. CAS updates
cannot change the root or advance the caller's version on validation failure.

The owner digest uses SHA-256 domain `mainframe-env.online-run-owner@2` plus a
zero byte, then u64 big-endian length-prefixed UTF-8 fields: root execution,
current execution, run unit, principal, program, selector, artifact, transaction.
The final length-prefixed field is canonical compact JSON for the tuple of
sorted grants, sorted provider generations, deadline tick and attempt. This is
an integrity binding, not a substitute for SAF or installed-call authorization.
Mutable COMMAREA, priority and blocking-effect updates do not change ownership.
Core retention validates the owning codecs and protects current/root executions
and staged prior/next/root executions. Corrupt or orphan continuation authority
sets the global unowned fence; it cannot permit core pruning.

Valid `mainframe-env.online-exchange@1` ownerless rows remain readable and ordinary
updates remain V1; reads do not promote them or guess their owner. New transfers
from those rows are Unsupported and require drain/reconciliation. V2 requires its
exact owner binding; unknown versions, schema relabeling, conflicting bindings and
malformed identities fail closed. Old readers reject V2. Rollback requires drained
writers and a compatible reader or verified pre-change backup, never dropping the
owner or relabeling V2 as V1.

The compiled root PROGRAM exit now completes after cold reopen with the original
owner COMMAREA and ABEND metadata; the pending child call remains unchanged.
This closes the COBOL run-owner handoff gap. The following CICS restoration
boundary addresses acquisition/UOW ownership separately; nearest non-root PROGRAM
exits and general/default ancestor recovery remain acceptance obligations.

### Restored CICS task owner versus replacement actor

The embedding restores an online replacement through
`restore_terminal_program_run(task, actor, ...)`. It must independently attest
the handoff before calling: the provider does not infer an owner from BTS or
UOW rows. The product reuses exchange V2's validated original execution and
exact Completed/HandoffCompleted proof. Only the original execution identity,
selector and artifact come from core authority; permissions, generations and
controls remain the validated current exchange scope, not a reconstruction of
unpersisted original Invocation fields. V1 ordinary same-actor restoration
remains supported without gaining transfer ownership.

The existing Run invocation retains task identity for acquisitions/settlement;
the current program frame retains actor identity for effects/audits. Both use
the same run, principal, attempt and generations. Actor controls cannot widen
the supplied task scope or alter execution-context/DPL outcome bindings. Warm
restoration validates existing owner/principal/generations/attempt, retains task
file updates, current records, browse cursors and channels, and replaces the
root logical frame without inventing a LINK parent. Selector/artifact equality
with a warm entry invocation is not an ownership predicate: executable admission
legitimately resolves those fields after terminal launch.

Cold restoration reloads the existing durable HANDLE and undo authorities; this
does not add persistence for volatile cursors or channels. Session version is
rechecked after the undo read, and existing idle-session/live-loan/cleanup fences
and capacity checks precede volatile replacement. Failure cannot remove the
existing task. The legacy restoration entry point delegates with task == actor.
No codec, namespace or retention target is introduced. Distinct task/actor UOW
rows use the existing MECU3 provenance and both-owner retention contract; root
rows still use MECU2. Downgrade keeps the already documented V2/MECU3 drain and
compatible-reader/verified-backup requirements, never stripping ownership.

Compiled root HANDLE ABEND PROGRAM tests now acquire a pending BTS process
before ABEND and commit or roll it back from the exit after SQLite/PostgreSQL
physical reopen. They verify successful RESP, process visibility/removal,
released acquisition, actual core effect actor, UOW actor/root provenance,
unchanged pending child CALL and final original COBOL run completion. This
proves explicit SYNCPOINT settlement only, not every implicit task-end or
non-root/default ancestor recovery obligation.

## Installed transfer target staging

After schema-3 observed source intent, the embedding can stage an immutable
local target in the same original CALL row. The existing CICS Transfer response
carries optional `PROGRAM.SELECTION`, schema
`mainframe-env.cics.program-selection@1`, with canonical MECPGD1 definition
bytes. XCTL and current/ancestor HANDLE ABEND PROGRAM freeze the definition
before existing outer replay and core result publication. Direct PROGRAM exits
use the issuer frame's entry COMMAREA, not a nested leaf's area; the historical
root-only retrieve fallback remains supported. This does not complete handler,
channel or task-storage lifetime semantics.

`attested_program_transfer_selection` is a read-only CICS adapter. The caller
must obtain the source effect from the trusted core store and independently
validate the source invocation. The adapter requires Completed canonical host
result proof with matching owner/run/attempt/capability/key/sequence/deadline,
the exact existing MECER003 replay tuple and observed Transfer payload, and
canonical frozen definition bytes matching the immutable generation row.
Only enabled local, non-Java, offset-zero executable artifacts are eligible.
No latest-generation or generic program-name fallback is allowed. Missing,
legacy, malformed and unavailable selections leave the original CALL unknown.
Reserved selection outputs are also validated before retention classification.

The pending CALL advances JSON schema 3/CAS 2 to schema 4/CAS 3 with `target`:
selector, artifact, generation, content identity, invocation, context digest,
checkpoint schema, canonical padded base64 checkpoint, checkpoint digest.
The strict invocation DTO stores request/execution/run/parent/selector/artifact,
principal/sorted grants, class/priority/deadline/trace/key/attempt, six resource
limits, sorted schema/base64 bindings, sorted provider generations, audit,
optional cancellation and live-probe presence. JSON follows the declared DTO
field order, compact serde encoding, and BTreeMap ordering. Resource limits
retain u64 effects/events and u32 frames. A caller must explicitly reinstall a
trusted run-scoped live probe; no pointer or observed probe state is serialized.

Existing installed-call length framing binds `installed-transfer-target@1`,
CALL key, source execution, generation (big-endian u64), and definition content
identity to deterministic target request/execution/trace/effect identities.
`staged-invocation@1` frames the canonical DTO JSON; target metadata frames
`installed-target-stage@1`, selector, artifact, generation, content identity,
context digest, checkpoint schema and checkpoint digest. Receipt metadata uses
`mainframe-env.cobol-call-receipt-metadata@4\0`, the unchanged prior owner/reply
framing, then source-intent and target-metadata digests in that order. SHA-256
digests are lowercase hex; definition/artifact identities retain `sha256:`.
Receipt JSON is bounded at 64 MiB; invocation binding limits and checkpoint
constructor limits remain independently enforced. Receipt 4 is pending-only
and cannot carry a reply or completion tick. It retains explicit provider-row
dependencies on the frozen definition generation and source CICS effect replay,
alongside existing CALL/source/caller/run dependencies. Syntax-only checkpoint
validation is protective retention metadata, never execution authorization.

The stored MECP0012 image is exactly a fresh reference-machine constructor
checkpoint: it precedes COBOL init operations, target instance admission and
last-used instance state. Inherited invocation bindings preserve the entry
COMMAREA even though unexecuted linkage storage is initially zero. Target
execution is not registered and source Suspended/checkpoint/busy-instance
authority is unchanged. A future consumer must revalidate source/core/result
proof, exact artifact and constructor checkpoint, trusted controls and instance
authority; acquire same-level CICS admission, perform owned handoff and execute
the target before any caller restoration or terminal claim. Warm/cold task
resources are not made durable by this staging phase.

Readers retain legacy schema 1 and ordinary schema 2, and pending schema 3;
unchanged ordinary writers still emit schema 2. Old readers reject schema 4.
Downgrade requires draining or preserving pending 3/4 rows with a compatible
reader and verified backup; never strip target/context/selection metadata or
relabel a pending row as completed. Artifact removal is not authorized by this
phase; unavailable immutable artifacts fail closed, not catalog replacement.
No new coordinator, provider namespace, executable host contract or ledger.

### Same-level synchronous task admission prerequisite

The next bounded manager contract uses the existing task claim and top program
loan for an attested installed Transfer. The embedding must independently prove
the retained canonical source command/result, original pending CALL, frozen
immutable selection, trusted source invocation and live source/target instance
ownership before requesting admission. A constructor image or integrity digest
alone is insufficient. Cold busy rows never recreate the synchronous loan.

At admission, the top loan and current task actor must exactly match the source
on the current executor thread, with no command outstanding. The target has the
source execution as its durable parent while retaining the same CICS logical
level and original invoking/return programs. Root task identity and shared
file/UOW/BTS resources remain owned by the original task. Principal, grants,
provider generations, live controls, service class and resource/deadline limits
cannot widen. Frozen program generation/artifact/content identity must match the
retained immutable definition; no latest-generation or name-only fallback.
The target's entry COMMAREA/current channel come from the independently attested
Transfer and admitted invocation. This loan provides no shared virtual memory.

Only the existing top loan actor is rebound. The source cannot reenter commands
or be restored as the running frame after replacement. The original outer
program loan still owns caller restoration after known target completion.
Unknown outcomes and abnormal unwinding retain the existing uncertain-session
fence; no terminal receipt, source retirement or target redispatch is inferred.
The separate original-CALL/instance contract must bind exact CAS versions and
cross-authority handoff proof before this helper can be connected to execution.
No durable codec number, new namespace, coordinator, routing or full-frame
acceptance is introduced by this volatile prerequisite.

### Exact-version source core terminalization prerequisite

`complete_suspended_handoff_at_version` uses the existing execution record,
lifecycle journal and atomic core CAS. Before mutation it requires the exact
positive, successor-compatible source version; execution/run/principal identity,
selector, immutable artifact and attempt; Suspended state without terminal age;
and the matching final Suspended event. The supplied positive terminal tick must
be monotonic and fit the existing durable integer domain. It never opens generic
resumable admission, which would create a missing source or relax suspended
selector/artifact identity. Races or disappearance are rejected by the existing
CAS; no second coordinator or namespace is introduced.

The Completed/HandoffCompleted step and its event/outbox are atomic through the
existing owner. Checkpoint deletion remains subsequent cleanup and can fail after
the terminal step committed. An error does not prove non-execution or authorize a
retry; the embedding must separately observe the exact terminal record/event.
This helper grants no provider disposition, target instance lease, CALL reply,
volatile task loan or redispatch. The existing generic handoff remains unchanged.
Source snapshot semantics and the original CALL/instance proof chain are separate
prerequisites before any runtime use.

### Read-only transfer and warm instance attestation prerequisite

The pending schema4 constructor is revalidated against the exact source
Suspended record/event/checkpoint, canonical completed CICS Transfer effect,
retained command replay and frozen immutable selection. The restored source
machine must match the captured checkpoint and effect sequence; the target image
must equal an independent fresh constructor with the exact inherited invocation
and COMMAREA. Rehashed executed images or widened context do not gain admission.
Live controls are checked without writes, target dispatch or fence changes.

The existing warm instance Lease retains its acquiring invocation in memory.
Read-only token observation requires that exact actor/context, source row CAS and
payload, current run ownership/membership/active counts and a closed source.
Only an idle compatible target and validated ordinary last-used target image are
accepted. It returns observed run/source/target CAS versions without reserving,
retiring or executing either program. It cannot reconstruct a token from cold
busy rows, prove a source transfer disposition or create a completed CALL reply.
This prerequisite preserves the existing schemas and unsupported runtime result;
source-state snapshot authority and durable CALL/instance phases remain separate.

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
