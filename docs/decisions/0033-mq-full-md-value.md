# Complete MQMD value primitive

Status: **Proposed**
Owner: **MQ host contract maintainers**
Scope: **complete descriptor value and codec, not per-call admission or message execution**
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

This is a value codec for later composition with the one effect/replay authority.
It is not an accepted new HostRequest, result, checkpoint, replay or storage
schema. Every old DTO literal and canonical domain/preimage remains unchanged.
`MqRawCapture::try_typed_descriptor` continues returning
`DescriptorRepresentationPending`; silently reducing a complete MQMD to the old
partial DTO is forbidden. Full-message request/result, delivery, replay and
persistence evolution require a separate sealed composition and compatibility
decision.

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
full-26 and CardDemo acceptance remain manager-owned and incomplete. Only the
licensed IBM differential oracle is human-skipped, with zero licensed credit.
