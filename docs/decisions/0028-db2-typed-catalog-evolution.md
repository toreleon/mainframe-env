# ADR-0028: Db2 typed catalog evolution through existing authorities

Status: **Proposed — implementation boundary, not owner acceptance**
Owner: **Db2, application-package and store-contract maintainers**
Scope: **db2.core catalog/binder preparation and versioned integration**
Applies from: **mainframe-env current subsystem contracts**

## Context

The accepted Db2 application catalog describes byte layout, defaults and
nullability, not exact SQL scalar types or SQL name identity. Its signed blob
reader consumes an array of table definitions; the historical catalog schema
describes a generation wrapper. Those are separate shapes, not evidence that
the production blob already has a version-selected SQL metadata codec.

The package runtime, selected generation handle, provider catalog installation,
object-row persistence, retained generations and rollback already own the
authority chain. Adding a second catalog or inferring SQL type from `max_bytes`
would break that chain. Legacy cells also cannot establish NULL versus empty
bytes, and primary-key-derived row maps cannot represent keyless duplicate rows.

## Proposed decision

### One source and one installed authority

Retain the existing signed `data/db2/catalog` entry, selected package handle and
provider installation/rollback path. Introduce an explicitly versioned typed
catalog blob contract `mainframe-env.db2-application-catalog@2`. Its normative
schema describes the actual blob, not a self-referential generation wrapper.
Application name, generation and signed package identity come from the selected
handle. The blob must not contain its own selected package identity.

The new model belongs to the existing Db2 catalog owner. A typed definition is
not another independently installed catalog, store or executor. Preserve v1 as
an explicit `LegacyBytes` representation; never synthesize SQL scalar types,
CCSID, collation, defaults or NULL values from legacy framing or host metadata.
The new binder fails explicitly on untyped metadata and does not retry legacy
SQL text dispatch after typed validation fails. Existing legacy byte routes
retain their accepted semantics.

One normative schema must generate applicable DTO/tag projections and schema
checks through the existing contract/xtask infrastructure. Validated constructors
and cross-reference/budget checks remain product-owned. Do not hand-maintain a
second enum inventory or copy the provider AST into the shared package layer.

### Metadata and identity

Typed definitions own effective qualified SQL name components, ordered columns,
exact scalar attributes, nullability, distinct default states, constraints and
explicit representation policy. Effective identifier equality uses owned SQL
identifier values; delimiter provenance is not a different catalog object.
Legacy punctuation-stripping and host suffix matching are not SQL equality.
Ambiguous legacy-to-SQL name mappings require explicit migration.

String subtype, length unit, encoding scheme, CCSID and comparison/collation
policy are explicit semantic inputs. A stored policy identity is not proof that
conversion or collation is implemented. Missing or unsupported context fails
closed at the applicable binder/runtime boundary. Likewise, absent default,
explicit NULL, type default and typed constant cannot collapse into one byte
default. A typed cell must distinguish NULL from empty value; no source spelling
or old byte value is silently promoted into a validated runtime cell.

Keep exact blob SHA-256, version-selected signed package identity and catalog
semantic identity separate. Specify a domain-separated, length/count-delimited
catalog encoding with fixed tags, ordered columns and sorted unordered sets.
Include every semantic field and absent/empty distinction, excluding source
positions, physical paths, timestamps and the package identity itself. Never
use ordinary Serde JSON as canonical semantic identity. Preserve frozen v1/v2
package signature preimages.

### Package and durable versions

A typed SQL package section should reference the signed catalog contract and
identity, rather than duplicate complete column definitions under a second
authority. If its shape/meaning changes, introduce `application.sql@2` and a
version-selected package envelope; preserve the existing package v2 reader and
signature algorithm. Version retained installer/publication payloads when they
contain those new package shapes. No optional SQL field may silently change a
frozen `@1` or `@2` package contract.

