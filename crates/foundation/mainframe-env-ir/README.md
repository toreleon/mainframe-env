# mainframe-env-ir

Ownership: generic typed-ID IR, operation catalogs, verification, canonical
text, and the owned binary envelope. Non-goals: COBOL ASTs, provider access,
execution scheduling, or infrastructure codecs. Allowed dependencies are the
source/diagnostic foundation and SHA-256 implementation.

The generated IMS call registry is compiled from the pinned 25-row comparison
catalog with `cargo xtask ims-catalog`. It preserves duplicate call and command
memberships for identity lookup. It makes no claim about handler readiness or
behavioral coverage.

Invariants: all arenas and strings are bounded; unknown operations and invalid
references fail verification; binary readers authenticate and bound payloads
before allocation. Verify with `cargo test -p mainframe-env-ir`.

## Documentation

[Documentation portal](../../../docs/README.md) · [Current package map](../../../docs/architecture/PACKAGE-MAP.md) · [Embedding guide](../../../docs/guides/EMBEDDING.md) · [Contribution and verification](../../../CONTRIBUTING.md)
