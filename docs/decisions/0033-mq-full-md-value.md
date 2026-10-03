# Complete MQMD value primitive

Status: **Proposed**
Owner: **MQ host contract maintainers**
Scope: **complete descriptor/message values and codecs, not executable message policy**
Applies from: **mainframe-env 0.15.0**

## Decision

Introduce `MqMdValue::V1` and `V2` alongside the unchanged partial
`MqMessageDescriptor`. The variants represent exactly the selected 324-byte
MQMD1 and 364-byte MQMD2 declarations. Version is the discriminant. Both retain
StrucId, Report, MsgType, Expiry, Feedback, Encoding, CodedCharSetId, Format,
Priority, Persistence, MsgId, CorrelId, BackoutCount, ReplyToQ, ReplyToQMgr,
UserIdentifier, AccountingToken, ApplIdentityData, PutApplType, PutApplName,
PutDate, PutTime and ApplOriginData. Only version 2 has GroupId, MsgSeqNumber,
Offset, MsgFlags and OriginalLength. Later versions are unsupported; version 1
does not fabricate extension fields or source initialization values.

MQLONG observations are signed fixed-width `i32`, including unrecognized values
and sentinels. MQCHAR and MQBYTE fields are exact fixed-width byte arrays. Blanks,
nulls and arbitrary bytes are retained without text decoding or normalization.
The explicit trusted structure character profile is either the existing
ASCII-compatible profile or the owned CP037 profile. This profile is part of the
value identity and never inferred from the descriptor's body Encoding/CCSID.
Structure byte order is the raw capture's independent trusted input; the value
contains decoded signed numbers, so equal observations across big/little-endian
captures have equal value bytes. Source capacity, physical byte order and suffix
remain available on the separate raw capture. A value neither hides nor supplies
pointer, buffer, actor or registry authority.

Raw projection uses the sole generated layout's names, types, widths and
identifiers. No offsets, defaults or second declaration authority are introduced.
The representation check validates structure identity under the declared profile;
Rust types enforce widths and the version vocabulary. It is not permission for
numeric options, context, identifiers, message generation, flags, UOW or per-call
policy. Legal combinations and unresolved per-call semantics remain pending at
their future admission boundary. Exact representation alone never makes an
arbitrary descriptor executable.

## Frozen value codec

The new schema is `mainframe-env.mq-md-value@1`, with canonical domain
`mainframe-env.mq-md-value@1` followed by one zero byte. It uses the existing MQI
encoding authority and canonical primitives, with an explicit schema text before
the typed value. Variant, object and field identities are fixed; fields are in
ASCII name order, counts and lengths use the existing fixed little-endian u64
encoding, and MQLONG values use its signed little-endian i32 tag. Version 2's
extension is a separate exact five-field object. Character-profile tags remain
explicit. No Serde, maps, debug text or native integer widths are involved.

Encoding and decoding require an explicit caller ceiling, capped by the new
2,048-byte product ceiling. Decoding checks input size before parsing and uses
fixed arrays without dynamic field allocation. Unknown schema/profile/version,
field/type/tag/count/order/width, duplicates, missing data, truncation, oversized
lengths and trailing bytes fail closed. Independent fixtures freeze complete
version-one and version-two preimages and SHA-256 digests; every field and the
character profile affects the digest.

This value codec composes with the one effect/replay authority below. It does not
define a checkpoint or delivery storage schema. Every old DTO literal and
canonical domain/preimage remains unchanged.
`MqRawCapture::try_typed_descriptor` continues returning
`DescriptorRepresentationPending`; silently reducing a complete MQMD to the old
partial DTO is forbidden. Durable delivery evolution requires a separate sealed
composition and compatibility decision before full-message execution.

## Additive complete-message boundary

