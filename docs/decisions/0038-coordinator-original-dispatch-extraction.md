# ADR-0038: Private coordinator original dispatch extraction

Status: Accepted bounded internal extraction; controller activation pending.
Owner: **Interpreter and execution-coordinator maintainers**
Scope: **MQ-1503.coordinator-original-dispatch-extraction, target mq.programming**
Applies from: **mainframe-env current subsystem contracts**

## Decision

The actual coordinator machine loop delegates its existing HostCall arm to one
private `coordinator/original_dispatch.rs` implementation. A transient borrow
references the same original Invocation, optional JournalCursor, controls and
observer. It carries the existing resumable/native-child mode, without a
constructor from rows or IDs, public Running facade, context selector, cache,
lease or additional journal. Admission and Running publication remain with the
actual enclosing drive.

The helper returns `Result<EffectResult, ExecutionOutcome>`. The machine alone
sets its HostResult resume on success. A stop returns directly, preserving any
lifecycle event already recorded by the original branch or check_control.
Intent, result, typed audit, event and outbox publication remain their existing
single journal authority; no completion or audit is duplicated.

Canonical request/result preimages, original core metadata and occurrence keys,
observer cadence, pre/post dispatch controls, Unknown precedence, native root
and child local restrictions, optional-journal local/audit-only behavior and
explicit Completed replay with full result-digest verification remain exact.
Intent/Failed/Unknown replay still requires reconciliation. Failed journal
publication never rewinds the cursor or enables a retry. Ordinary observer and
machine panic limitations remain unchanged; native catch/protection remains
its separate existing path. No public entrypoint or durable schema changes.

## Remaining prerequisites

This extraction supplies no standalone Running execution owner to the Batch
callback in [ADR0037](0037-running-step-program-transport.md). Actual enclosing
controller admission/ownership, ControllerExit/stop latch, the private server
JesClaimRun join, physical Work/Job decision-time clock, scheduled/compiled
terminal ownership and genuine DefaultContext remain separately unimplemented.
Neither a matching row, public Invocation nor retained Running-step view can
mint the missing journal ownership. No Program-context receiver is activated.

Focused real ReferenceMachine and existing owning regressions verify the
extraction's boundary; fixture host ports are not installed/native/JES, SAF,
participant, official full26 or licensed acceptance. Parent mq.programming remains active
and its nonlicensed obligations remain required.
