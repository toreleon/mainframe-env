# MQ historical handle observation

Status: **Proposed**
Owner: **MQ contract and provider maintainers**
Scope: **typed result storage and readonly existing-handle lookup, not service acceptance**
Applies from: **mainframe-env current subsystem contracts**

## Decision

Separate a handle's exact historical canonical identity from permission to use
the registry. Issued tokens retain their original registry, slot, generation,
epoch and role. A private irreversible historical disposition distinguishes
decoded tokens from live tokens; ordinary equality includes it, while existing
canonical parts deliberately retain their exact old bytes. Every registry
entry/entry_mut path refuses historical values before looking up a slot, so
connection validation, child access, in-use transitions, release, disconnect and
lifetime observation cannot treat coincident numeric identities as permission.

Only the bounded fixed-field `MqHandleObservation` projection supports Serde.
It denies duplicate, unknown and missing fields, invalid roles, zero identities
and slots outside the existing finite ceiling. Executable opaque tokens have no
Serde or public numeric constructor. Capture is readonly identity observation,
not a statement that the actor was authorized. Decoding constructs historical
tokens only and never allocates, connects, opens, adopts or changes a registry.

The existing private typed result codec stores issued Connected, Opened with
exact nullable dynamic metadata, MessageHandle and both Subscribed roles through
this one observation authority. It recomputes the full existing host canonical
result digest under the original trusted explicit MQI limits. Non-handle storage
bytes and canonical schemas are unchanged; successful historical decode retains
the actual result shape and outcome rather than reducing it to pending/status.

Symbolic Default and Unassociated cannot carry a historical disposition in the
unchanged Hconn enum. Historical connection reconstruction of those values
remains explicitly unsupported: decoding must never acquire the current CICS
task's default connection. Child handles retain their own historical disposition
even when their independently admitted live parent is Default or Unassociated.

## Checked resolution and caller obligations

Readonly registry resolution returns only an already-existing live entry under
exact registry/slot/generation/epoch, exact full entry owner, role, independently
admitted live connection and existing applicability/in-use checks. It performs
no allocation or resurrection. Exact owner matching is intentionally stronger
than ordinary shared-thread use permission. A default entry cannot be relabelled
as an issued connection. Foreign registries, retired slots, reused generations
and advanced epochs fail closed. Resolution is not a token-disposition mutator.

Before calling it, the service must independently validate the exact retained
receipt/core occurrence, canonical request/result, original actor/run and current
admitted frame. The same locked registry and trusted connection/role must be used.
Neither matching counters nor observation/digest/owner bytes supply this proof.
The historical result remains the replay identity; an ABI alias requires separate
service/ABI authority and must never be mistaken for the registry token or UOW.

Cold dispatch exposure still requires the existing service recovery authority to
advance its durably retained registry incarnation/epoch. Process-local counters
do not establish cross-process freshness. No owner directory, UOW identity,
replay namespace, journal, store or queue authority is introduced. This distinction
extends [the shared handle kernel](0028-mq-shared-handle-kernel.md) and preserves
[the host lifecycle boundary](0030-mq-host-lifecycle-directory.md).

## Sources and acceptance

Offline hash-verified baseline `ibm-mq-9.4-mqi-2026-08-31`, rows 0008 MQCONN,
0009 MQCONNX, 0012 MQDISC, 0019 MQOPEN, 0010 MQCRTMH and 0025 MQSUB: topics
`q101760_`, `q101770_`, `q101800_`, `q101870_`, `q101780_`, `q101930_` below
`SSFKSJ_9.4.0/refdev/`. The source-bound lifetime/role contracts are unchanged;
historical disposition is a product storage/authority distinction, not IBM wire.
Source review grants zero execution credit. Tests prove pure codec identity and
registry safety, not actual retained receipt authority or audited publication.
Service/host/SAF, lifecycle/recovery, durable UOW, retention, participant and
CardDemo integration remain required. Only the licensed IBM MQ differential
oracle gate is human-skipped, with zero licensed credit.
