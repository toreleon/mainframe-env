# ADR-0029: Versioned Db2 core participant binding before mutating integration

Status: **Proposed — prerequisite direction, not capability acceptance**
Owner: **execution-contract, coordinator and Db2 maintainers**
Scope: **0.12 local Db2 core and early INT-1601 participant integration**
Applies from: **mainframe-env 0.12.0 development; implementation remains pending**

## Context

The common implementation prompt requires an owned participant binding before
new mutating integration. Db2 0.12 includes local transactions, but the frozen
early participant v1 names `v0.13-owned-participant-binding` for Db2. Its schema,
generator, runtime validator and binding guard explicitly admit only CICS
capabilities. A dependency-string edit cannot make the existing descriptor
provider-neutral, and a private Db2 coordinator would bypass the required
shared authority.

The existing provider's byte-route tests and parser/type kernels do not supply
the new binding. No proposal, generated descriptor or source review can promote
Db2 from pending to accepted without its mutation/failure/replay/restart proof.

## Proposed decision

Preserve `mainframe-env.transaction-participant@1` and its CICS-only reader,
schema, fixtures, generated projection and guards unchanged. Prepare an explicit
`mainframe-env.transaction-participant@2` through the existing execution-contract
owner and generator, not a Db2-local descriptor registry. Its minimal scope is
the source-backed local Db2 core required in 0.12. Advanced distributed,
prepare/heuristic/in-doubt and mixed-resource closure remain separate 0.13/0.16
obligations; their pending state must not prevent a correctly bounded local
descriptor or be relabeled accepted by that descriptor.

Keep one `ExecutionCoordinator`, canonical effect journal, authorization/audit
authority, store CAS and fenced reconciliation path. Version selection must be
explicit at admission and the selected descriptor must cover the actual
provider, action and trusted execution context. Preserve finite v1 reads and
old CICS behavior; reject unknown versions and cross-version descriptor mixing.
Changing participant metadata is not permission to change old effect, package,
checkpoint, UOW or replay preimages. Version those separately if their actual
shape or meaning changes.

### Context and operation applicability

Full SQL COMMIT, full SQL ROLLBACK, ROLLBACK TO SAVEPOINT and an upstream
transaction-manager completion are distinct operations. A generic
`explicit_syncpoint` bit cannot establish all four permissions. The v2 boundary
must preserve source-backed operation/context applicability and a typed
transaction extent without importing provider ASTs into the shared execution
or compiler contracts. Trusted host/call-chain context is not a caller-selected
string or a claim inferred from a statement name.

The reviewed Db2 13 pins prohibit SQL COMMIT in CICS/IMS and prohibit full SQL
ROLLBACK there, but permit ROLLBACK TO SAVEPOINT. That partial rollback affects
Db2 resources, not every upstream recoverable resource. Upstream completion
continues through the owning transaction manager. Stored-routine calling-chain
and commit-coordinator restrictions also need explicit applicability. Exact
rejection SQLCODE/SQLSTATE and host-manager effects need their own reviewed
diagnostic/context sources; no values are invented by this proposal.

Prepare capability, rollback of uncommitted recoverable work, post-commit
compensation, and unknown post-dispatch outcome remain separate. A durable
intent is not a prepare vote. No universal 2PC, exactly-once behavior, automatic
compensation or automatic redispatch follows from a local accepted binding.
Undeclared contexts fail closed rather than falling through to legacy SQL text
dispatch or borrowing another provider's accepted context.

## Required implementation sequence

1. Declare the early shared v2 schema/compatibility slice under INT-1601 and
   the owned adapter slice under DB2-1204 in the existing status documents.
   Specify exact operations, context ownership, capabilities, outcomes,
   ordering, fencing, retention and read/write versions before mutation.
2. Generate applicable descriptors from one readable shared contract and
   compile its Draft 2020-12 schema. Validate v1/v2 compatibility, malformed
   capabilities, missing actions/contexts, unknown versions and pending entries.
   Generalize only the v2 validators; do not weaken the frozen v1 guard.
3. Bind the accepted Db2 UOW/provider path to the selected shared descriptor.
   Demonstrate deadline/cancellation, canonical intent, typed authorization
   before mutation, audit/result atomicity, CAS/lease fencing, failure and
   exact replay. Unknown outcomes require fenced observation, not redispatch.
4. Prove SQLite/process restart and affected PostgreSQL concurrent-instance,
   restart/backup/restore behavior whenever the durable route changes. Exercise
   forbidden-context no-mutation and preserved CICS v1 behavior. Memory tests
   alone do not establish durable recovery or cross-resource closure.
5. Admit the new mutating core only after those actual gates pass. Preserve
   deferred 0.13/0.16 obligations, source locators and owners in the full
   174-row common/deferred map. Licensed differential remains pending until
   the pinned authorized Db2 environment supplies exact-candidate observations.

This decision prepares the minimum prerequisite; it does not implement a v2
reader, accept Db2 capabilities, change the 174-row denominator or certify
DB2-1204, INT-1601, 0.12 or 0.16.

## Sources and retained contracts

Offline baseline `ibm-db2-for-zos-13-2026-08-13`, product `SSEPEK_13.0.0`:

- SQL0026, `sqlref/src/tpc/db2z_sql_commit.html`, 19582 bytes,
  `61cb2e1f8c0e7c4f33716b4b276707e46b43f8d257cae64bfa54fddc0802de62`:
  local recovery-unit completion, CICS/IMS prohibition and routine/coordinator
  restrictions.
- SQL0119, `sqlref/src/tpc/db2z_sql_rollback.html`, 21891 bytes,
  `087441bdef9562e0e73c8f201dff58f8398cad578163bde9103fb59444dce786`:
  full versus savepoint extent, CICS/IMS applicability and Db2-only partial
  rollback.

Normal `ibm_docs.py search/read` remains TOC-blocked. Retained topic paths were
absent; both archive bodies matched manifest SHA-256 and byte count and were
read with the repository plain-text parser. This is source review, not licensed
execution evidence. No refresh was requested or performed.

Retain ADR-0003/0004/0011, the common early-participant requirements,
`TRANSACTION-PARTICIPANT-V1.md`, execution/durability, canonical effects,
provider-row persistence, security and retention contracts. The v1 generator's
`validate_contract`, execution-contract `TransactionParticipantContract::validate`
and `check_transaction_participant.py` establish why an unversioned capability
or dependency edit is insufficient.
