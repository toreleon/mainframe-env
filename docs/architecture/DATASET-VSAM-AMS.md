# Dataset, VSAM, catalog, and AMS authority

Status: **Normative from 0.6.0**

## Authority boundary

`mainframe-env-dataset` owns dataset definitions, catalog relationships,
allocation, deterministic abstract volumes, lifecycle, record/index state,
locking, RLS/TVS decisions, and dataset recovery. The host contract carries
owned bounded DTOs. SQLx provider-state transactions and artifact storage are
replaceable persistence adapters; their locks, transaction result, object key,
or filesystem layout do not define VSAM semantics.

AMS is a language and orchestration adapter over these typed operations. It
does not own a catalog database, allocation table, record representation, or
recovery log. JCL, CICS, z/OSMF, application installation, and AMS observe the
same selected dataset authority.

## Frozen programming surface

The normative detailed inventory is
`conformance/0.6/inventory/dataset-programming-surface.json`. It contains 130
typed surface descriptors in ten families, including all 31 frozen AMS command
rows and exact mappings for the five official VSAM organization rows. It adds
obligations beneath the immutable 0.2 denominator; it does not invent new IBM
coverage rows.

`cargo xtask dataset-contract --check` compiles the Draft 2020-12 schemas,
checks the 36 official mappings, validates the exact family counts and command
order, and verifies the generated Rust descriptor registry. Catalog presence
or descriptor generation grants no behavior coverage.

## Definition and capability contracts

`mainframe-env.dataset-definition@1` composes:

- organization, record format, LRECL, key, and CCSID attributes;
- block size and bounded buffering;
- primary, secondary, directory, and extent policy;
- ordered volume selection and unit count;
- SMS class and extended-format/addressability attributes;
- CI/CA, SHAREOPTIONS, spanned, reuse, erase, write-check, buffering, and
  striping attributes;
- compression and encryption-key references;
- catalog type, catalog selection, owner, expiration, and retention; and
- lifecycle, migration level, and backup generation.

The deterministic provider advertises abstract-volume support. Physical disk,
tape, ACS, encryption, compression, striping, migration/recall, RLS, and TVS
remain false until their adapter or semantic work package passes. An operand
that requires a false capability returns `UnsupportedCapability` with both the
capability and affected operand; it cannot be silently stored, ignored, or
reported as successful.

## Durable state and migration

Dataset state writer version 3 (`MEDS3`) is a bounded owned binary codec. The
reader accepts `MEDS1`, `MEDS2`, and `MEDS3`. Older records materialize reviewed
compatibility defaults and are fully validated before any version-3 write.
Corrupt, over-limit, cross-reference-invalid, or capability-invalid state fails
before publication.

The portable state, diagnostic, capability, and migration schemas live in
`conformance/0.6/schemas`. The non-destructive migration contract is
`conformance/0.6/migrations/dataset-state-v2-to-v3.json`. Rollback restores a
digest-verified pre-migration provider snapshot atomically; partial version-3
state is never selected. Backup/restore and injected-failure evidence belongs
to DAT-606.

## Deterministic allocation model

The abstract adapter calculates capacity from declared units using fixed
contract constants: 56,664 bytes per track and 849,960 bytes per cylinder;
block allocations use explicit BLKSIZE or LRECL, and record allocations use
LRECL. These values model deterministic allocation behavior only. They do not
claim a real device geometry, VTOC placement, tape operation, compression
ratio, encryption, or stripe layout.

## Evolution rules

- Writers emit only the current state version; readers support the documented
  finite range.
- A new accepted operand extends the normative inventory and generated
  registry before public handling is added.
- Every operand changes definition, allocation, catalog, volume, lifecycle, or
  access behavior, or fails with an explicit capability diagnostic.
- Catalog aliases, GDGs, AIX/base relations, migration ordering, and
  invalidation reuse one bounded dependency-graph authority.
- Dataset names and initial layouts remain application-package data; provider
  code cannot dispatch on them.
