# MQ program-to-machine frame handoff

Status: **Proposed**
Owner: **program/execution and MQ maintainers**
Scope: **explicit typed installed-batch handoff, not MQ public readiness**
Applies from: **mainframe-env 0.15.0**

## Decision

The installed-program host can be configured with a single
`ProgramMqHostAdmission` before runtime binding. After actual installed artifact
admission and the winning original durable CALL reservation, the batch executor
constructs a nonserializable `InstalledBatchAdmission` for that factory. Its
private constructor preserves actual parent and parentSome child Invocations,
catalog namespace/key/version or selected-program provenance, validated artifact
reference/content/manifest and original compiler/interface metadata, original
enclosing ProgramCall, retained canonical core intent and running parent, exact
CALL reservation and frozen physical store/control/host/artifact-store references.
The proof has read-only accessors, no Clone/Serde or public numeric constructor.
Application bindings cannot choose the factory.
Contradictory CICS task/outer/child provenance is rejected, not erased. The
factory must independently admit process/frame topology and use the selected
service's lifecycle/registry authority; decoding this binding is not attestation.

Factory installation and runtime publication use one setup guard. Repeated or
partial runtime setup fails before publishing new fields. Typed runtime publication
also freezes the first execution-control binding; legacy embeddings retain their
existing control installation after construction and before dispatch. Admission
copies the configured factory/store under that guard and releases it before
invoking embedding callbacks. This guard is not the complete trusted producer's
service/store/authorizer/clock bundle, which remains a separate obligation.

With typed configuration, the original parent occurrence must be retained on
that configured store as an unrecovered canonical intent with exact execution,
run, attempt, sequence/key, full request digest, capability, audit attribution
and finite live deadline. The current parent must remain running with its exact
principal/selector/artifact. The pending CALL record and admitted catalog are
rechecked before and after the factory; clock regression/cancellation/deadline
and changed observations reject preparation. Driving rechecks the original core
and CALL dependency while leaving cancellation/timeout classification with the
existing coordinator. No protected reservation is deleted or automatically
retried. Legacy paths without a typed factory keep their existing behavior.
Direct scheduler/helper paths without a real retained original occurrence are
Unsupported for typed admission; they cannot manufacture an enclosing call.

This is **admission provenance**, not independently admitted host-root/process
attestation, an MQ lifecycle lease, SAF or an atomic publication permit. The
real selected bridge must match the proof's physical Arc store and frozen setup
to its SAME service, independently admit actual host parent/process/task topology
and current incarnation, and retain core/CAS/audit ownership. Equal rows in a
foreign store and decoded binding/owner bytes cannot perform that check. The
observed core/execution reads do not create whole-store serialization or a
coordinator permit; physical publication and live lifecycle fencing remain
their original authorities.

The factory returns a bounded `InstalledMqFrameSession`. Its `program_frame`
uses the existing `MqMqiProgramFrame`, explicitly installed on the reference
machine before driving. Its profile supplies the independently minted owner
and bounded MQI limits. The machine rechecks that profile before constructing
the original typed effect and before consuming its result. A changed owner or
profile cannot rewrite an already journaled effect. The existing effect builder
owns sequence, key, run, deadline and Mutation identity. ScopedHostService and
the durable coordinator retain capability, audit, original intent and completion
ownership. This port grants no SAF, queue, UOW, recovery or participant permission.

The session declares its physical store and execution-control Arcs. The server
compares both against the frozen admission setup before installing any frame;
equal record/observation bytes on different adapters are rejected and preparation
is aborted. This necessary composition check is not independently admitted
service/root identity. A real selected factory must still bind its own SAME
service authority and host root, rather than merely echoing observed Arcs.

The server owns one session guard per admitted child. All binary/frame/environment
preparation failures receive one `abort_preparation`; all returned raw coordinator
outcomes receive one `finish` before ProgramOutput/HostProblem mapping. Completed,
Condition, Abend, Cancelled, TimedOut, ResourceExhausted, ProviderFailure (including
Unknown), InfrastructureFailure, Rejected, Suspended, Invoke and Transfer retain
their original distinctions. Neither return nor Condition proves task/process
end. Abort/finish failure or panic becomes protected UnknownOutcome. The transport
is invalidated before callback invocation, and a callback is never retried. Drop
only invalidates bounded executable transport: it invokes no cleanup callback,
commit/backout/MQDISC or detached queue. Unwind with no returned raw outcome leaves
the original CALL/core uncertainty protected by the existing host protocol.
Factories must roll back only newly owned volatile preparation on admission
error and must not perform durable cleanup in session Drop. Actual source-bound
UOW/task-end and fallible retirement policy belongs to the real selected bridge.

