# Canonical host effect representation, version 1

- Status: **Frozen contract; implementation deviations tracked before 0.9.0**
- Owner: execution and host-contract maintainers
- Scope: canonical persisted host request/result digest representation
- Applies from: mainframe-env 0.8.2 hardening

`mainframe-env.effect-canonical@1` is a frozen binary representation of the typed
`HostRequest` and `Result<HostResult, HostProblem>` values. It is not Rust Debug,
JSON text, a COBOL data layout, or a licensed mainframe representation.

## Bytes and hashing

Native root lifecycle subjects use independent bounded streaming domains
`mainframe-env.root-terminal-setup@1` and
`mainframe-env.root-terminal-resource@1`. They encode the frozen original
compiled setup and actual pre-terminal machine observation/complete closure,
respectively, through the same Encoder authority. They are neither application
HostRequest effects nor replacements for HostCanonicalV1. Matching their digest
does not attest physical host/store/control/provider identity or finality. See
[ADR 0034](../decisions/0034-mq-root-terminal-publication.md). Every existing
request/result golden byte and original effect identity remains unchanged.
These domains belong to the root-publication framework prerequisite; they do
not establish compiled pending-PUT or removed-GET terminal acceptance.

The request preimage begins with the ASCII bytes `mainframe-env.effect-request@1`
followed by a zero byte; the result preimage uses `mainframe-env.effect-result@1`
and a zero byte. SHA-256 hashes that domain prefix and the canonical value.
`canonical.rs` and its exhaustive `canonical/generated.rs` and
`canonical/cics.rs` modules are the explicit schema. Public type and variant
names appearing there are wire identifiers and must remain fixed within version
1, even if Rust types are renamed later.

The additive `HostRequest::MqMqi(MqMqiHostRequest)` and
`HostResult::MqMqi(MqMqiHostResult)` names are distinct from the retained legacy
`Mq` variants. `canonical/mq.rs` retains the legacy MQ implementations
mechanically; `canonical/mq_mqi.rs` frames the new host records through the same
streaming encoder. Existing `Mq` names, bytes, validators and grants are unchanged.
The new request object encodes `envelope` then `mutation`; the result object
encodes `limits` then `result`. Their values use the existing exhaustive MQI,
Mutation and limits implementations directly, without standalone MQI preimages
or digests replacing host payloads.

Every typed MQI occurrence conservatively uses `host.mq.write` and the existing
mutation journal/replay admission path, including inquiries and cursor-bearing
observations. This classification does not claim that every MQI call changes
queue state. The original `EffectRequest` sequence and required idempotency key
must match its embedded `Mutation`. `mq_mqi_occurrence` validates and borrows
that one immutable effect; envelope and mutation extraction cannot accept a
separately substituted payload. Run-unit identity remains in the outer effect,
while execution/principal, trusted context, provider constraints, live deadline
and cancellation, grants and SAF remain their existing dispatch authorities.

Before structural validation can clone bounded MQ fields, the shared encoder
counts the actual host request or Ok/result preimage under the original explicit
MQI canonical limit and the 64 MiB host ceiling. Host name, record, field,
record-count and property aggregate ceilings also apply, including requested
output capacities and reported truncation lengths. Result call/status, payload
and validation limits remain explicit. The future provider must compare the
result with its original request, capacities, UOW and handle lifetimes before
publication; standalone result validation does not attest that relationship.

Private MQI admission consumes the host API's validated original occurrence,
with no separate envelope, mutation or metadata-only effect argument. Its
trusted scope requires the already-admitted host Invocation, independently
minted lifecycle owner, provider descriptor and HostLimits; constructing that
scope does not attest arbitrary bindings. Admission compares the complete
Invocation and derives payload, mutation, run, sequence, key and deadline from
the one original borrow. It retains the full shared canonical HostRequest
digest and bounded byte count, explicitly distinct from standalone MQI shape
digests. Existing context and nested/outer origin decoders remain authoritative;
valid origin provenance never selects an application syncpoint coordinator.

