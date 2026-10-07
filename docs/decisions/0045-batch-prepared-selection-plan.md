# ADR-0045: Batch prepared-selection observation

Status: Accepted bounded prerequisite; atomic joined admission remains pending.
Owner: **Batch maintainers**
Scope: **MQ-1503.batch-prepared-selection-plan, target mq.programming**
Applies from: **mainframe-env current subsystem contracts**

## Decision

The fresh contained runner privately prepares exact Queued, Selected and Running
Job records from its actual submitted Job, existing semantic validators and sole
pure selection-edge builders. The ordinary path uses those same builders at its
old points: it builds Running after the known Selected write. The contained path
retains the same sequential Selected CAS then Running CAS, events, attempts,
serialized records, original JESJOBS request and standalone scoped audit. Neither
path is an atomic Work/Job/Core join. Older Program-only transport is unchanged.

The non-Clone, non-Serde PreparedSelection borrows the actual Batch service and
retains its physical adapter identity, exact original Invocation, current Job and
expected writes, complete bounded Job namespace and scheduler/topology semantic
configuration and physical rows where present. It has no public constructor,
adoption/drive method, commit flag or authority from a recovered Running row.
Private read-only getters expose structural dependencies and expected writes.
Planning creates no run owner, step admission, exit, checkpoint or Core record.

The finite preparation profile reserves two configuration observations within
4096 records: at most min(max_jobs,4094) Job rows, requested with a max-plus-one
scan. Before cloning a selected Job it checks complete ordered unique keys,
positive SQL-compatible versions, physical payload/count bounds and exact
physical/cache membership. Existing Job, event, program registration, scheduler,
topology, route, owner and active-selection validators remain authoritative.
The 64 MiB budget includes captured Job payloads, semantic configuration bytes
and both prospective encoded writes; overflow and overbudget refuse without
selection. This is a smaller contained profile, not a raised legacy exemption.

JESJOBS Execute uses the existing sequence-2 builder after fresh eligibility and
capture checks. Control/provider callbacks are outside Batch/configuration locks.
After preflight, and after the last prepublication control callback, the plan
recaptures and compares the exact namespace/configuration/current semantic state.
Preflight retains its existing standalone audit, including its retention-epoch
advance; that audit is not physically claim-fenced or a Core-original admission.
Failed preflight or changed observations do not publish Selected/Running.

## Limits and future owner

Capture uses separate bounded physical snapshot reads and semantic locks. It is
not an atomic graph capture, a phantom fence, a current Work lease/time proof or
backend publication permission. Version-zero scheduler/topology configurations
retain actual in-memory observations and absent rows, not manufactured CAS rows.
Configuration changes are rechecked; future semantic freeze/fencing is unresolved.

The future joined Store operation must repeat complete membership/phantom and
configuration checks, real current claim/backend decision time and absent Core
under one physical transaction. Its actual private host/coordinator callstack
alone owns the known acknowledgement and first journal cursor. A positive returned
DTO, matching rows/IDs, prepared plan or transport callback cannot supply that
ownership. Existing effect sequences are not renumbered into a fabricated cursor.

Memory and exclusively owned SQLite fixtures prove bounded planning, preflight
and sequential containment only. Orderly reopen is not process crash; fixture
callbacks are not installed/JES/SAF/Core/native admission. No controller, scheduled
root/terminal, physical clock, context/GMT, UOW or recovery policy is activated.
All26 and other nonlicensed parent acceptance remain required; only the licensed
IBM oracle is human-skipped 0/26. Infrastructure reference review grants no credit.
