# MQ program-to-machine frame handoff

Status: **Proposed**
Owner: **program/execution and MQ maintainers**
Scope: **explicit typed installed-batch handoff, not MQ public readiness**
Applies from: **mainframe-env 0.15.0**

## Decision

The installed-program host can be configured with a single
`ProgramMqHostAdmission` before runtime binding. After actual installed artifact
admission and construction of the program's Invocation, the batch executor
selects its fixed batch/queue-manager context and calls that factory with the
same physical PlatformStore. Application bindings cannot choose the factory.
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

The returned `MqMqiProgramFrame` is explicitly installed on the reference
machine before driving. Its profile supplies the independently minted owner
and bounded MQI limits. The machine rechecks that profile before constructing
the original typed effect and before consuming its result. A changed owner or
profile cannot rewrite an already journaled effect. The existing effect builder
owns sequence, key, run, deadline and Mutation identity. ScopedHostService and
the durable coordinator retain capability, audit, original intent and completion
ownership. This port grants no SAF, queue, UOW, recovery or participant permission.

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
CALL return removes a nonfinal child reference without retiring parent handles
or deciding pending work; no Drop cleanup is installed. Final task end, raw
abnormal/unknown host outcomes and child-only CONNECT/return policy need separate
owned integration. Cold restart restores retained UOW/receipt bytes, never opaque
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
