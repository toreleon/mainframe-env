# mainframe-env-zosmf

Thin compatibility translation for the 23 accepted z/OSMF 0.1 routes. It owns
HTTP path/query/header/body DTOs and stable HTTP error mapping only. Dataset,
security, job, spool, and console state remain behind the typed `ZosmfBackend`
application-service boundary.

The 23 official method/path bindings are generated from the frozen owned route
catalog. Seven custom CICS session methods are generated from a separate
`/mainframe-env/*` namespace inventory; official and custom IDs cannot overlap.

ZMF-1101 adds a generated, non-advertising view of the pinned z/OSMF 3.2
normalization authority. It exposes family/backend and legacy-route/operation
metadata without registering any of the 352 candidate route variants. Missing,
partial, or unresolved backends and identity-only payload schemas remain
publication blockers; the existing 23 official routes and seven custom routes
are unchanged.

## Documentation

[Documentation portal](../../../docs/README.md) · [Current package map](../../../docs/architecture/PACKAGE-MAP.md) · [Embedding guide](../../../docs/guides/EMBEDDING.md) · [Contribution and verification](../../../CONTRIBUTING.md)
