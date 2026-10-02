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

IBM MQ 9.4 baseline `ibm-mq-9.4-mqi-2026-08-31`, row `0008` MQCONN,
`SSFKSJ_9.4.0/refdev/q101760_.html`, SHA-256
`fa0cdd2c5e19326dfb91e5ad0b921fd47a1a3a918682e13c4ff5e36c2ba40347`,
defines the name, nonshared scope and returned Hconn. Row `0012` MQDISC,
`SSFKSJ_9.4.0/refdev/q101800_.html`, SHA-256
`8e33bfec37f7fb467b9f206e8d2f03dc84a18bebf230068582dd84b7d4375e36`,
defines the input/output Hconn and undefined z/OS value after success. Completion
numbers come from the existing reviewed status authority, not another table.
Offline source review grants zero execution/licensed credit.

ProductServer still opens and registers the previous MQ profile. It does not
automatically configure this factory. A real factory must bridge the admitted
installed program CHILD topology to the selected private directory; pretending
it is a parentless root is prohibited. Actual service/SAF/UOW/receipt publication,
handle-valued Completed replay, fenced Unknown recovery, frame-end policy,
checkpoint/retention and all applicable 26-call contexts must compose before
readiness. The source signatures and compiled-program/journal fixture tests prove
only this handoff, not owned MQ mutation, participant or CardDemo acceptance.
