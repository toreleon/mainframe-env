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
`TmInputMessage`, and `TmCall` define the TM contract. `TmService` persists
bounded catalog, message, session, conversation, outbound, and replay rows
through `ProviderStateStore`. It uses `WorkStore` for scheduling, cancellation,
the durable logical clock, and fenced leases. The existing
`EnterpriseAuthorizer` checks PSB, transaction, and destination access before
protected transitions. `ImsMetadataCatalog` and
`validate_ims_metadata` expose the additive `mainframe-env.ims-metadata@1`
contract, described by the Draft 2020-12
[`metadata schema`](../../../conformance/0.14/schemas/ims-metadata.schema.json).

Package publication retains at most 64 validated metadata generations per
application in provider-owned versioned rows and atomically advances a separate
selected-generation row. Repeated publication is idempotent, conflicts fail
closed, and selecting a retained rollback generation uses the verified package
identity and catalog.

The `database` module exports the recovered in-memory engine foundation. It
validates bounded hierarchy, field and index definitions, and owns deterministic
GU/GN/GNP-style selection, caller-owned position and holds, insert, replace,
physical subtree delete, append-only GSAM, and secondary-index maintenance.
It is isolated from `ims_providers`; it does not emit PCB statuses or persist
its image. Metadata publication, host routing, authorization and UOW integration
remain separate contracts.

The engine is available through the existing `ImsService` and `ims_providers`
route after `install_metadata`. The typed catalog selects database and PCB
authority; separate versioned rows retain generic images and undo, while
sessions and replay use the existing IMS rows. GU/GN/GNP and their Get Hold
forms, ISRT/REPL/DLET, bulk load/unload, checkpoint, commit, and rollback use
the bounded engine for all pinned data organizations. INDEX and PSINDEX are
metadata-validated index databases and reject application data PCB scheduling.
Logical child occurrences retain metadata-selected parent links in the existing
database images; a child read includes the linked parent data, and paired
parent deletion removes linked children in one provider-row CAS publication.
Insertion identifies one logical parent with parent-segment field qualifiers.
An unpaired parent with live children rejects deletion. Composite secondary
indexes remain outside this route. Legacy and generic catalogs may coexist
when their database and PSB names do not overlap.

TM admission records a provider-row intent before adding work. An exact retry
or `repair_schedules` repairs that bounded cross-interface gap without executing
the application. Current-step nonexpress output is published at commit and
discarded at rollback; express PURG output remains available. SQLite reopen
preserves the recorded TM cursor, output, and conversation state. This is a
provider foundation, not an application execution route or licensed IMS claim.

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