The private result preflight binds the existing EffectResult sequence, exact
MQI call and original limits, then streams the complete host result budget and
uses ordinary host/result validation, including call-specific reviewed statuses.
Copied MQGET bodies, inquiry arrays, property name/value bytes and conversion
buffers must fit the original requested capacities. The existing get-disposition
validator preserves required lengths above capacity while checking actual copied
bytes and original mode/truncation intent. Property/conversion required-length
payload forms absent from the frozen vocabulary remain pending; reviewed
size-reporting status identities do not justify fabricated payloads or wire
equivalence. Pending, unknown and duplicate observations retain their identities,
and explicit shared UnknownOutcome keeps its existing precedence over corrupt
reply metadata. Preflight is not a returned-handle, context, UOW or state permit.
The actual service must still apply registry/state authority, SAF and atomic
publication, and translate post-dispatch uncertainty through the shared effect
authority. Live controls remain rechecked at owned boundaries.

This freezes a typed boundary, not a handler, executable route or accepted MQ
transaction participant. Pending structures, selectors, contexts and status
mapping retain their typed pending identities; validation does not turn pending
public dispatch into success. The MQ 9.4 baseline
`ibm-mq-9.4-mqi-2026-08-31`, catalog rows `0001`–`0026`, preserves 26 call
identities and 27 source positions, including both MQMHBUF positions.
`MQCB_FUNCTION` (row `0005`, `SSFKSJ_9.4.0/refdev/q101730_.html`) describes
callback parameters without an application entry point, so this command/reply
boundary rejects it. Source review and unit tests grant zero licensed or
conformance execution credit.

`MqMqiOutcome::ReviewedOutput` is an additive named variant (tag `41`, type
`MqMqiOutcome`, variant `ReviewedOutput`, two fields `output` then `status`).
It retains the reviewed call return and its bounded typed observations in the
complete `Ok(HostResult::MqMqi(...))` result preimage. Host limits inspect the
output, and provider preflight checks its original request relationship before
publication. Existing canonical domains, old outcome bytes, status catalog
digest and retained receipts are unchanged. The
[MQ programming boundary](../architecture/MQ-PROGRAMMING-SURFACE.md) defines
the explicitly admitted shapes and their source locators.

Older storage/replay codecs cannot consume this new variant without an explicit
lossless extension. They must reject it rather than discard output, substitute
status-only replies, mint fresh handles or re-execute an old effect. Historical
results retain their original form and digest; there is no rewrite or automatic
migration. This addition supplies neither a durable replay decoder nor registry
authority for handle-bearing outputs. Manager integration must extend the strict
typed non-handle replay codec and separately resolve historical handle authority.

All lengths/counts are unsigned 64-bit little-endian numbers. Integers are fixed
width little-endian with distinct signed/unsigned type tags; `usize` uses u64,
not native pointer width. Text is UTF-8 with a byte length, without Unicode
normalization. Raw bytes do not expand to decimal digits or escape sequences.

| Tag (hex) | Representation following the tag |
|---|---|
| 01 | UTF-8 byte length, text bytes |
| 02 | Raw byte length, bytes |
| 03 / 04 | Boolean false / true, no further bytes |
| 10..14 | u8/u16/u32/u64/u128 in fixed-width little-endian |
| 15 | usize represented as u64 little-endian |
| 18..1c | i8/i16/i32/i64/i128 in fixed-width two's-complement little-endian |
| 20 / 21 | None / Some followed by its encoded value |
| 22 / 23 | Ok / Err followed by its encoded value |
| 30 | Element count followed by encoded sequence elements |
| 31 | Pair count followed by key/value pairs in BTreeMap key order |
| 32 | Tuple element count followed by encoded elements |
| 40 | Encoded type name, field count, encoded field-name/value pairs |
| 41 | Encoded type name and variant name, field count, field-name/value pairs |
| 42 | Encoded newtype name followed by encoded identifier text |

