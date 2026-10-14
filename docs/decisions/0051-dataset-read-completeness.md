# ADR-0051: Explicit dataset read completeness

Status: Proposed implementation for issue #186
Owner: Dataset and shared host-contract maintainers
Scope: Bounded dataset read results and whole-input consumers
Applies from: mainframe-env dataset result contract @3

## Decision

Append `more: bool` to `DatasetResult::Records` and `MemberGeneration`. A true
value means matching input was omitted at `max_records`; it cannot accompany an
empty result. Read, generic, sequential, concatenation and member-generation
paths compute this observation before discarding records. Bytes and identities
remain paired within the requested bound.

Whole-input consumers reject `more` with `DATASET_READ_TRUNCATED`, a local host
budget condition with response 16 and response2 0. Batch retains an explicit
4,096-record budget per input DD, across concatenation entries. It fails the
step before executing a program on incomplete input. Cursor readers already
have continuation and end-of-input observations.

Single-record CICS and COBOL READ operations consume the requested record even
when `more` is true, preserving record areas, status and update context. The
interpreter requires completeness for SORT input, which needs the whole dataset.

## Compatibility

The provider advertises dataset result contract `@3`. Rust constructors must add
`more`; exhaustive patterns must inspect it or use the shared completeness
check before consuming records. No serialized result DTO schema or generator
owns these in-process variants. The file named `canonical/generated.rs` is the
hand-maintained exhaustive wire authority described by the canonical contract.

Complete results (`more=false`) keep the frozen `Records` and
`MemberGeneration` canonical names, field counts, order and bytes. Incomplete
results use new `RecordsTruncated` and `MemberGenerationTruncated` names, append
`more=true`, and therefore cannot share a digest with a complete prefix. The
canonical version-1 domain is retained through this append-only extension.
Independent legacy digest vectors pin both complete forms. Read results are
not stored in dataset mutation replay rows, so no durable codec migration is
needed. Request bytes and existing mutation receipts remain unchanged.

This decision defines a local completeness and resource-budget contract. It
introduces no IBM record-format, positioning or response-code projection and
grants no conformance credit.
