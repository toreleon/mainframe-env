# mainframe-env-compiler-api

Ownership: compiler requests/results, validated stages, legality, artifact
manifests, and semantic fingerprints. Non-goals: a frontend implementation,
backend implementation, cache, or persistence adapter. Allowed dependencies:
source, diagnostics, IR, and SHA-256.

Invariants: compiler-private parsed/semantic state cannot be fabricated;
verified HIR is consumed into lowered and legalized MIR; artifact bytes are
encoded only from legal MIR; `ArtifactContentId` is the payload SHA-256 used by
runtime `sha256:` references, while `SemanticArtifactId` has its own explicit
`semantic-sha256:` namespace. Current `mainframe-env.artifact@3` manifests name
the exact payload-derived `namespace@major` dialect set; publication rejects a
missing, extra, or stale dialect declaration. The pre-dialect manifest remains
identified as `mainframe-env.artifact@2` for historical compatibility. It is
accepted only through `ValidatedArtifact::read`, which decodes canonical IR,
verifies the executable profile, derives the missing dialect set, and retains
the original bytes and source-contract identity. Verify with
`cargo test -p mainframe-env-compiler-api`.

## Documentation

[Documentation portal](../../../docs/README.md) · [Current package map](../../../docs/architecture/PACKAGE-MAP.md) · [Embedding guide](../../../docs/guides/EMBEDDING.md) · [Contribution and verification](../../../CONTRIBUTING.md)