Named fields are sorted by their ASCII field identifiers, not declaration order.
Tuple variants use field identifiers `0`, `1`, etc. Integer vectors are sequences
except `Vec<u8>`, byte slices and byte arrays, which use tag 02. None differs
from an empty byte vector, empty text, or a missing field. Collections have
explicit boundaries. There are no floating-point host values in this schema.
Adding a field/variant requires updating an exhaustive implementation; it cannot
silently disappear through a default serializer. A schema change requires
review of the protocol version, golden vectors and persistence compatibility.
The `AddressSet`, `Cancel`, `ChangeTask`, `Delay`, `Deq`, `Enq`, `HandleAid`, `HandleCondition`,
`IgnoreCondition`, `PopHandle`, `PushHandle`, `SetAssociationUserCorrData`, and
`Start`, `Suspend`, and `Retrieve` CICS operation identities and `Ignored` CICS
disposition are additive named variants: they do not alter the canonical bytes
of any existing value, and their exact variant-name bytes are frozen by golden
tests.

`SecurityRequest::ValidatePrincipal` is likewise an additive named variant. It
contains only the bounded `PrincipalId`, is non-mutating, and returns the
existing `SecurityDecision` vocabulary. Its exact canonical variant, field, and
newtype bytes are frozen by a golden vector; existing authentication,
authorization, and audit request bytes are unchanged. The security request
principal-field helper is isolated from the large generated encoder without
changing its wire domain or version.

A terminal CICS ABEND records `ABEND.DUMP` in the response output map with
schema `mainframe-env.cics.abend-dump@1` and exact value `requested` or
`suppressed`. The entry therefore participates in the ordinary canonical result
digest without changing the `CicsResponse` object shape. Historical retained
responses that lack the entry remain readable and make no dump claim.

## Typed size budgets

`CapabilityDescriptor.max_request_bytes` and `.max_result_bytes` now count the
canonical preimage, including domain prefix, tags, field identifiers and lengths.
The result budget includes its Ok/Err discriminant. Existing per-field HostLimits
still apply. The provider limit and the 64 MiB maximum canonical effect size are
both enforced. Counting and hashing stream through a checked sink; no expanded
Debug string or full-preimage allocation is required. Arithmetic overflow fails
as ResourceExhausted before the sink is called.

Oversized requests fail before provider dispatch. A malformed/oversized successful
mutation reply is UnknownOutcome after dispatch, not a retryable known rejection.
An explicit UnknownOutcome remains unknown even if its provider also corrupts
the sequence or declares an insufficient response budget. A journal encoding
or persistence failure after dispatch of a mutating request returns
`UnknownOutcome` and leaves a discoverable intent for explicit reconciliation,
including when the provider already returned known success. The intent records
the typed capability, dispatch owner, execution attempt, durable creation and
recovery-not-before ticks, and execution-event epoch.

`stale_intents` applies both the persisted recovery-not-before tick and a
positive minimum-age boundary in the execution's monotonic logical tick domain,
and omits active recovery leases.
`claim_stale_intent` installs an expiring recovery
owner/attempt/epoch fence with compare-and-swap semantics. Once claimed, a late
result from the original dispatcher conflicts rather than overwriting recovery.
The bounded `StaleEffectRecoveryWorker` asks a service resolver to query an
authoritative provider idempotency ledger and then uses
`reconcile_stale_intent` to record a proven completion or failure under the
original digest format. Pending or ambiguous observations are not redispatched;
an expired recovery lease can be claimed only at a higher recovery attempt and
epoch.

## Provider replay and lifecycle outbox encodings

Db2, IMS, and MQ replay receipts use the corresponding typed `HostRequest`
encoding above and record digest format
`mainframe-env.provider-replay-canonical@1`. A receipt without the format field
is legacy Debug format and fails as `UnknownOutcome`; it is never redispatched.
Provider-specific reconciliation requires the exact retained legacy digest and
the matching typed request/idempotency key before replacing only its replay
metadata.

