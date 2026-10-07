# MQ root publication framework

- Status: Proposed (implemented framework prerequisite; native acceptance remains pending)
- Date: 2026-10-03
- Owner: execution coordinator, store adapters, configured COBOL host and MQ provider
- Scope: the explicitly configured synchronous ordinary z/OS batch root
- Applies from: mainframe-env current subsystem contracts

## Decision

The configured compiled `parentNone` root uses the existing artifact/catalog
validator, frozen scoped host router, physical selected MQ provider Arc, physical
PlatformStore Arc and its original execution-control Arc. The setup is a
deliberate Rust embedding choice. Application JSON, matching identifiers,
bindings and a completed execution row cannot construct this producer. The
original Invocation is preserved. Legacy ProductServer and coordinator entries
do not acquire this route or a terminal hook.

The embedding registers `DefaultProgramRouter::native_root_program_provider`
before freezing the host. This closed weak program facet delegates only to its
actual router. Root admission checks the exact physical program and MQ provider
Arcs; matching descriptors or an alternate forwarding wrapper are insufficient.
Native scope accepts the modeled LE `CEE3ABD` selector at its existing ABI only,
alongside genuine compiled ordinary CALLs. A program name alone cannot admit a
different runtime service as a native-abnormal producer.

`ExecutionCoordinator::execute_root_with_control` owns the exclusive real drive.
It inserts `RootDriverAdmission`, associates the live opaque MQ root eagerly,
and performs a once-only machine frame preparation before the first drive or
effect. Only that drive constructs the borrowed `NativeRootAdmission` and
`WinningRootTerminal`. Descendant compiled CALLs enroll their original child
executions through the actual winning original CALL reservation. The original
parent intent, full request digest, child catalog and parent linkage remain
frozen. Child originals remain `parentSome`; no child becomes a fabricated root.

The shared JournalStore extension owns durable root membership and a Closing
gate. `RootDriverClaim` is an opaque structural observation of inserted core
ownership, not host attestation or a settlement permit. Membership indexes cover
the run, every enrolled actor, exact registered lifecycle identities and complete
configured provider scopes. Existing low-level core/effect/event/outbox/work/
checkpoint/provider writers must respect the same gate. This first synchronous
profile refuses scheduled work, live checkpoints, recovery leases and unknown
descendant closure. It does not truncate a global scan into an ownership claim.

Direct checkpoint insertion and checkpoint-bearing journal steps both refuse
enrolled actors, including while Open. Direct event append and journal steps
classify the event kind independently of an optional state update: root
Completing/terminal events require the composed terminal publication. Genuine
child completion and ABEND remain permitted while Open. SQLite audit-only
publication rechecks the same actor gate inside its writer transaction; new
outbox insertion also refuses Uncertain. Existing exact outbox delivery remains
available outside Closing, including Terminal and Uncertain; it cannot insert or
change notification content. These guard parity repairs supply physical store evidence,
not installed native pending-work acceptance.

Standalone effect-audit writes also obey the enrolled actor gate. Generic
provider-row writers cannot copy or insert a root-terminal audit subject;
the existing effect-audit decoder rejects that class, and only the composed
root publication's private physical writer creates it. Unenrolled legacy
authorization diagnostics retain their existing authority and behavior.

`close_root_driver` captures the complete bounded graph under the physical
lock/transaction. Distinct `Exact` and `Absent` dependencies are not provider
mutations. The mutation vocabulary remains Put/Delete/Move. The server's full
existing CALL and COBOL instance parsers validate captured bytes. Completed child
rows alone cannot replace the original parent CALL result digest and retained
compiled provenance. Scope captures preserve unrelated immutable history.

The MQ root prepares under the same sole selected authority mutex. It validates
its original root/logical owner, existing registry lifetime, connection receipt,
original core occurrence, selected control/incarnation, unit provenance and
mandatory resource-specific EnterpriseAuthorizer. It stages a prospective rich
state through the existing delivery planner and strict reader. No new unit is
allocated after terminal settlement. No application MQDISC, synthetic
EffectRecord, private lifecycle journal or second queue engine is introduced.

