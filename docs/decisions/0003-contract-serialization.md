# ADR-0003: Contract, Serialization, and Identity Policy

Status: **Accepted by repository owner**
Decision scope: **Public DTOs, durable state, artifacts, evidence, and hashes**

## Context

0.1 needs long-lived artifacts, checkpoints, execution records, z/OSMF payloads,
configuration, and compatibility evidence. Rust layout and ordinary serializer
output are not stable contracts.

## Decision

### Owned domain contracts

mainframe-env owns all stable DTOs and identifiers. Third-party framework types
are converted at adapter boundaries. Stable DTOs use private validation or
validated constructors so arbitrary deserialization does not create executable
state.

### Formats

| Data | 0.1 format |
|---|---|
| Product configuration | versioned TOML with explicit precedence |
| z/OSMF requests/responses | route-specific JSON/text/binary compatibility formats |
| Evidence and manifests | versioned JSON; canonical JSON only when hashing/signing it |
| IR human form | deterministic versioned text |
| IR/executable payload | owned versioned binary codec plus envelope |
| Checkpoint envelope | versioned JSON metadata plus bounded typed payload |
| SQL durable metadata | normalized versioned schema and migrations |
| Large immutable artifacts | opaque bytes in artifact store with integrity digest |

`bincode`, Postcard, Rust debug output, memory layout, and default enum encoding
are not public or durable contracts.

### Semantic identity

Artifact and input identity are SHA-256 hashes over an explicitly specified
canonical semantic manifest. The manifest uses length-delimited fields and
sorted collections. It includes exact semantic inputs and version identities,
not physical timestamps or absolute paths.

Payload bytes have an independent integrity digest. A change in storage codec
does not silently change semantic identity.

### Version families

The following evolve independently:

- source bundle;
- compiler API;
- IR object/text/binary envelope;
- dialect major/minor;
- executable artifact;
- execution API and outcomes;
- host-service interfaces;
- checkpoint and session state;
- SQL schema;
- z/OSMF route compatibility;
- product configuration; and
- conformance evidence.

An observable semantic change requires a dialect/interface major or an explicit
upgrade path. A product release number is not a substitute for contract
versions.

### Reader/writer policy

- Writers emit only the current schema.
- Readers support a documented finite range.
- Removed fields and enum values are reserved and never reused.
- Unknown executable operations fail legality.
- Incompatible checkpoint/artifact state is rejected, not best-effort decoded.
- Upgrade functions are pure, versioned, bounded, and tested with frozen
  fixtures.

## Rationale

Protobuf is not required by the 0.1 in-process architecture and ordinary
Protobuf serialization is not canonical for hashing. Human-readable versioned
metadata makes early compatibility review easier, while owned binary codecs
keep executable IR independent of Rust serialization internals.

## References

- [Protocol Buffers serialization is not canonical](https://protobuf.dev/programming-guides/serialization-not-canonical/)
- [RFC 8785 JSON Canonicalization Scheme](https://www.rfc-editor.org/rfc/rfc8785.html)
