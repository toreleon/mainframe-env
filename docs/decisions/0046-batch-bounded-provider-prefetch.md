# ADR-0046: Bounded provider namespace prefetch

Status: Accepted bounded infrastructure prerequisite; joined admission remains pending.
Owner: **Store and Batch maintainers**
Scope: **MQ-1503.batch-bounded-provider-prefetch, target mq.programming**
Applies from: **mainframe-env current subsystem contracts**

## Decision

`ProviderStateStore::list_provider_state_bounded(namespace, max, max_bytes)`
observes one key-ordered namespace page. Unsupported adapters return
`InvalidTransition`, with no unrestricted-read fallback. Existing get, list and
prefix APIs retain their behavior. Ordinary Batch and older Program-only paths
remain unchanged; only private contained prepared-selection capture selects the
new read.

Before allocation the read validates a nonempty namespace of at most 256 UTF-8
bytes, a positive row limit at most the existing 262145 scan ceiling, and a
positive byte limit at most 64 MiB. The page budget counts namespace UTF-8 bytes
for every row, key UTF-8 bytes and payload bytes, using checked arithmetic.
Keys remain nonempty and at most 1024 bytes; versions are positive signed-SQL
integers; each payload also obeys its backend's configured blob ceiling. This is
a returned-page byte bound, **not** a total transient heap or allocator-overhead
claim. It includes identity bytes; Batch's existing prospective-write and
configuration payload accounting remains separate.

Memory inspects borrowed records, exact map identities and the entire selected
page under its sole state lock before reserving output or cloning payloads.
SQLite opens one read transaction, projects fixed-size byte lengths and type/
version flags for the same ordered limited page, validates its aggregate and
individual bounds, then checks bounded key bytes/UTF-8 before fetching payloads.
Lengths use BLOB casts, not Unicode character counts. Payload fetch and decode
consume that same transaction snapshot. No callback, clock advancement, audit,
event, outbox, epoch or row mutation occurs. Invalid or over-budget pages return
no partial result. Metadata and bounded key buffers still consume memory.

Prepared selection retains max-plus-one membership checking, exact physical/
cache row and version equality, configuration observations, preflight and final
recapture. A namespace page is a structural observation, not a permission,
whole-graph capture or phantom/configuration fence. Future joined Work/Job/absent
Core publication must repeat its owning complete membership and physical claim/
decision-time validation atomically. This read does not authorize that protocol.

## Compatibility and evidence

There is no schema, namespace, index, migration or persisted byte change. The
Memory/SQLite legacy namespace-list bodies move mechanically into private
owning read children, with the same ordering, limits and decode behavior.
Adapters that do not implement the new method cannot use contained prepared
selection; they retain their ordinary legacy operations.

Small physical adversaries test exact byte boundaries, UTF-8 lengths, malformed
rows and error ordering before payload materialization, together with unchanged
physical records, clock and retention epoch. Owned SQLite close/reopen verifies
stored content, not process-crash recovery. These fixtures supply no installed,
native, SAF, JES, Core-join, physical-lease, full26 or licensed MQ credit.