The server composes prepare-only COBOL cleanup with original legal core terminal
steps, exact existing lifecycle outbox bytes and two distinct typed audit
subjects. `commit_root_terminal_step` publishes this whole plan once. Memory
uses its touched-entry rollback under one lock; SQLite uses its existing writer
transaction and quota/epoch/clock authorities. Final whole capture, dependency,
epoch, row CAS, lifecycle sequence, audit and quota checks are inside that same
transaction. Any failure restores all touched rows and counters. There is no
sequential publication fallback. Other backends return the default refusal
(`InvalidTransition`) without writes.

## Policy and uncertainty

The accepted source selector is actual normal machine completion or a genuine
modeled native ABEND, with all descendants closed. Ordinary normal pending local
work selects commit; known abnormal pending work selects source-backed backout.
Generic errors, cancellation, expiry, panic, Drop, missing reply, unresolved
child, quota/CAS failure and lost acknowledgement select neither. They retain
the actual durable winner/work and fence the shared live gate as UnknownOutcome.
They cannot select a guessed commit-impossible fallback or redispatch.

The real source-known commit-impossible classifier is a separate required
dependency. There is no classification from generic infrastructure failures.
Live full-message BACK uses the existing source-backed delivery helper. It does
not prove crash accuracy between GET and BACK or supply an unconfigured
HardenGetBackout queue attribute/default. Those policies remain separate.

After a known physical commit and a still-live control window, checked directory
retirement revokes the actual root/process and registry entries. A raced or lost
acknowledgement keeps the transport fenced even if durable publication won.
Normal nonfinal child return remains distinct. Drop makes no durable decision,
implicit DISC, cleanup callback or detached work. Historical handles cannot
revive cold live authority. Native root history is conservatively protected from
retention; this extension supplies no age reconciliation, independent pruner or
scope/slot handoff that discards uncertain history.

## Lock order and bounds

The exclusive coordinator drive establishes Closing, then the server releases
all topology-map locks before artifact/control callbacks. The same MQ mutex
holds preparation through physical publication and candidate adoption. SAF and
live control probes execute outside the physical transaction; durable control,
complete core closure and provider dependencies are rechecked inside it. No
arbitrary provider/authorizer callback executes under a backend transaction.

The entire graph and composed plan are bounded before deep copying/encoding:
at most 256 actors, 4,096 combined captured/terminal operations and 64 MiB,
further narrowed by actual backend/profile quotas. Max+1 is an unsplit refusal.
The first profile also refuses live cursor reclamation without its actual owning
task-end policy. There is no namespace wildcard grant or acceptance-cap increase.

## Versioned storage and deployment

Core ownership uses `mainframe-env.core-root-ownership@1` and the bounded
`durable-root-*-v1` indexes. Terminal audit subjects use
`mainframe-env.root-terminal-audit@1` under the existing audit ordinal authority.
Old effect audit bytes retain their meaning. The typed subject reader returns
both families; an old reader refuses a terminal subject instead of omitting or
misreading it. Shared streaming canonical root setup/resource domains are
independent from unchanged HostCanonicalV1 request/result bytes.

No SQL table migration or operator action is performed by this feature. Before
deliberately enabling the new versioned row/subject profile, deployments must
drain every old writer/reader, verify a backup, and prove compatible new core,
provider, audit and recovery readers. Downgrade while these rows or their history
remain is refused; stopping admission and restoring the verified pre-enablement
backup is required. A retained root cannot be erased to make an old binary work.

## Evidence boundary and remaining work

### Original-effect attributed writer prerequisite

`JournalStore::mutate_root_provider_states` accepts a nonserializable
`RootProviderPublication`: the existing structural root claim and occurrence,
exact current execution and original intent metadata, plus owned mutations.
Memory and SQLite compare the actual retained execution, canonical unresolved
intent, actor/run indexes, Open document, live deadline/lease/clock and every
namespace or exact-row membership inside their sole publication lock or writer
transaction. Both Move endpoints must belong to this same root. Structural
observations are not host admission or a writer permit; unsupported adapters
refuse without a sequential fallback. The existing mutation kernel owns writes
and rollback. The operation creates no audit, intent, result or terminal decision.

