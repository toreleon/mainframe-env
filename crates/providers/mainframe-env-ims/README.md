# mainframe-env-ims

Ownership: IMS provider maintainers.

This crate owns the bounded deterministic IMS application authority currently
required by the product: HIDAM-style root/child data, secondary indexes,
PSB/PCB selection, DLI navigation and mutation, checkpointing, load/unload,
durable commit/rollback, replay, and host-provider registration. It also owns
bounded TM transaction, message, I/O/alternate-PCB, and conversational contracts.

## Invariants

- Callers use typed `mainframe-env-host-api` requests and results.
- Provider state is bounded by `ImsLimits` and persisted only through the owned
  `ProviderStateStore` contract as versioned database, session, checkpoint,
  unit-of-work, and replay rows.
- Mutations require idempotency and preserve run-unit transaction boundaries.
- Unknown outcomes, conditions, cancellation, and provider failures remain
  distinct.
- Application names and conformance row IDs never select production behavior.
- Versioned DBD/PSB metadata validation closes names, hierarchy, fields, indexes,
  relationships, PCB options, and sensitivity paths before producing a digest.

## Allowed dependencies

The provider depends on execution, host, and store contracts plus serialization
and digest utilities. Product applications may compose it; the provider must
not depend on Axum, CLI DTOs, conformance tooling, or another application's
state.

## Public surface

`ImsService` installs validated application definitions and exposes typed host
providers through `ims_providers`. Public definition and limit types describe
the bounded installation and execution contract. `TmDefinitionSet`,
`TmInputMessage`, and `TmCall` define the TM contract without a TM scheduler or
durable TM runtime in this slice. `ImsMetadataCatalog` and
`validate_ims_metadata` expose the additive `mainframe-env.ims-metadata@1`
contract, described by the Draft 2020-12
[`metadata schema`](../../../conformance/0.14/schemas/ims-metadata.schema.json).

The `database` module exports the recovered in-memory engine foundation. It
validates bounded hierarchy, field and index definitions, and owns deterministic
GU/GN/GNP-style selection, caller-owned position and holds, insert, replace,
physical subtree delete, append-only GSAM, and secondary-index maintenance.
It is isolated from `ims_providers`; it does not emit PCB statuses or persist
its image. Metadata publication, host routing, authorization and UOW integration
remain separate contracts.

## Non-goals

- Complete IMS 15.6 compatibility, which belongs to the 0.14 program.
- DRDA, network protocol, deployment, or UI behavior.
- Licensed IBM equivalence from local/model results.
- A multi-node storage architecture.

The row layout and legacy migration rules are frozen in the
[provider row contract](../../../docs/contracts/PROVIDER-ROW-PERSISTENCE-V1.md).

## Verification

```bash
cargo test --locked -p mainframe-env-ims
cargo clippy --locked -p mainframe-env-ims --all-targets -- -D warnings
cargo xtask architecture-fast --check
```

Cross-provider, restart, authorization, and application-profile behavior is
also exercised by `mainframe-env-conformance` and the applicable certification
gates.
