# Canonical host effect representation, version 1

- Status: **Frozen contract; implementation deviations tracked before 0.9.0**
- Owner: execution and host-contract maintainers
- Applies from: mainframe-env 0.8.2 hardening

`mainframe-env.effect-canonical@1` is a frozen binary representation of the typed
`HostRequest` and `Result<HostResult, HostProblem>` values. It is not Rust Debug,
JSON text, a COBOL data layout, or a licensed mainframe representation.

## Bytes and hashing

The request preimage begins with the ASCII bytes `mainframe-env.effect-request@1`
followed by a zero byte; the result preimage uses `mainframe-env.effect-result@1`
and a zero byte. SHA-256 hashes that domain prefix and the canonical value.
`canonical.rs` and its exhaustive `canonical/generated.rs` are the explicit
schema. Public type and variant names appearing there are wire identifiers and
must remain fixed within version 1, even if Rust types are renamed later.

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
failure after dispatch must leave a discoverable intent for explicit
reconciliation.

The current implementation does not yet satisfy that last requirement for a
known-success mutation whose result journal write fails: it can return an
infrastructure failure while leaving an intent that the reconciliation query
does not enumerate. Provider-local Db2, IMS, MQ, and RACF replay digests also
still contain diagnostic `Debug` encodings outside this canonical codec. These
are release-blocking deviations R-01 and R-07 in the
[pre-0.9 review](../reviews/PRE-0.9.0-DEEP-REVIEW.md), not exceptions to this
contract.

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

New records use schema 2, `digest_format: mainframe-env.effect-canonical@1`, and
fields `request_canonical_v1` / `result_canonical_v1`. They intentionally omit
legacy `request` / `result`: old decoders ignored schema numbers but required
`request`, so a downgrade fails rather than silently treating canonical digests
as Debug digests. Unsupported or internally mixed schema/format combinations
fail closed. Recomputing old Debug digests under a new compiler is forbidden;
there is no bulk rewrite or automatic deduplication-domain migration.

Drain/reconcile active counter-era runs according to the installed-call replay
and run-unit lifecycle upgrade rules. Completed cached replies from #55/#47 are
not re-executed merely because journal encoding changes. This adds neither an
automatic recovery algorithm nor an exactly-once guarantee.

## Golden digests

HostRequest::State(StateRequest::Get { key: "one" }): 150 preimage bytes,
`ae9669241c55748ca32bfe633c73d5bc14eaf68550dfcad22353bd1d6329cf74`.

Err(HostProblem::UnknownOutcome): 83 preimage bytes,
`4df2605185ce5b0070cdc2054dc65da70a5bb2bb3302cc10b150468a06f7d5b9`.

Tests also cover map insertion order, absent versus empty, embedded 00/ff/newline,
length framing, exact provider budgets, and a value whose Debug implementation
panics while its canonical implementation succeeds.
