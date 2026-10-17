# ADR 0077: Local consumption of completed plain CICS READ results

Status: Proposed; implementation and owning verification pending.
Owner: Execution runtime and CICS provider maintainers.
Scope: Local delivery of already completed configured-file plain CICS READ results under original core, task, store and SAF fences; no browse, UPDATE or global acceptance.
Applies from: mainframe-env unreleased proposal; adoption and owning verification remain pending.

## Context

The ordinary CICS read journal refuses a duplicate read with UnknownOutcome because a durable reply does not reconstruct a volatile browse cursor. The installed online machine may instead restart before its next checkpoint after a successful configured-file plain READ and its core Completed effect are both durable. Returning Unknown for that completed occurrence prevents the original machine from consuming its already committed result.

The distinction is one completed plain READ under the original current owner. It does not include UPDATE, browse, cursor, generic or alternate-address requests. It does not permit a second data read. The original unknown-outcome rule remains on ordinary duplicate dispatch.

## Decision

The coordinator's original Completed branch offers a borrowed opaque observation to an opt-in dispatch hook. The scoped host retains the actual audited result and invokes the separate provider replay port once. Absent, repeated, failed or panicked hooks refuse; no hook supplies a substitute result.

The CICS receiver uses a privately minted same-service/store observation, reacquires the actual command lease, verifies the current Running owner and installed frame, and repeats current command and Dataset SAF checks. It requires the complete original successful receipt, same session and configured file alias, no volatile browse/update/undo state and no active unit of work. The existing atomic ProviderReplayAssertion fences the exact core effect, execution, receipt, original alias and session, current file status and absent unit-of-work/undo rows. Loss of that fence returns UnknownOutcome. Ordinary host invocation and security checks are allowed; Dataset data retrieval is not performed by this delivery port.

Pinned COACTVWC and COSGN00C spell the logical file as DATASET. The existing typed operand mapping preserves DATASET and FILE independently; this path selects the actual pinned DATASET spelling and still requires its configured CICS file-alias row. A direct physical Dataset name without that alias never gains a V4 receipt or delivery authority.

The additive MECER004 envelope binds the original alias CAS identity and bytes in the existing CICS receipt namespace. Earlier MECER001, MECER002 and MECER003 decoding and no-alias encoding remain intact. Existing current READ observers require exactly V4 only when the independent pinned request and audit digests identify those new eligible reads; other browse, mutation and legacy observations retain exactly V3. There is no general V3-or-V4 acceptance union. Unknown, malformed, pending, wrong-owner, stale-alias, cancellation/deadline and unsupported backend cases retain refusal. Memory and SQLite have the existing atomic primitive. PostgreSQL parity requires its corresponding bounded provider-read implementation and genuine backend verification; the local draft earns no PostgreSQL credit.

## Owning verification

A separate ignored first-party producer installs the pinned source application, authenticates through the real HTTP surface, selects account view and enters account 00000000050. A test-only exact store forwarder exits the first process with code 77 only after the real coordinator transaction has committed the third successful READ result, before result delivery and the next checkpoint. It creates no core result or receipt. The second positively reaped process opens the same SQLite/artifact store without reinstalling or rebuilding and resumes through authenticated HTTP.

Independent pinned record oracles compare all 50, 300 and 500 bytes and source-derived BMS field widths. The producer checks receipt/core identity, denies unauthorized and wrong-CSRF access, measures Dataset host decisions and backing provider-state fetches during resume, and checks disconnect and graceful shutdown. These counts are not operating-system disk-read or aggregate-RSS measurements. Dataset caches records; a successful first dispatch's retained full reply is separate from the backing-store fetch count.

The cut owner remains Running; this producer does not claim the separate original J09 “process restart while suspended” predicate. It contributes only its actual source-bound observations when executed. Original 20 journeys, 114 observations, 105 acceptances, J09/J19/J20 requirements and financial oracles are unchanged. Existing browse and Suspended terminal producers remain separate and retain their own producing-source lineage.

## Authority and consequences

The verified offline CICS READ command body requires FILE lookup and preserves full-key equality, INTO/LENGTH and NOTFND behavior. Omitting UPDATE is a plain read. This decision changes local delivery of a completed result, not IBM cursor, hold or unit-of-work behavior. The retained IBM body is reference-only and is not reproduced or granted licensed/vendor acceptance credit.

Affected Rust consumers require diff-scoped compatibility, failure, restart and backend checks under existing repository policy. The owning producer retains the existing default features and process/resource limits. The external proposal has no formatter, compiler, runtime, publication, backend or global acceptance result until the Root executor binds and produces those actual facts.
