# mainframe-env-application

Ownership: content-addressed application manifests, bounded typed subsystem
sections, signature-gated generation staging, atomic ready-generation
selection, retained rollback generations, program artifacts, BMS/CSD resources,
and dataset catalogs.

The `mainframe-env.application-package@1` reader remains available for accepted
profile.carddemo packages. Version 2 adds host ABI libraries, SQL schemas and rows, IMS
definitions and rows, MQ resources, batch controllers, and security resources.
Current writers emit `mainframe-env.application-package@3` using framed identity
inputs. The Rust DTO remains `ApplicationPackageV2`. Trusted retained @2 recovery
preserves original signatures and identities; fresh @2 admission refuses, and
legacy retry requires complete retained-package equality and current trust.
Standalone @1 remains available. Back up retained state before upgrade because
older binaries cannot read @3. See [ADR 0051](../../../docs/decisions/0051-package-identity-framing.md).

The optional IMS metadata section binds the shared versioned DBD/PSB contract
into the signed package identity. Every
cross-reference is validated before a generation can be staged. A signature
verifier is mandatory; the package kernel does not contain a trust store or
accept a digest as a signature.

Every blob-bearing reference is inside the validated manifest closure. Section
counts and nested bounds are checked before normalization or owned allocation.
The bounded installer state persists retained package bytes and selection, and
revalidates signatures, content digests, sizes, and references on recovery.
Default identity writers enforce `PackageLimits`; an embedder can use
`package_generation_identity_with_limits` with the same explicit budget as its
installer. The package schema validates actual DTO structure, independently of
signature trust and reference admission.

A staged generation is never selected. Commit changes its state to ready and
selects it under one application lock. Repeating stage/commit for the same
identity is idempotent, while same-generation conflicts, stale generations,
bad signatures, changed or non-manifest blobs, unresolved references, and
exceeded bounds fail
closed. Rollback selects only a retained ready generation.

Non-goals: provider state, application-specific behavior, native executable
loading, or a plugin marketplace. Verify with
`cargo test -p mainframe-env-application --locked` and
`cargo xtask application-packages --check`.

## Documentation

[Documentation portal](../../../docs/README.md) · [Current package map](../../../docs/architecture/PACKAGE-MAP.md) · [Embedding guide](../../../docs/guides/EMBEDDING.md) · [Contribution and verification](../../../CONTRIBUTING.md)
