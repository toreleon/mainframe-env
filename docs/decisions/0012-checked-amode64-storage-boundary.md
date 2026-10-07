# ADR-0012: Keep AMODE(64) storage in a checked virtual task arena

Status: **Proposed for cics.application-api development; acceptance gate pending**
Owner: **CICS and execution maintainers**
Scope: **GETMAIN64 and FREEMAIN64 typed IR, host effects, virtual addresses, and checkpoints**
Applies from: **mainframe-env current subsystem contracts**

## Context

IBM CICS TS 6.x limits GETMAIN64 and FREEMAIN64 to non-LE AMODE(64)
assembler callers. Catalog baseline `ibm-cics-ts-6x-2026-08-31` identifies
them as application-command rows `0095` and `0085`. The reviewed GETMAIN64 topic is
`SSJL4D_6.x/reference-applications/commands-api/dfhp4_getmain64.html` at
`sha256:3df90f5932a9b3d23dd9e71b5da382953e67e35c4f6428f5250edf18ac10c0ee`;
the FREEMAIN64 topic is
`SSJL4D_6.x/reference-applications/commands-api/dfhp4_freemain64.html` at
`sha256:0fff6a28a72a46895fe5ca3760229a75a25d018c80053c79e646ffb9e0d93d2f`.
Both exact archive files were hash verified and parsed with the repository
`ibm_docs.py` PlainText parser. Publication bytes remain outside Git and carry
no licensed execution credit.

The COBOL GETMAIN/FREEMAIN path already uses checked virtual pointers and a
task checkpoint. Reusing its pointer identity for the 64-bit commands would
conceal the caller ABI, address width, location selection, key policy, and
cross-task lifetime. Native process pointers would make replay and restart
unsafe. The cics.application-api implementation therefore needs an explicit bounded contract
while assembler source admission and durable shared storage remain unavailable.

## Decision

1. GETMAIN64 and FREEMAIN64 have distinct typed Core-MIR operations and
   append-only tags. The invocation must bind the checked non-LE AMODE(64)
   caller marker and task data key. SET64 and DATAPOINTER use eight-byte slots.
   COBOL source cannot emit either operation.
2. The interpreter owns a separate virtual allocation arena. Above-bar,
   LOC24, and LOC31 identities occupy checked numerical ranges; none is a
   native pointer or an alias of the COBOL GETMAIN allocation identity.
   Monotonic cursors prevent reuse of a freed address. Bounds, task ownership,
   CICS/user key access, frame count, 16-byte rounding, and two eight-byte
   task guard charges are checked before use.
3. The CICS provider owns authorization, audit, source-defined conditions,
   and durable mutation replay. A successful GETMAIN64 host result is an
   allocation specification, not an address. The interpreter compares its
   length, location, key, executable flag, and response state with the pending
   request before creating an address. A successful FREEMAIN64 result must
   echo the pending DATA or DATAPOINTER identity before the interpreter frees
   it. Cancellation and deadline preflight do not dispatch or write replay
   intent.
4. Machine checkpoint v11 introduced allocation bytes, attributes, and
   never-reused cursors. V12 adds DATA-area bindings for FREEMAIN64. Restore
   validates allocation structure and combined 31/64 capacity, rejects forged
   or stale DATA bindings and shared allocations, and preserves historical
   checkpoint reads. Foreign task allocations remain inaccessible. Completion,
   cancellation, timeout, and typed CICS ABEND release private allocations.
5. SHARED remains fail-closed because a task-local checkpoint cannot preserve
   cross-task bytes or ownership. The current arena does not represent
   CICS-maintained LOAD storage, a native executable page, or a compiler for
   assembler source. The compiled selected-route test translates a COBOL
   layout scaffold to distinct typed IR; it does not prove assembler source
   admission. These limits do not receive whole-row semantic or differential
   credit.

## Consequences

- A later HLASM frontend may emit the distinct operations only after proving
  the same caller ABI and operand widths. It cannot infer AMODE(64) authority
  from a COBOL pointer layout or from raw source tokens at runtime.
- Durable SHARED or CICS-maintained storage requires a new cross-task
  persistence and ownership decision. It cannot be enabled by changing only
  a provider option or by treating a task snapshot as global storage.
- The host boundary remains replayable without persisting native addresses.
  A host result with mismatched attributes or a snapshot asserting shared
  storage is rejected before the interpreter exposes a pointer.
- The required `architecture-fast --check` currently lacks a passing receipt
  because unrelated pinned source-closure HTML bodies and a CICS TX DUMP
  supplement are unavailable in the bounded offline caches. Their identities
  and cache-specific availability are recorded in the cics.application-api status. This ADR
  does not waive that gate or request a network refresh.

## Verification

The bounded route is covered by IR tag uniqueness and plan round trips,
interpreter arena and checkpoint tests, Memory/SQLite provider replay tests,
SAF/audit and cancellation/deadline checks, and compiled selected-route tests
for GETMAIN64 and both FREEMAIN64 operand forms. The module-boundary, typed
semantic-boundary, documentation, dependency-policy, and formatting checks
must remain green for the candidate. Required source-review gates retain their
own evidence and may not be relabeled from these focused tests.