The first adapter maps MQCONN/MQDISC for ordinary z/OS batch. It validates exact
argument count, MQCHAR48 input and signed fullword output storage before dispatch,
preserves case/significant name padding, and retains actual issued HCONN tokens
behind bounded, non-reused application ABI aliases. It never creates registry
tokens from application integers. MQDISC retires its alias on actual successful
completion; retaining undefined z/OS Hconn bytes does not retain access. Reviewed
failure status is copied exactly without substituting a connection output.
Uncertain, malformed or unusable post-dispatch replies remain UnknownOutcome.
Other known MQI calls fail closed under this initial frame rather than bypass
it through legacy MQ; unrelated program names remain ordinary program calls.

The typed adapter additionally maps MQCMIT/MQBACK for this ordinary batch/local
queue-manager profile. The embedding frame's read-only `local_unit` lookup takes
the actual live HCONN token, never an application integer, and supplies a current
local-UOW assertion before effect construction. It is not a permit: original
intent, logical owner, physical incarnation/control and UOW CAS remain the
selected provider's responsibility. Older frames default to Unsupported. The
adapter rechecks the same profile after lookup and keeps the existing immutable
effect's sequence/key/actor. External/absent/zero units fail before dispatch.

All three arguments are checked signed fullword reference storage; successful
UOW output must match the original asserted unit. Actual CompCode/Reason
observations use the one reviewed call-specific status authority, including
warnings and failures. Neither status writeback nor normal CALL return decides
or advances durable work or retires connection aliases. Unknown/duplicate,
changed profile, wrong unit and unusable typed reply envelopes remain protected
UnknownOutcome before application writes; legacy reply-validation errors and
checkpoint bytes remain unchanged. These machine/frame fixtures are not a real
installed MQ service producer, shared participant or task-end policy. The real
session containment must forward the lookup through its live revocation guard.

The unchanged legacy checkpoint schema has no typed lifecycle/alias references.
Typed frames therefore do not emit that checkpoint or restore it. A serialized
binding cannot install a frame after restored/driven execution. Proper durable
checkpoint/handle replay is still required before public profile acceptance;
this explicit refusal is not an inapplicable-gate disposition. Legacy machines
and their checkpoint schema/bytes remain unchanged. Their earlier MQ adapter is
mechanically extracted to a child module to preserve the production-line ratchet.

Public in-process `snapshot()` exports a typed source only as diagnostic schema
zero, which no restore accepts, including a fresh unbound destination. Zero is
not an accepted machine/checkpoint version. Manual binary projection retains that
invalid marker; `MachineSnapshot` has no Serde projection that removes it. The
existing destination frame refusal also remains. Legacy sources still export
schema 12 and unchanged checkpoint bytes. Editing public diagnostic fields cannot
create provider lifecycle or registry authority. Actual typed durable snapshot
and historical-handle adoption support remains pending.

## Sources and acceptance boundary

### Private same-task child ownership

`MQ-1505.selected-batch-child-ownership` adds a checked provider-private
`prepare_selected_batch_child(parent_frame, parent, child, relationship)` path.
The existing opaque parent lease and exact frozen parent Invocation are required.
The trusted, already-admitted host must independently supply the ordinary SAME
TASK CALL relationship; the private selector is not an attestation capability.
Equal bindings, parent IDs or run/principal alone cannot establish that relation.
Separate subtasks, clients, CICS and IMS cannot inherit this batch processing unit.
Root minting/binding still refuses every `parentSome` Invocation.

The private explicit-context plane admits an original unbound parent without
inserting `mq.host-context` into its Invocation. The trusted embedding separately
selects ordinary `ZosBatch/QueueManager` configuration against the same selected
service/store. Explicit mint/bind freezes that mode in an opaque process lease;
the existing binding-only methods cannot upgrade it or fall back to it. Present
MQ/CICS bindings still use the single decoder and must match; malformed bindings,
client/IMS/host-owned contexts and contradictory CICS origins fail closed.
Checked same-task children inherit only their live parent's frozen mode. A
bounded directory-issued proof ties scope context to the exact original snapshot
and owner under the sole authority mutex. Neither this private parameter nor
decoding application JSON is host attestation. Original CALL/core/effect digests,
child actor attribution, UnitOwner@1 and replay bytes remain unchanged. The real
cross-crate producer/session bridge and public acceptance remain separately owned.