Intent-audited publication derives its private root attribution only after its
existing physical intent/execution fence succeeds. Each indexed scope must have
that same enrolled actor's root owner; an Open actor elsewhere is insufficient.
Unowned legacy rows and audit-only/deny forms retain their existing semantics.
Rooted batches share the 4,096-operation/64-MiB bound, with Move counted twice.
Anonymous APIs and all existing ownership/index/schema bytes stay unchanged.

### Initial root preparation prerequisite

The additive `JournalStore::mutate_root_preparation_states` accepts a nonSerde
`RootPreparationPublication` observation and owned finite row batch. Memory and
SQLite require the full original current Admitted version1 execution, exact
original inserted Open/root-only membership, single exact Admitted event and
every initial actor/run/namespace/exact-row index inside the existing physical
transaction. Any effect for the actor/run, work or checkpoint refuses. The
registered anchor and both Move endpoints must belong to the same claim, with
all applicable indexes checked for conflicting ownership. No dynamic enrollment,
intent/audit/event/outbox construction, terminal decision or Running/child stage
is provided. Unsupported adapters refuse without fallback; Durable routes only
SQLite. Existing wire/schema bytes and anonymous/original-effect guards remain
unchanged. The sole mutation kernel and touched-entry rollback publish the batch
and logical clock together under existing4096-operation/64-MiB and backend bounds.

The observed tick must be at or after admission and the retained floor and before
the original deadline. An acquired lease or advanced execution refuses. This is
logical freshness, not a physical Work/Job expiry decision after lock wait. Eager
schema3 setup in the actual admitted server hook is a separate next feature;
this store prerequisite neither wires that hook nor bypasses original CALL
publication. Scope succession, retention and native acceptance stay pending.

The original-effect operation is step 1, not server writer wiring or scope succession. Genuine server
protocol/instance/CALL writers must later supply their retained original effect
and current controls. A lifecycle writer without an original effect needs a
separately reviewed live-actor contract, never a fabricated intent. Scope reuse
remains refused; no history, outbox delivery, retention or recovery authority is
released by this prerequisite. Store tests supply no compiled/native acceptance.

The framework's physical store tests and genuine compiled connection/CALL tests
are separately identified external receipts. They are not complete native PUT,
installed default, shared-participant, all-26 or licensed execution evidence.
This prerequisite does not supply the original typed compiled OPEN/HOBJ/PUT
and qualified FullGET routes required for the mandatory pending-PUT normal
commit, genuine CEE3ABD backout and complete removed-GET terminal backout
vertical proofs on Memory and owned SQLite. Those owning compiled producer
features must compose their original ABI frame and native root enrollment, with
the original effect/core/CALL/SAF identities intact. Manual pending seeds,
legacy dispatch and fabricated outputs cannot substitute for those proofs.
This ADR does not turn connection-only tests into native root acceptance or
complete the native-root implementation goal.

Broader native recovery and lost-reply reconciliation, source-known fallback,
HardenGetBackout crash accuracy and queue profile, typed checkpoints, final
retention/ownership release, deployment normalization, participant composition,
all MQI forms and CardDemo remain required owning acceptance work.

The physical publication failure contract additionally compares complete fixture
observations and SQLite provider-table bytes, epoch and clock across known normal
and abnormal commits, final CAS/dependency and quota failures, second-audit,
final-outbox and clock-write SQL faults, and owned close/reopen. Discarding a
successful return simulates a lost acknowledgement: duplicate and opposite
publications refuse without changing the retained winner. An Uncertain root
continues to refuse settlement and new writers after reopen while exact existing
notification delivery remains allowed. These tests use structural store fixtures;
they supply no provider settlement permission, volatile handle reconstruction,
root retry, retention release or compiled/native acceptance. Close/reopen is
logical restart simulation, not process-crash or mid-transaction kill evidence.

Source review: original `ibm-mq-9.4-mqi-2026-08-31` MQDISC row 0012
(`q101800_`), MQCMIT row 0007 (`q101750_`) and MQBACK row 0001 (`q101690_`);
supplemental `q097395_` BackoutCount and one-topic recovery scope `q103230_`
HardenGetBackout. Hash-verified offline reference review earns zero execution
credit. The unavailable licensed differential oracle alone is human-skipped
0/26; no other acceptance is waived.