`MqFullMessage` retains `MqMdValue`, exact body bytes and the existing ordered
typed property vocabulary. `FullPut` and `FullPutOne` carry `MqMqiFullPut`;
`FullGet` carries `MqMqiFullGet`. They map to the original MQPUT, MQPUT1 and MQGET
call identities and source positions. No old `MqMessage`, descriptor, Put or Get
field/literal changes. The sole MQI canonical encoder emits distinct new variant
tags and composes the owned MD encoder directly within the unchanged host @1
domains. Fixed arrays and signed numbers are not reduced to the partial DTO.

Complete representation is not accepted report/flag/context/option/identifier
policy. Existing typed controls are reused, including finite wait/capacity,
truncation and UOW intent. Full requests remain pending in host review and private
provider admission; the selected route returns Unsupported before queue
transition. No generic success or live-handle construction is introduced.

`FullPut` output retains the exact returned MD. `FullGot` retains a complete
message observation plus required optional `data_length` and `cursor` fields.
Complete GET requires body length = DataLength = complete disposition length.
Truncated GET retains the full MD and copied prefix, copied < required and
DataLength = required. Both lengths are independently bounded; original capacity,
mode/truncation and reviewed status must agree. Source MQGET fills the descriptor
also on rejected truncation. No-message/wait-expired/unknown have no message,
DataLength or cursor. Undefined error-buffer contents are not synthesized.
Conversion/property size-report forms remain pending; these controls do not
pretend to implement MQGMO conversion, match flags or arbitrary numeric options.
Input/output descriptor version and structure character profile must agree.

Shared message limits bound body, IDs/format, property count/type/width/name/
aggregate bytes; HostLimits add field/record/state budgets. Body and DataLength
are nonnegative MQLONG-representable; present cursor/local unit storage identities
are positive SQL-compatible observations, never ownership proof. Explicit
character profile stays independent of body Encoding/CCSID. Historical handle
observations retain identity but fail live registry access.

## Existing result replay codec evolution

The same private `mqi_replay` codec emits EXACT storage@1 for every old result
shape. Only FullPut/FullGot outputs use
`mainframe-env.mq-mqi-result-storage@2`, with distinct output tags and `md_value`
containing the bounded owned MD value codec. FullGot uses the shared property
projection directly; it never goes through the partial SnapshotMessage.
Storage@1 refuses new full outputs; storage@2 refuses old shapes. Older readers
fail closed at @2. No namespace, journal, dispatcher or parallel schema owner
is added; the manager must update retained receipt selection before public use.

Both versions require strict fields (including optional fields), reject duplicate
or unknown fields, preflight finite bytes/counts before typed allocation, validate
the public result and recompute the FULL `HostResult::MqMqi` canonical digest.
Unknown/trailing/truncated/reordered/wrong-width MD payloads, inconsistent GET
lengths and digest mutations fail closed. @2 is a private storage compatibility
decision, not IBM wire, status calculation, delivery-row migration or new
capability readiness. No old row is rewritten or automatically migrated.

## Sources and acceptance

Offline hash-verified original baseline `ibm-mq-9.4-mqi-2026-08-31` rows 0015
MQGET (`SSFKSJ_9.4.0/refdev/q101830_.html`), 0020 MQPUT (`q101880_`) and 0021
MQPUT1 (`q101890_`) establish the call context. Supplemental baseline
`ibm-mq-9.4-programming-supplements-2026-09-12` topics `q097390_` (complete MQMD
declarations, SHA-256 `57354bc889449b82a1d3324b3b4f777fdc2dbdf7919d04784115c7fbbd87446d`),
`q097395_` (field details) and `q091870_` (structure/version constants) accompany
the existing generated layout authority. Point-layout baseline
`ibm-mq-9.4-point-layout-sources-2026-09-12` topics `q093580_`, `q093600_`,
`q093630_` and `q093780_` cover elementary types, COBOL widths and independent
structure encoding. Expected retained topic paths were checked before verifying
the local SHA-addressed archive; no source refresh or publication body is in Git.