Introduce a Db2 manifest `@2` capability barrier and namespace/value-aware
object envelope readers/writers before any typed metadata is persisted. Keep
existing namespace locators and CAS/atomic mutation authority; their historical
`v1` spelling does not require another catalog namespace. Readers accept only
declared finite legacy/new combinations. New typed writes require the new
manifest; nested unknown fields/tags, duplicates, unsupported versions and
key/kind/namespace mismatches fail before publication. A malformed tagged
manifest must never fall through to unversioned legacy decoding.

Schema definitions, retained generation snapshots and adopted legacy snapshots
carry their exact metadata/value versions. Version replay/retention readers and
archive validation whenever their envelopes change; preserve canonical request
and result digests and original archive bytes. Atomic migration preserves
ownership, row CAS, rollback references and failure behavior.

### Compatibility and remaining runtime obligations

No implicit type or value conversion is an in-place compatible upgrade. Complete
semantic compatibility includes all new attributes. Migration requires quiescent
admission and drained old writers. New-binary rollback restores retained v1/v2
metadata and rows atomically. Old-binary rollback requires the declared pre-upgrade
backup until a lossless downgrade is independently implemented and verified.
Never strip metadata or lower markers to simulate compatibility.

General SQL metadata permits no primary key. Admitting such tables to storage
requires stable durable row identity preserving equal-valued duplicate rows;
do not synthesize the first column as a key. Until that slice lands, installation
must reject the unsupported storage case explicitly. Also resolve rollback to
a package without a Db2 catalog: skipping publication must not silently retain
the newer generation. Keep the 64-generation bound and reference-safe retention.

## Implementation sequence and acceptance

1. Freeze the actual typed blob schema, canonical vectors and strict bounded
   metadata codec within the current catalog owner. Initial private preparation
   cannot advertise install, binder, execution or migration support.
2. Integrate version-selected package references, signature/canonical identity,
   selected-handle validation and retained publication/installer versions.
3. Integrate strict versioned persistence, typed cells, migration, compatibility,
   retained snapshots, restart, rollback, retention and keyless row identity.
4. Bind and execute only validated typed metadata through the existing provider
   route after the exact common/deferred 174-row obligation freeze.

Each stage has its own declared feature seal. Required affected evidence includes
schema/runtime parity, all bounds and duplicates, canonical vectors and field
sensitivity, hostile signed metadata rejection, denial-before-mutation, atomic
failure/CAS, adopted legacy recovery and retained rollback. Persistence changes
require SQLite/process restart and affected PostgreSQL concurrent-instance,
restart and backup/restore evidence; memory tests alone cannot establish these.
Use existing schema/catalog/package/provider-row/retention gates and CI selectors.
An unchanged unrelated architecture blocker is not silently waived or retried.

This proposal does not accept a full common subset, change the 174-row
denominator, grant a statement-row pass or waive licensed Db2 13 differential.

## Sources

Offline Db2 13 baseline `ibm-db2-for-zos-13-2026-08-13`, product
`SSEPEK_13.0.0`, topic-set digest
`83f04d82753771425aab203c7913dd9fea1c768766cf315df421100015e72463`.
Matching local HTML was verified and read; the ordinary reader remains
TOC-blocked. Source presence is not execution evidence.

- SQL0050 `sqlref/src/tpc/db2z_sql_createtable.html`, 874327 bytes,
  `104cc7fd0f43e804819da99c18887de60983cad8fa78b7d550ffaf63dfd299d6`:
  explicit column types/attributes and optional primary-key syntax.
- `sqlref/src/tpc/db2z_sqlidentifiers.html`, 11729 bytes,
  `d5c99a5640234e19a310d2e44f1abd9bbe3bf9c0fea1cf87c06b24ee9f4a1f8b`:
  SQL versus host identifiers, folding and effective delimited values.
- `sqlref/src/tpc/db2z_datatypesintro.html`, 22904 bytes,
  `a488006755eedd9ef58da3ba8ef9f304a3d79c3910cc39da637dda1f3c38f570`:
  scalar families and attributes; no type inference from host framing.

Retain ADR-0003/0004/0009/0011 and the accepted application catalog, package,
provider-row, durable-storage, canonical-effect, security and retention contracts.
