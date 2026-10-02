# Audited provider publication under a retained intent

Status: **Proposed**
Owner: **store-contract and adapter maintainers**
Scope: **additive Memory/SQLite publication primitive for v0.15 integration**
Applies from: **mainframe-env 0.15.0**

## Decision

Add `ProviderStateStore::publish_provider_states_audited` with an owned
`AuditedProviderPublication`: the exact observed `EffectRecord`, one typed
`AuditRecord`, an observed logical tick and at most 4,096 mixed provider row
mutations. This ceiling shares the existing bounded row/retention product guard.
An empty batch permits audit-only publication. `AuditDecision::Deny` prohibits
every row mutation, including deletes and moves. Unsupported adapters return
`InvalidTransition`; callers must not substitute sequential row and audit calls.

Publication admits only canonical, fully attributed, unrecovered `Intent` with
no result or resolution tick. Creation must be positive, the observed tick must
equal the audit tick, and creation <= observation < recovery-not-before. Ticks
and the recovery boundary must fit the existing positive SQL logical-clock
domain. The observed tick cannot regress the retained clock. Under the same
physical lock/transaction, publication compares the entire retained intent and
requires its execution/run/attempt to match the retained execution, which must
be `Running`. The audit principal must match that execution, and capability,
resource digest including its format, invocation key, sequence and attempt must
match the intent metadata. An expired retained execution lease also rejects.
Recovery claim, resolution, changed intent metadata or a changed execution
attempt between observation and publication cannot publish rows or audit.

The typed principal, SAF decision, capability grants, cancellation and request
digest computation remain service/coordinator responsibilities. Equality with a
retained record is a fence, not proof of authorization or request semantics.
No store transaction spans SAF or provider dispatch.

## Physical implementation and ownership

Memory reuses its locked typed audit map and per-touched-row `Journal`. It saves
only changed provider entries and scalar byte, epoch, clock and audit-ordinal
values. The predicted new audit key is journaled before append, so failure of
the trailing audit epoch update also removes the inserted audit. No whole-store
clone, serde snapshot or MQ-private journal is introduced.

SQLite obtains its writer lock through the existing retention-lock row before
reading the effect and execution rows. It decodes those rows with the existing
core codecs, applies the existing mixed-row loop, and appends the existing audit
codec under `durable-audit-v1` using its `direct:` ordinal format. Audit rows
consume the existing shared row quota and payload budget. Explicit rollback
restores all provider/audit rows, audit ordinal allocation, trigger epochs and
the logical clock if any CAS, encoding, capacity or clock operation fails.
Readers observe the same `AuditSink` ordering and storage formats.

The new publication API rejects `durable-` namespaces and the legacy logical
clock row, preventing its mutation batch from changing core intent, result,
execution, events, outbox or audits. Existing core journal and legacy provider
APIs retain their names and behavior. PostgreSQL uses the additive default
unsupported implementation in this bounded slice; it has no publication or
disposable-environment acceptance claim.

## Compatibility and remaining acceptance

There is no durable migration, changed canonical framing or new codec/schema.
Old binaries can read the same effect, provider and audit bytes; they cannot
invoke the new API. Mixed old writers still require service-level rollout
coordination because this primitive does not make sequential legacy publication
atomic. Providers supply a CAS-protected UOW/replay dependency for at-most-once
publication: disjoint batches sharing an unchanged intent are not globally
deduplicated, and audit-only calls remain append operations like `AuditSink`.

The coordinator still owns core result/lifecycle/outbox completion. Providers
must compose their resource, UOW and replay rows, retain request identity and
known result, and reconcile post-publication uncertainty from that authority.
MQ dispatch/SAF composition, receipt ownership, cancellation observations,
participant acceptance, retention/restore composition and PostgreSQL integration
remain manager-owned obligations. Store tests grant no MQ per-call, licensed or
accepted-participant credit. This infrastructure slice requires no unrelated
IBM publication projection.
