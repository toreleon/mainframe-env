# mainframe-env-conformance

Ownership: independent 0.1 fixtures, selected-route observations, and
out-of-process oracle comparison. Non-goals: production authority or reusable
product state. It may depend on public product crates; production crates may
not depend on it.

Invariants: fixtures are immutable and bounded, compatibility mismatches stay
exact, oracle execution is out of process, and external CardDemo paths never
enter receipts. `CARDDEMO_CORPUS_DIR` is consulted only by the explicit
CardDemo corpus gate. Verify with `cargo test -p mainframe-env-conformance`.
The corpus-backed source-preprocessor acceptance route is
`cargo xtask carddemo-source --check`.

## Documentation

[Documentation portal](../../../docs/README.md) · [Current package map](../../../docs/architecture/PACKAGE-MAP.md) · [Embedding guide](../../../docs/guides/EMBEDDING.md) · [Contribution and verification](../../../CONTRIBUTING.md)
