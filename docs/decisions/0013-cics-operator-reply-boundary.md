# ADR-0013: Bind CICS operator replies to durable command positions

Status: **Proposed for v0.9 development; acceptance gate pending**
Owner: **CICS and console maintainers**
Scope: **WRITE OPERATOR, virtual console ingress, and suspended COBOL tasks**
Applies from: **mainframe-env 0.9.0 development**

## Context

The reviewed CICS TS 6.x `WRITE OPERATOR` topic, baseline
`ibm-cics-ts-6x-2026-08-31`, application catalog row `0256`, is
`SSJL4D_6.x/reference-applications/commands-api/dfhp4_writeoperator.html` at
`sha256:a0c239d675dd55bc26f5e08bd4bc90535d1f9917de7525c417ea2ce7b0c07aa4`.
The retained raw HTML matched this digest and was read with the repository
PlainText parser before the semantic change. Publication bytes remain outside
Git and confer no licensed execution credit.

WRITE OPERATOR can return immediately or suspend the issuing task until an
operator reply or finite timeout. The interpreter reissues a suspended COBOL
instruction with a new outer effect key. A message keyed only by that effect
would be duplicated when the task resumes. A key based only on program position
would incorrectly reuse a reply on a later loop iteration.

## Decision

1. The interpreter supplies a checked `run-unit:program-position` identity for
   WRITE OPERATOR. It is not exposed as an IBM command option.
2. The CICS provider atomically persists a strict message record and an active
   command-position pointer before returning Suspended. The pointer survives
   SQLite reopen and binds all reissues until one reply or expiry is consumed.
   Consumption records the new effect identity, so its retry replays the same
   result while a later iteration creates a new message.
3. The virtual console gateway lists validated messages and accepts `R` replies
   only after the existing FACILITY console authorization. The CICS authority
   repeats the SAF check through the audited host boundary before posting a
   reply. Specific-console messages reject a reply from another console.
4. Reply deadlines use durable logical time and work claims. Worker promotion
   atomically expires an unanswered message and wakes only its online task.
   A received reply attempts an immediate wake through the same selected-route
   helper; the due work also wakes a still-pending task if that immediate wake
   failed after the reply was durable.
5. MCEP v2 assigns WRITE OPERATOR operation tag 159, operand tags 645–652,
   option tags 575–577, and output tags 698–699. Existing tags and v1 plan
   decoding remain unchanged. Invalid source options, lengths, routing codes,
   action descriptors, and durable rows fail closed.

## Consequences

The operator message is a virtual console record owned by the current
execution. It does not claim delivery to a physical z/OS console or licensed
IBM differential coverage. The bounded region default for omitted TIMEOUT is
30 seconds; explicit TIMEOUT accepts the reviewed 0–86,400 second range.
The console gateway projects messages longer than 113 bytes into at most ten
69-byte lines, retaining the exact source bytes in the durable record.

## Verification

Focused IR codec, compiler, provider, and selected compiled online tests cover
reply, truncation, timeout wakeup, EIBFN/RESP/RESP2, loop reentry, replay,
SQLite reopen and malformed rows. Descriptor, module-boundary, public API docs,
typed-semantic, docs, formatting, dependency, and diff gates apply to the
candidate. Licensed differential testing remains a separate acceptance step.