The directory checks the actual child linkage, distinct execution, exact
run/principal/grants/generations/attempt, shared physical cancellation probe,
live controls and non-widening deadline/resource limits before bounded frame
insertion. Each root batch frame retains a small frozen logical origin; only a
checked same-task child copies it. Surviving frame references retain that origin
without reconstructing it from a CONNECT receipt. Origin bytes count against the
existing directory budget. No lease or origin has a public/Serde constructor.

The selected service obtains that proof afresh under its sole authority mutex.
It validates access to the original connection's retained logical owner rather
than reassigning `UnitOwner@1` to the child. Existing owner schema and root bytes
remain exact. Child GET/PUT and explicit batch CMIT/BACK/DISC retain the actual
child's immutable effect, core intent, SAF principal, audit and occurrence
receipt. Physical publication still composes current control/UOW dependencies,
catalog/marker/delivery CAS and insert-only receipt with audit in the same store
transaction; adoption follows the entire commit. Nested/outer intents cannot
replace the child's original intent. Reply uncertainty fences this service and
leaves reconciliation with the existing recovery authority.

Explicit preparation abort removes only a newly created child frame. Ordinary
CALL return removes a nonfinal child reference without retiring task handles
or deciding pending work; no Drop cleanup is installed. A checked same-task child
may establish the task's first default, nonshared connection. Registry ownership
uses the admitted task/thread/epoch; the unchanged durable owner record retains
the logical origin and that child's original CONNECT key. Its core intent, SAF,
audit and receipt retain the actual child actor. A surviving admitted parent or
next same-task child can use the retained connection, objects and current UOW;
explicit CMIT/BACK/DISC still require that caller's original controls, authority
and whole audited publication. Repeated CONNECT remains explicitly unsupported
until its existing-connection warning/output policy is composed.

This rule relies on the trusted host proving an ordinary CALL in the same
continuing task. MQCONN's nonshared z/OS scope is the task, excluding subtasks;
the handle expires on MQDISC or termination of that processing unit. The source
does not attest topology from Invocation fields, and CALL return is not treated
as task termination. Final task end and raw abnormal/unknown host outcomes need
separate owned integration. Cold restart restores retained UOW/receipt bytes, never opaque
lineage or tokens; durable incarnation advancement remains mandatory. This
private composition awaits the real installed host producer/session and deliberate
cross-crate authority design. It does not register a ready public provider or
accept a shared participant.

IBM MQ 9.4 baseline `ibm-mq-9.4-mqi-2026-08-31`, row `0008` MQCONN,
`SSFKSJ_9.4.0/refdev/q101760_.html`, SHA-256
`fa0cdd2c5e19326dfb91e5ad0b921fd47a1a3a918682e13c4ff5e36c2ba40347`,
defines the name, nonshared scope and returned Hconn. Row `0012` MQDISC,
`SSFKSJ_9.4.0/refdev/q101800_.html`, SHA-256
`8e33bfec37f7fb467b9f206e8d2f03dc84a18bebf230068582dd84b7d4375e36`,
defines the input/output Hconn and undefined z/OS value after success. Completion
numbers come from the existing reviewed status authority, not another table.
Offline source review grants zero execution/licensed credit.

The syncpoint handoff also reviews baseline `ibm-mq-9.4-mqi-2026-08-31`,
row `0007` MQCMIT, `SSFKSJ_9.4.0/refdev/q101750_.html`, SHA-256
`590f32c213d129d6937c253f048ccdb9c5cbd963ca2d3310cf5672dcf68c42f4`,
and row `0001` MQBACK, `SSFKSJ_9.4.0/refdev/q101690_.html`, SHA-256
`9550bf98c66f47f1d61943e0dbb7ab89043d0db1a3918ea3a4314dccf7182c86`.
Their signatures use Hconn input and CompCode/Reason output; the reviewed usage
requires queue-manager coordination and the same connection's UOW. CICS, IMS
transaction-manager and RRS/shared resource work are not admitted by this port.

ProductServer still opens and registers the previous MQ profile. It does not
automatically configure this factory. A real factory must bridge the admitted
installed program CHILD topology to the selected private directory; pretending
it is a parentless root is prohibited. Actual service/SAF/UOW/receipt publication,
handle-valued Completed replay, fenced Unknown recovery, frame-end policy,
checkpoint/retention and all applicable 26-call contexts must compose before
readiness. The source signatures and compiled-program/journal fixture tests prove
only this handoff, not owned MQ mutation, participant or CardDemo acceptance.
