# ADR 0043: Checked provider read publication

Status: Implemented store prerequisite; provider/native inquiry remains pending.
Owner: store contract and Memory/SQLite adapter maintainers.
Scope: atomic provider read assertions, receipt/audit publication and replay fences.
Applies from: mainframe-env mq.programming

## Decision

Use `ProviderStateStore::publish_provider_read_audited` for an original read
observation that must become a retained receipt and one typed provider audit.
Its non-Serde `CheckedProviderReadPublication` wraps the existing
`AuditedProviderPublication`, the full observed current `ExecutionRecord` and
unique `TerminalRowDependency` Exact/Absent observations. The dependency enum is
reused only as a structural read assertion; no Closing/terminal lifecycle is
invoked. None of these caller observations is host, SAF, root or JES permission.

A successful publication has exactly one insert-only version1 Put receipt and
its matching Absent dependency, plus the existing attributed Success audit.
An audit-only Deny has no mutations or success receipt. Other shapes refuse.
Exact dependencies cannot be mutated. No queue/catalog/control/unit/registry
version advances to simulate a read fence; the store never parses MQ results.
Existing generic system-identity exclusions apply to reads as well as writes.

Under the same Memory State mutex or SQLite writer transaction, the store reads
and compares the entire original canonical unresolved Intent and current Running
execution. The old exact audit capability/resource/invocation/principal/attempt
validator, positive logical observation, finite recovery deadline and execution
lease remain mandatory. Recovery-owned, unknown, failed or completed intents
cannot publish. All supplied dependencies are compared by full identity,
version and bytes, or actual absence, before mutation.

Private root attribution is derived from the actual actor/run indexes and
retained root document after the original/execution comparison. The document
must be Open, live and contain that exact execution/artifact/selector/attempt.
Each indexed actor endpoint needs a genuine matching namespace or row index.
This new API conservatively refuses simultaneous namespace and row indexes on
one endpoint even when both identify the same root. Older root APIs are unchanged.
Missing, corrupt, foreign or incompatible indexes refuse. A legacy actor without
actor/run indexes is allowed only when all dependency and receipt endpoints are
unindexed. No new
CALL enrollment, claim, current-frame grant or dispatch permission is minted.

## Nonpublishing replay assertion

`ProviderStateStore::assert_provider_replay` accepts a non-Serde
`ProviderReplayAssertion`: full original effect, exact current Running execution,
logical observation and unique dependencies, including an Exact dependency
selected by its explicit receipt identity. The receipt bytes remain opaque.
An unclaimed canonical Intent is bounded by its existing recovery deadline.
A canonical Completed effect instead uses the existing terminal/effect validator,
real retained result digest and positive resolution metadata. It is not rewritten
into an Intent; recovery-claimed, Failed, Unknown or legacy records refuse.
Live execution lease, logical floor, root deadline and endpoint ownership remain
required for both states.

This method returns only `()`. It performs no provider/audit/ordinal/epoch/clock
writes. SQLite obtains the existing writer lock by its epoch-preserving lock-row
update; no stored counter changes. A future provider decodes and preflights the
original retained result first, invokes this final physical assertion last, and
returns that result without recomputing it from a changed catalog. A refusal is
not a new success audit. Replay denial/failure audit ownership is a separate
prerequisite; this operation cannot republish a Completed effect.

## Bounds and physical implementation

Preflight permits at most 4096 combined requested read dependencies, physical
mutation endpoints and emitted typed audits. Success allows 4094 dependencies
plus one receipt Put and one audit; audit-only Deny allows 4095 dependencies plus
one audit. Nonpublishing replay allows 4096 dependencies, with no mutation or
audit. Internal validation lookups do not grant caller mutation permissions.
The unchanged byte bound permits at most 64MiB
combined captured identity/payload bytes, with narrower configured blob limits.
Duplicate Exact/Absent identities, collisions, empty/oversized names, `durable-`
identities and the legacy logical-clock identity refuse. Count checks precede
identity-set allocation. Checked arithmetic rejects aggregate overflow. Typed
core strings are bounded before codec allocation. Memory conservatively charges
JSON's finite escape bound plus fixed framing before encoding current core
records. SQLite charges payload lengths before fetching any current core,
provider, root or index blob and uses the existing bounded reader before decode.
The new path also checks existing audit ordinal key counts, maximum length and
aggregate bytes before the unchanged audit kernel loads its bounded key scan.
The aggregate also includes captured current rows/indexes. This can conservatively
refuse a very large observation; it never silently batches or trims it.

Memory reuses the touched-entry Journal, existing provider mutation loop and typed
audit map. SQLite reuses the existing transaction finish, row mutation loop,
audit codec/direct ordinal and retention-lock authority. Only the old publisher's
already-validated mutation/audit tail is extracted for reuse. No whole-State
clone, second table/journal, nested public transaction or callback fallback is
introduced. Quota, CAS, encoding, audit ordinal, epoch and clock failures roll
back receipt/audit and all counters. No backend lock spans provider/SAF dispatch.
Lost acknowledgement after possible persistence remains caller uncertainty,
never automatic store retry, cleanup or coordinator completion.

## Compatibility and proof boundary

Old APIs, record codecs, root/index/control version1 bytes and mutation semantics
remain unchanged. The additive default trait methods refuse unsupported adapters;
`SqliteStateStore`, the actual `JournalStore`/`ProviderStateStore` adapter, routes
to its transaction implementation; there is no exported DurableStore wrapper.
`PostgresStateStore` retains the default refusal for these additive methods.
There is no schema or database migration.
Two parent module ratchets decrease after routing extraction; no ceiling rises.

Owning fixtures cover Memory and owned-file SQLite, original Intent/Completed
replay, exact rows/counters, conflicts, bounds, root indexes/phases, faults and
orderly close/reopen. They establish a store contract, not host grants, process
crash recovery, native MQINQ, installed ABI, accepted participant or official
26-call coverage. Provider INQUIRE admission, real SAF, current live frame/handles,
receipt decoding, replay denial auditing, native table writeback and parent
recovery/IR/CardDemo integration remain required. Only the licensed IBM MQ oracle
is human-skipped with zero credit; this prerequisite does not waive other gates.