RACROUTE uses domain `mainframe-env.racroute-request@1` followed by a zero byte,
explicit request/enum tags, framed strings and ordered fields. RACF command
replay uses domain `mainframe-env.racf-command@1` followed by a zero byte and
encodes the canonical command keyword, positionals, operand names, counts and
values. Credential values in `PASSWORD` and `PHRASE` operands of `ADDUSER`,
`ALTUSER`, and `PASSWORD` are replaced by typed redaction markers before
hashing; their operand kind and value count remain part of the identity. Thus a
credential retry must use the same idempotency key, while an intentional new
credential operation must use a new key.

On RACF database open, unversioned replay digests and generated command or
RACROUTE audit digests are overwritten with metadata-only hashes marked
`mainframe-env.legacy-replay-redacted@0`. This removes old raw-command and
Debug-derived hash oracles. Scrubbed receipts fail closed until an explicit
reconciliation binds the retained actor, operation and idempotency key to a
reviewed command or RACROUTE request and the caller attests the exact scrubbed
digest.

Lifecycle notifications use topic `execution.lifecycle.v1`. Their payload
begins with `mainframe-env.execution-lifecycle@1` and a zero byte, followed by
an explicit one-byte event tag. Effect sequence numbers are big-endian u64 and
completion return codes are big-endian i32. The encoder exhaustively matches
all lifecycle variants, so adding a variant requires an explicit wire choice.
Already-persisted `execution.lifecycle` rows retain their legacy topic and
payload for legacy draining; they are never relabeled as version 1 bytes.

## Persisted identity and upgrades

`EffectRecord.digest_format` is explicit: LegacyDebug or CanonicalHostV1. Intent,
result and transactional result transitions compare both format and digest. Keys,
execution IDs, run units and effect occurrences are unchanged by this encoding.

Legacy durable JSON schema 1 without a format field reads as LegacyDebug and its
existing 32-byte digests are preserved verbatim. Reconciliation can update the
result under the observed domain via `reconcile_unknown_versioned`; a requested
format mismatch is rejected. The older reconciliation API remains available to
existing callers, which are responsible for supplying a digest in the stored
format; new callers should use the version-checked API.

New records use durable JSON schema 3,
`digest_format: mainframe-env.effect-canonical@1`, fields
`request_canonical_v1` / `result_canonical_v1`, and the fenced intent metadata.
Schema-1 legacy and schema-2 canonical rows remain readable; missing metadata
is conservatively projected as the retained execution owner, attempt 1, no
typed capability, creation/recovery ticks 0, and effect sequence as its epoch,
making a pre-upgrade orphan discoverable after restart. New records
intentionally omit legacy `request` / `result`: old
decoders ignored schema numbers but required `request`, so a downgrade fails
rather than silently treating canonical digests as Debug digests. Unsupported
or internally mixed schema/format combinations fail closed. Recomputing old
Debug digests under a new compiler is forbidden; there is no bulk rewrite or
automatic deduplication-domain migration.

A migrated intent without a typed capability requires an operator-reviewed
legacy resolver keyed by its retained effect identity. It is never guessed into
a current provider route.

Drain/reconcile active counter-era runs according to the installed-call replay
and run-unit lifecycle upgrade rules. Completed cached replies from #55/#47 are
not re-executed merely because journal encoding changes. The recovery worker
establishes no generic exactly-once guarantee: it can finalize an intent only
from an authoritative service-specific observation.

CICS installed-program occurrences use the owned
[`cics-program-v2:` identity](../decisions/0027-cics-logical-program-frames.md),
binding the durable outer command, run unit, frame actor and bounded occurrence,
not the volatile task-global host counter. This changes only nested program-key
construction; canonical request/result bytes, outer effect keys and other
nested subsystem keys are unchanged. Inputs remain separately fingerprinted,
so changed inputs conflict under the original occurrence instead of choosing a
second child. Fresh installed-call protocol schema 3 explicitly admits this
domain; counter-era active protocol state cannot authorize it. Retained unknown
calls are not redispatched to reconstruct a frame.

## Private MQI core-intent binding

