# mainframe-env-compiler-api

Ownership: compiler requests/results, validated stages, legality, artifact
manifests, and semantic fingerprints. Non-goals: a frontend implementation,
backend implementation, cache, or persistence adapter. Allowed dependencies:
source, diagnostics, IR, and SHA-256.

Invariants: compiler-private parsed/semantic state cannot be fabricated;
verified HIR is consumed into lowered and legalized MIR; artifact bytes are
encoded only from legal MIR; `ArtifactContentId` is the payload SHA-256 used by
runtime `sha256:` references, while `SemanticArtifactId` has its own explicit
`semantic-sha256:` namespace. Verify with
`cargo test -p mainframe-env-compiler-api`.
