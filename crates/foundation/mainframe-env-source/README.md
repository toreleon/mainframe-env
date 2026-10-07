# mainframe-env-source

Ownership: mainframe-env compiler foundation. Non-goals: parsing, language
semantics, filesystem discovery, and persistence. It owns exact source bytes,
validated logical paths, source formats/encodings, deterministic identities,
and bounded expansion provenance. It may depend only on narrowly approved
deterministic utilities. Public surface: the types re-exported by `lib.rs`.

Invariants: physical paths and timestamps never enter semantic identity; every
file, byte, option, and provenance edge is bounded. Verify with
`cargo test -p mainframe-env-source`.

Explicit multi-file closures use ordered `SourceLibrary` values. Library order
is part of the additive source identity; missing, duplicate, ambiguous, unknown,
or unassigned members fail before compiler publication. The legacy
`SourceBundle::new` identity remains unchanged.

`HostAbiLibraryDefinition` is the deterministic boundary for subsystem-owned
compatibility source. It validates version, Apache-2.0 license, non-vendor
origin, exact member bytes, bounds, uniqueness, and ordered materialization;
it does not supply ABI content itself.

## Documentation

[Documentation portal](../../../docs/README.md) · [Current package map](../../../docs/architecture/PACKAGE-MAP.md) · [Embedding guide](../../../docs/guides/EMBEDDING.md) · [Contribution and verification](../../../CONTRIBUTING.md)