Source review grants zero execution or licensed credit. Tests prove complete
value/capture/codec behavior and old canonical compatibility. Real full-message
PUT/GET, SAF/context/generation admission, durable storage/replay, participant,
full-26 and CardDemo acceptance remain manager-owned and incomplete. Full-message
request/result/replay tests establish this value boundary only; actual selected
PUT/GET, full-MD durable delivery/restart, generated IDs and property/conversion
policy still require manager composition. The global public API docs ratchet is
unwaived; added API items are documented. Only the
licensed IBM differential oracle is human-skipped, with zero licensed credit.

## Same-kernel complete delivery storage

`MQ-1503.full-message-delivery` tags each existing entry as Partial or Complete.
One queue entry list owns its homogeneous profile: Partial, or Complete with
exact descriptor version (1/2) and structure character profile. Queues of both
profiles coexist in the same kernel/catalog. Wrong routes or full version/
characters refuse before IDs, UOWs, cursors or expiry change. There is no
incompatible-head skipping, narrowing, fabricated MD or second queue inventory.

Complete storage retains the sole MD value codec, exact body and ordered shared
properties. Only explicit persistence 0/1 (`q092170_`, MQPER_NOT_PERSISTENT/
MQPER_PERSISTENT) and unlimited expiry -1 (`q097390_`, MQEI_UNLIMITED) are stored.
Unknown/default persistence and other expiry observations fail pending. Other
signed/fixed observations remain exact; these private storage primitives do not
admit flags, context, conversion, generated IDs/GMT, segmentation or backout-count
policy. Public FullPut/FullPutOne/FullGet execution still returns Unsupported.

Cold `mq-delivery@2`, live `mq-delivery-live@2` and `mq-delivery-rows@2` are
explicit additive schema selections within the same owning codecs/namespaces.
Every profile, tag, nullable field and payload field is required. The @2 DTO
converts to the same checkpoint cross-reference/entry validator; its strict
JSON preflight precedes typed allocation. MD bytes use the owned bounded value
decoder and properties reuse the original typed property projection. SQL-range
IDs/counters and queued plus pending payload overhead are bounded. Prospective
JSON expansion and rich-plus-retained snapshot quotas are checked before
adoption. Every @1 partial byte/default/digest remains exact. Old public @1
readers refuse @2; private stored reads select the actual schema. No accepted @1
export drops complete fields. Cold @2 retains only persistent queued/backed-out
GET entries, discards staged puts/nonpersistent messages/pending/cursors and
retains prior decisions. Live @2 preserves pending (including empty units),
nonpersistent entries and browse positions. Neither changes MD/backout/time or
resurrects volatile authority.

The explicit rich upgrade builds one composable batch; open never upgrades.
Its first profile is pre-activation globally quiescent: pending units and cursors
must be absent, and any selected control/runtime is refused. An owner-approved
retirement/quiescence seam is still required for previously activated services;
this module does not parse/retire the selected owner directory. Populated partial
queues stay partial; changing a queue profile requires it drained. The unchanged
rich marker @2 shape/catalog/generation/fence is jointly CAS-bound with metadata
and exact catalog bytes. Selected activation/old writers must use that same
marker/meta publication, so stale @1 versus upgrade has one winner. Ordinary
deltas preserve the loaded schema/profile; an explicit plan alone may change it.
Unaffected final/cursor/replay/core/CALL/audit records retain bytes and versions.
Plan limits remain 1024 mutations/64 MiB, delivery 64 MiB, combined reader 128 MiB;
no silent split or retry. Adoption requires the entire known atomic commit.

The planner carries no operator/core/SAF authorization. The manager owns drained
deployment, verified backup/rollback and an accepted new-reader selection; old
binaries cannot resume @2, and downgrade never strips fields or relabels schema.
Memory/owned SQLite private storage and audited-transaction tests are storage
evidence only. Actual selected full PUT/GET, trusted behavioral/context producer,
retention description, SAF, participant, full-26 and CardDemo acceptance remain
incomplete; unknown outcomes never authorize rollback or redispatch.

## Explicit live z/OS BackoutCount policy

