# ADR-0043: Batch contained all-effect loan

Status: Accepted bounded transport prerequisite; enclosing owner activation pending.
Owner: **Batch maintainers**
Scope: **MQ-1503.batch-contained-all-effect-loan, target mq.programming**
Applies from: **mainframe-env current subsystem contracts**

## Decision

`BatchService::run_claimed_with_all_effect_dispatch` is an explicit privileged
Rust host-composition transport. It lends every actual host occurrence from the
existing Batch builders through `BatchEffectOccurrence`, not just Program. The
observation has private construction and no Clone/Serde or row/ID factory. It
retains the actual PRE-binding Invocation and owned request/sequence/key/deadline.
Program alone also retains the exact POST `jes.work-id` view and genuine borrowed
RunningStepAdmission. Other effects carry no fabricated Program admission.

The callback must own actual scoped preflight/result/audit and journal settlement.
Batch does not invoke its ordinary host or persist a second scoped audit on this
route. Missing callback refuses before selection or observation, with no fallback.
Plain run and older Program-only transport retain their old builders, bytes,
audits, error/disposition behavior and callback timing. New explicit transport
shares the existing contained scheduler/drive/stop/exit and sequential Selected/
Running writes; it supplies no atomic joined admission or coordinator cursor.

Actual scope checks surround callbacks. Callback panic or raw sequence mismatch
retains Unknown; returned Malformed and existing control/CAS/uncertainty errors
poison the contained run before a later effect or disposition. Try-borrow refuses
callback reentry rather than converting a RefCell panic into success. All external
callback/observer calls occur outside Batch locks. The genuine Program StepOwner
is revoked before output decode/next action; retained views cannot extend it.
Known success may retire through the existing owning path. Drop only revokes,
never dispatches cleanup or settles durable work.

## Limits and remaining composition

These callbacks expose structural current observations, not Core/JES/SAF or
physical claim/clock permission. Equal rows, positive RC and public callback types
cannot mint enclosing ownership. Existing Batch sequences are deliberately not
renumbered or claimed to form one coordinator cursor. The future closed actual
coordinator owner must supply its actual journal, sequence allocator, scoped
audits and claim/selection join without another Machine or fabricated Intent.

No Any/TLS/cache, native/JES context, root/terminal/UOW decision, physical Work
expiry fence, source/default/GMT profile or external subsystem semantics are
activated. Memory/owned SQLite fixture tests prove this transport and containment
only, not compiled installed/Core-original/JES/native acceptance. All26, recovery,
participants, IR/CardDemo and other parent acceptance remain required. Only the
licensed IBM oracle is human-skipped 0/26; source presence earns zero execution.
