# mainframe-env-source

Ownership: mainframe-env compiler foundation. Non-goals: parsing, language
semantics, filesystem discovery, and persistence. It owns exact source bytes,
validated logical paths, source formats/encodings, deterministic identities,
and bounded expansion provenance. It may depend only on narrowly approved
deterministic utilities. Public surface: the types re-exported by `lib.rs`.

Invariants: physical paths and timestamps never enter semantic identity; every
file, byte, option, and provenance edge is bounded. Verify with
`cargo test -p mainframe-env-source`.