The separate private `backout_complete_zos` candidate applies the reviewed
`q097395_` BackoutCount rule to complete payloads actually removed by syncpoint
GET into the exact pending unit. It increments once, saturates at 255 on z/OS,
and rejects signed observations outside 0..255 without changing any queue, unit,
counter or finalization. Browse/rejected truncation and staged PUTs do not count;
all other complete fields, body and properties remain exact. The existing
candidate restores GETs/discards PUTs and checks finalization/encoded quotas
before adoption. Repeated/unknown/committed decisions preserve existing behavior.

The public storage-only `backout`, cold projections and their bytes remain
unchanged. Selecting this explicit primitive requires the owning source-bound
operation's context/unit/SAF/core/audited-publication admission. It does not
initialize a native MQMD, resolve HardenGetBackout crash accuracy, authorize a
final task end, or implement recovery. Negative surrounding test observations
prove field preservation only, not native MQMD or licensed execution legality.
Selected full GET/BACK composition and final-task-end recovery remain required;
the primitive alone grants no executable call or full-v0.15 acceptance credit.

## Qualified full GET resolved-name observation

`QualifiedFullGet(MqMqiFullGet)` explicitly selects the additive
`QualifiedFullGot(MqMqiQualifiedGot)` value. Original FullGet/FullGot literals,
canonical domains/preimages and storage@1–@5 classes remain exact. The new tuple
tags and MqMqiQualifiedGot typed object join the sole canonical encoder; fields
are characters, cursor, data_length, disposition, message, resolved_queue in its
canonical field order. Explicit structure characters are independent of body
Encoding/CCSID and are checked against meaningful complete MD and the original
request. No serialization of live object or context authority is introduced.

Pinned MQGET row0015 q101830_24/39/48 defines MD, copied prefix and DataLength for
both truncation forms. GMO q096715_1260–1268 defines ResolvedQName as the actual
local queue of a retrieved message, including alias/model differences. The first
qualified selected profile remains predefined ordinary local INPUT_SHARED,
Remove/NoWait, MD1/2 matching catalog@2, empty properties/header-free and actual
Local or NoSyncpoint. It encodes the held resolved queue through the existing
privileged structure source at the independently configured character profile;
no OD name, default/body charset, principal, copied row or GMT/JES sample supplies it.

Complete and accepted-removal observations carry exact48 bytes. No-message and
unknown retain absence. The pinned call page does not explicitly guarantee this
GMO field for rejected truncation/incomplete processing; this finite observation
retains absence there while preserving every defined MD/prefix/length. Absence
means no native writeback, preserving caller bytes, not an IBM-defined blank or
universal undefined assertion. Broader output applicability remains a source
review obligation rather than a fabricated output.

Same original core occurrence/current frame, live object/logical owner/unit,
typed Read SAF, catalog/marker/meta/control dependencies, delivery candidate and
insert-only exact result receipt publish under the existing audited transaction.
Known full commit alone adopts. Source errors, panic/reentry refusal and late
CAS/control/core/cancel/quota failures adopt nothing; postcommit Unknown retains
the receipt and fences without cleanup or redispatch. Replay rechecks the original
receipt/current authority and does not resample the resolved name. This boundary
does not implement interpreter/server/native aliases or all-argument writeback.

The sole replay codec chooses storage@6 iff QualifiedFullGot is present. Strict
required nullable fields, profile/type/48-byte width, duplicate/unknown/missing/
trailing/collection checks precede typed allocation; @6 additionally requires
its deterministic stored field order and bytes. Full original HostResult digest
is recomputed; old classes under @6 or the new class under old versions refuse.
No namespace, row-envelope, rich marker or delivery schema changes occur.
Older readers fail closed. Operator rollback needs a compatible reader or verified
backup preserving core/CALL/audit/receipt references; stripping/relabeling/deleting
protected new receipts or replay-dispatch is forbidden. No startup rewrite or
operator admission is granted by this implementation.
