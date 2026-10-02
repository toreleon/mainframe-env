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

`ImsService::execute_pcb_feedback_v1` and `HostRequest::ImsPcbFeedbackV1`
expose bounded owned selected database PCB feedback through the existing
provider and atomic receipt pipeline. Primary successful retrieval/ISRT return
concatenated keys, segment name/level, metadata fields and transferred data
length. Failed-call witnesses, secondary key layouts and physical masks remain
explicitly unproved. See the [class review](../../../docs/delivery/subsystems/ims/selected-pcb-feedback.md)
and [ADR-0035](../../../docs/decisions/0035-selected-pcb-feedback.md).

`ImsService` installs validated application definitions and exposes typed host
providers through `ims_providers`. `ims_providers_with_recovery` adds logical
LOG and basic/symbolic CHKP/XRST dispatch using the canonical journal from the
same shared store authority.
The selected DB-batch CALL adapter requires typed PSB/database SAF and an exact
canonical effect intent before loading the existing RecoverySession. It publishes
the log and selection fence through the existing utility bridge. CHKP atomically
releases actual undo, all PCB positions/holds and Q reservations with its recovery
receipt; symbolic CHKP saves seven bounded areas and provider-derived key paths.
XRST observes those paths through real qualified GU and retains real GN position.
The Session enforces once-per-execution-attempt XRST and checkpoint-kind order.
Read-only recovery observation lets a fenced coordinator resolver distinguish
published results from ambiguous outcomes without redispatch. Recovery session keys bind
the selected application, package, PSB/database, run and principal; they are
addresses in the existing version-one recovery namespace, not a new authority.
GSAM symbolic CHKP retains discriminated live logical addresses, beginning/EOF
and output prefix positions through the same resolver and atomic bridge. Its
checkpoint commits pending writes; XRST removes a witnessed later output suffix
and restores independent selected input PCB positions. Removed addresses remain
invalid, while a settled row-version change alone does not prevent valid resume.
Basic CHKP rejects selected GSAM. Later committed output without a UOW witness
requires reconciliation and returns UnknownOutcome without erasure.
Timestamp context identity, BMP/LAST, physical GSAM RSA/file restoration, other recovery
families and contexts, raw language framing, physical log sizing,
participant admission and concurrent recovery-lease fencing remain pending.
Public definition and limit types describe
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
indexes concatenate one to five source fields in their declared order. Pointer
entries resolve to the source occurrence or its physical ancestor target.
An optional DB PCB `secondary_index` selects a full-function physical-root
target processing sequence. GU/GN/GNP and holds retain independent pointer
cursors per PCB; named qualifiers use the index's XDFLD identity on its target.
Primary and selected updates maintain the same index. Selected target ISRT/DLET
are rejected, and successful indexed target replacement or search-byte changes
lose selected parentage. Nonroot restructuring/aliases, Fast Path secondary
processing, optional search fields in selected sequences, NULLVAL/exits and
SUBSEQ/user pointer data remain unavailable in this bounded shape. Legacy and generic catalogs may coexist
when their database and PSB names do not overlap.

Historical metadata without a PCB selector, single-field engine descriptors and
positions without a pointer cursor retain their serialized bytes and identities.
Engine images rebuild pointer maps from retained records and validate target
ancestry and uniqueness before use. Extended descriptors and selected cursors
are additive reader inputs. Extended engine descriptors write `source_field`
instead of the old required `field`, forcing prior readers to reject rather
than silently ignore extensions; old metadata/position readers reject selected
PCB/cursor fields. Before
admitting extended metadata or writing such images, drain older writers and take
a coherent backup of metadata/package generations, database images, sessions,
checkpoints, UOW/undo and replay rows. Binary rollback requires stopping admission
and restoring that pre-feature backup with its referenced artifacts. Removing
fields from live rows is not a rollback or an identity-preserving migration.
The additive `ImsGsamRequest` route supplies fixed-length GSAM GU/GN/ISRT in
DB batch with selected-PCB positions, authorization, shared UOW authority,
atomic provider rows, and canonical replay. Its issued `ImsGsamAddress` is a
bounded host logical identity, not IBM's physical RSA layout. See
[ADR-0034](../../../docs/decisions/0034-gsam-logical-address.md) for the source
comparison, symbolic checkpoint/restart behavior, and compatibility limits.

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