The private MQ service binding accepts only an actual original
`MqMqiAdmission::ServiceValidation` with ordinary `CoreEffect` provenance. It
borrows one `PlatformStore` for both `IdempotencyStore::effect` observation and
audited provider publication; no separately supplied intent or store can replace
them. Trusted host/lifecycle admission remains a construction precondition.
Nested CICS actor/root composition is pending: its outer key cannot substitute
for a same-actor canonical intent.

The observed record must be an unresolved, unrecovered canonical-host intent
matching the original execution/run, sequence/key, full shared HostRequest
digest, dispatch owner/attempt, capability, canonical audit resource and
invocation key. Creation is positive and no later than observation; the finite
recovery boundary equals the coordinator's minimum invocation/effect deadline.
The current execution must match the original principal, attempt, program
selector and artifact and remain running. The retained epoch is observed, never
inferred from an envelope or reconstructed. Backend lock/transaction checks
remain the final intent, execution, lease, clock and CAS authority.

Preparation rechecks live controls and monotonic observations, validates exact
typed audit identity, and bounds MQ-owned `mq-` mutation shapes to the shared
4,096-mutation ceiling and 64 MiB aggregate payload/identity budget. This is not
a row-schema or queue/UOW semantic validator. Only an actual successful decision
may accompany mutations; other known decisions are audit-only. Explicit unknown
outcome fails through shared `HostProblem::UnknownOutcome`. An audit decision is
not a SAF permit. The caller must first obtain the real resource/state/output
decision, preserving pending/unknown/duplicate outcomes and source-specific
statuses rather than treating result preflight as execution permission.

The borrowed prepared publication cannot replace the intent/store and is
consumed by the existing `publish_provider_states_audited` transaction. Its
final observation must equal the audit's decision tick; if time advances, the
caller drops/reprepares with a newly observed actual audit. Live cancellation,
deadline and time regression are checked again before publication. Failure has
no sequential row/audit fallback or automatic retry.

Publication does not complete the core intent/outbox or adopt queue state.
Binding is not at-most-once: the caller supplies actual replay/UOW/CAS
dependencies, adopts next state only after commit, and translates uncertainty
after dispatch through shared effect reconciliation. Selected queue authority,
durable UOW ownership, SAF, typed result/replay/retention, nested composition and
participant/public-route readiness remain separately required. No durable
schema, legacy canonical bytes or licensed credit changes here.

## Golden digests

HostRequest::State(StateRequest::Get { key: "one" }): 150 preimage bytes,
`ae9669241c55748ca32bfe633c73d5bc14eaf68550dfcad22353bd1d6329cf74`.

Err(HostProblem::UnknownOutcome): 83 preimage bytes,
`4df2605185ce5b0070cdc2054dc65da70a5bb2bb3302cc10b150468a06f7d5b9`.

Tests also cover map insertion order, absent versus empty, embedded 00/ff/newline,
length framing, exact provider budgets, and a value whose Debug implementation
panics while its canonical implementation succeeds.

Provider replay golden digests are:

- Db2: `73deaa15e0e23619ee059776d818b7aa0b39805f4dc350f46cb013d3242cb4ad`.
- IMS: `0be6adcc52e9699a9c1ae6976b0eba69e3b53d1de56b296a6c8a4e7a8621d872`.
- MQ: `15290c92f51c0823f3a65fbe5f4ff0a96efad4561394b0b8ba7f1225b85a313b`.
- RACROUTE: `f12a7fce354c0f3c1a42b54376597d9230b956d729186932208c9f519824f503`.
- Credential-redacted RACF command:
  `5b150c202f1af2c3d1f63a24875153e7055dcc894d28daa90de9f3eb5356035e`.

The typed MQI host golden request (MQCONN, owner IDs 1/2/3/4/epoch 5,
queue manager QMGR, sequence 7, key `mqi-host-golden`, default MQI limits) is
`433a011034843ece80caaef730dd18d990c3fba577df9d1dd84663004cf68b8f`.
The typed MQI host golden result (MQDISC, source-pinned OkNone/NoOutput,
default explicit limits) is
`feb3c22c6f1150b7a39bf17cee6612d0b722f524edc582e43b8a401b72826c27`.
