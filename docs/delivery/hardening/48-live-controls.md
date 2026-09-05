# Live execution controls — issue #48

The coordinator's `execute_with_control` observes an injected source at admission,
after each bounded machine quantum, and again immediately before host dispatch.
The deterministic machine consumes explicit observations; it does not read a clock.
The existing `execute` method remains an explicitly static/snapshot adapter for
callers that intentionally supply deterministic controls.

## Production wiring

Installed CALL, installed batch, inline COBOL and the ProductServer online execution
loop use the live entry point. By default, program deadlines use Unix milliseconds,
anchored once and advanced with `Instant` so wall-clock adjustments do not move an
active router's clock backward. An embedding using logical ticks must bind
`ProgramExecutionControl` at setup with the same clock domain as its deadlines.
Legacy callers that used a small finite deadline with a frozen zero clock must
provide a logical source or a real deadline; a past deadline no longer runs anyway.
`u64::MAX` retains the existing effectively-unbounded convention.

The batch program controller attaches the actual `jes:<job-id>` work identity in a
bounded `jes.work-id` binding. Child invocations inherit it and explicit cancellation.
The program shell observes the durable WorkStore cancellation flag every quantum;
this is the flag the existing ProductServer JobCancel route writes. The new readonly
`get_work` contract does not claim or lease work. Missing rows permit existing offline
batch execution; malformed bindings/read failures fail closed. This is not a new
permission to cancel jobs or change their scheduler state.

## Precedence and bounds

Already-observed unknown outcome wins over any new cancellation, deadline, or
control-source failure. Otherwise observed cancellation wins over deadline, which
wins over ordinary completion/budget exhaustion at that observation boundary.
Backward clock observations and unavailable controls fail closed. Journal lifecycle
ticks follow observations rather than remaining at the admission tick.

A stop is observed no later than the next bounded machine quantum or pre-dispatch
check. This is **not preemption of a synchronous provider already in flight** and
is not a millisecond latency SLA. An in-flight effect is recorded with its observed
outcome before termination; uncertain effects remain uncertain and need reconciliation.
A cancellation after a recorded intent but before dispatch can leave a non-dispatched
intent, so recovery must conservatively inspect intent records, not assume commitment.

Regression tests use scripted observations rather than sleeps: CPU-only programs,
all three COBOL adapters and nested CALL, deadlines crossed during a quantum, scope,
real memory/SQLite work cancellation, unknown-effect precedence, malformed/regressing
control sources, and pre-dispatch stop behavior. No IBM equivalence credit is implied.
