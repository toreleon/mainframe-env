# ADR-0001: 0.1 Technology Stack

Status: **Accepted by repository owner**
Owner: **repository owner**
Scope: **technology stack and deferred framework boundaries**
Applies from: **mainframe-env 0.1.0**
Decision scope: **mainframe-env 0.1**

## Context

0.1 must support COBOL, CICS, JCL/JES, datasets, RACF/security, and z/OSMF with a
small, supportable dependency surface. Infrastructure frameworks must remain
replaceable and cannot define mainframe semantics or stable domain contracts.

## Decision

### Language and build

- Rust Edition 2024.
- Pinned development/release toolchain: Rust 1.98.0.
- Contract/MSRV floor: Rust 1.95.0, verified separately in CI.
- Cargo resolver 3.
- One committed application `Cargo.lock`.
- Workspace dependencies declared centrally with minimal feature sets.

### Adopt for 0.1

| Concern | Technology | Boundary |
|---|---|---|
| Async I/O and task lifecycle | Tokio | applications and execution shell only |
| Service middleware/backpressure | Tower | application/provider boundaries |
| HTTP/z/OSMF gateway | Axum + Hyper | gateway and server crates |
| TLS | Rustls | server transport adapter |
| Database access | SQLx | SQL store adapter only |
| Local durable store | SQLite | local profile |
| Production durable store | PostgreSQL 18 | production metadata/state profile |
| Immutable blobs | Apache `object_store` | artifact adapter |
| Lossless syntax trees | Rowan | frontend syntax package |
| Serialization plumbing | Serde, `serde_json`, TOML | codecs/config/evidence adapters |
| Typed library errors | `thiserror` | implementation convenience, no leaked type dependency |
| Diagnostic rendering | Miette | CLI/application renderer only |
| Instrumentation | `tracing` | structured internal events; exporter is application-owned |
| CLI | Clap | CLI application only |
| Property testing | Proptest | verification packages |
| Fuzzing | cargo-fuzz/libFuzzer | parsers, codecs, protocols |
| Concurrency exploration | Loom | custom concurrency primitives only |
| Bounded verification | Kani | pure validators/state transitions where useful |

### Selective use

- Handwritten lexers and parsers are authoritative for COBOL and JCL.
- Logos or Winnow may be used for small isolated grammars when they materially
  simplify code without hiding recovery or source mapping.
- OpenTelemetry export may be added at the application boundary; OpenTelemetry
  types do not enter platform contracts.
- PostgreSQL-specific queue claims are implementation details behind the work
  store contract.

### Deferred beyond 0.1

The following are not 0.1 dependencies or deliverables:

- Wasmtime/WIT and external Wasm plugins;
- Tonic/Protobuf process or remote workers;
- NATS JetStream or another message broker;
- Cranelift, LLVM, JIT, or native AOT backends;
- Salsa incremental query engine;
- Cedar or another general policy engine;
- Temporal, Restate, or another durable workflow platform;
- Kubernetes/multi-node placement infrastructure; and
- Tree-sitter as an authoritative compiler parser.

Deferred technology requires a new ADR based on an accepted in-scope workload.

## Rationale

Tokio provides explicit bounded channels and task primitives; Tower provides a
protocol-neutral readiness and middleware model; Axum reuses Tower rather than
creating a separate middleware stack. They are therefore appropriate for the
outer asynchronous shell but unnecessary in compiler and machine semantics.

SQLx keeps SQL visible, supports SQLite and PostgreSQL, and permits checked
queries without imposing an ORM domain model. PostgreSQL provides transactional
metadata, row locking, and a supported production lifecycle. Object storage is
suited to immutable content-addressed artifacts and avoids treating filesystem
paths as platform contracts.

Rowan supports lossless concrete syntax trees while leaving lexical, parse,
recovery, and semantic policy under mainframe-env control.

## Consequences

- Foundation and contract crates have very small dependency graphs.
- Applications may depend on infrastructure frameworks but translate at the
  boundary.
- 0.1 does not pay build, security, or operational cost for speculative plugin,
  broker, native-backend, or distributed frameworks.
- Framework upgrades cannot change durable or public DTO semantics without an
  explicit contract change.

## References

- [Rust 1.98 release](https://blog.rust-lang.org/releases/latest/)
- [Rust 2024 and resolver 3](https://doc.rust-lang.org/stable/edition-guide/rust-2024/cargo-resolver.html)
- [Tokio bounded channels](https://tokio.rs/tokio/tutorial/channels)
- [Tower service readiness](https://docs.rs/tower-service/latest/tower_service/trait.Service.html)
- [Axum integration model](https://docs.rs/axum/latest/axum/index.html)
- [SQLx](https://github.com/launchbadge/sqlx)
- [PostgreSQL version support](https://www.postgresql.org/support/versioning/)
- [Apache object_store](https://docs.rs/object_store/latest/object_store/)
- [Rowan](https://docs.rs/rowan/latest/rowan/)
- [Rustls](https://docs.rs/rustls/latest/rustls/)
