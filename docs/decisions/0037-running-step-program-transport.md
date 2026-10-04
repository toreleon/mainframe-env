# ADR-0037: Running-step Program context transport

Status: Accepted bounded transport prerequisite; host activation remains pending.
Owner: **Batch and host-contract maintainers**
Scope: **MQ-1503.running-step-program-transport, target 0.15.0**
Applies from: **mainframe-env 0.15.0 development**

## Context

The Batch runner publishes actual Running job and step state, validates the
selected Program registration and builds the original Program request. Its
ordinary direct scoped invocation persists a typed audit, but does not create a
coordinator-owned original core Intent. Invocation binding text, equal Job rows
or public IDs therefore cannot establish independent JES admission or core
ownership.

## Decision

`BatchService::run_claimed_with_program_dispatch` is an explicit opt-in to a
higher-ranked synchronous callback at the existing ProgramService dispatch site.
The ordinary scheduler, Running CAS/checkpoint, registration and request builder
remain shared with `run_claimed`. Other handlers retain their existing paths.
Only the private runner site constructs a non-Clone, non-Serde
`RunningStepAdmission` borrowed from a private volatile owner. No Batch mutex is
held across the callback.

`retain_for_host` returns a non-Clone, non-Serde `RunningStepView` referencing the
same owner's irrevocable revocation state. Its read-only observations bind the
original Invocation, exact published Job row, actual physical store and validated
registration. `check_live` checks revocation before and after observing the same
physical row. This is a current observation, not a decision-time lease fence.
It never renews a lease or creates an effect, core row, handle or permission.

The owner revokes on callback success, failure, panic and unwind before output
decoding, step retirement or cleanup. Owner Drop only revokes; retained-view Drop
performs no host action. A restored row cannot restore volatile ownership.
Malformed callback sequence and callback panic retain uncertainty rather than
reporting known success. Callers must apply scoped/coordinator admission
independently; the callback itself is not an authorization bypass API.

`HostProvider::invoke_program_context` defaults to Unsupported and never falls
back to ordinary `invoke`. `ScopedHostService::invoke_program_context` transports
borrowed `&(dyn Any + Send + Sync)` only for Program requests. Both entry points
share one preflight, frozen provider selection, result-validation and typed audit
implementation. Non-Program context requests refuse before provider dispatch.
Any is neutral transport, not authority: a future closed server receiver must
downcast and validate its own genuine joined runner/claim proof. A public trait
implementation or equal IDs do not constitute that proof.

No original Invocation parent, grants, limits, generations, sequence, effect key,
Program payload, canonical preimage or storage schema changes. The existing
`jes.work-id` compatibility binding remains observational text, not a permit.

## Remaining ownership and acceptance

The server's genuine claimed-work owner and revocation/join transport, actual
coordinator-owned Intent dispatch, compiled step/root terminal policy and
decision-time physical Work/Job lease fence remain separate prerequisites.
The store must not derive semantic Batch authority from guessed JSON. A stale
prepublication tick cannot close publication-lock expiry races.

This boundary activates no DefaultContext, environment user/accounting, JES job
origin or GMT source. Direct non-JES DefaultContext stays refused. NoContext must
continue to sample neither GMT nor batch context. No compiled installed, native,
root-pending, SAF, participant, licensed or full-MQI acceptance follows from
transport tests. All nonlicensed parent acceptance obligations remain required.
