# mainframe-env-compiler-api

Ownership: compiler requests/results, validated stages, legality, artifact
manifests, and semantic fingerprints. Non-goals: a frontend implementation,
backend implementation, cache, or persistence adapter. Allowed dependencies:
source, diagnostics, IR, and SHA-256.

Invariants: analyze-only or illegal IR cannot become publishable; semantic and
payload identities are independent. Verify with
`cargo test -p mainframe-env-compiler-api`.
