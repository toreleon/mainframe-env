# mainframe-env-spool

Ownership: JES/spool provider maintainers.

This crate owns durable JES spool metadata and immutable artifact-backed record
chunks. It implements typed append, list, read, seal, replay, restart, and
intent-first purge behavior for the batch authority.

## Invariants

- Spool state uses `mainframe-env.spool-state@1` and is bounded by
  `SpoolLimits`.
- Exact record bytes are retained in immutable artifacts; metadata references
  content-addressed chunks.
- Mutations require typed idempotency metadata and replay the original result.
- A partial purge remains recoverable and cannot delete a chunk still needed by
  another logical owner.
- Authorization occurs before this provider is invoked; public callers use the
  host contract rather than internal state.

## Allowed dependencies

The provider depends only on execution, host, and store contracts plus
serialization/digest utilities. It must not depend on batch implementation
internals, HTTP, conformance tooling, or application-specific workload names.

## Public surface

`SpoolService` composes a `ProviderStateStore` and `ArtifactStore`.
`spool_providers` exposes the typed host-provider registration, while
`ProviderArtifactStore` adapts an owned provider-state namespace where needed.

## Non-goals

- JES scheduling, DD allocation, or job lifecycle ownership.
- Output transport, UI, or external object-store protocol semantics.
- Multi-node replication or licensed JES2 equivalence.

## Verification

```bash
cargo test --locked -p mainframe-env-spool
cargo clippy --locked -p mainframe-env-spool --all-targets -- -D warnings
cargo xtask architecture-fast --check
```

Batch integration, authorization, recovery, and artifact-retention behavior is
covered by `mainframe-env-batch`, `mainframe-env-server`, and conformance gates.

## Documentation

[Documentation portal](../../../docs/README.md) · [Current package map](../../../docs/architecture/PACKAGE-MAP.md) · [Embedding guide](../../../docs/guides/EMBEDDING.md) · [Contribution and verification](../../../CONTRIBUTING.md)
